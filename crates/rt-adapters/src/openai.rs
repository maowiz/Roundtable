//! OpenAI Responses API adapter: structured output via `text.format` json_schema (strict).

use crate::http::{failure_from_status, post_json, HttpOutcome};
use crate::{shape, AdapterError, AttemptResult, CallParams, Capabilities, ErrorCategory, ProviderAdapter, ProviderCatalog, WireRequest};
use async_trait::async_trait;
use rt_budget::Usage;
use rt_core::AttemptOutcome;
use rt_envelope::Envelope;
use rt_secrets::Secret;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct OpenAiAdapter {
    pub catalog: ProviderCatalog,
}

impl OpenAiAdapter {
    fn is_reasoning_model(model: &str) -> bool {
        let m = model.to_lowercase();
        m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") || m.starts_with("gpt-5")
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiAdapter {
    fn id(&self) -> &str {
        "openai"
    }

    fn capabilities(&self, model: &str) -> Capabilities {
        let reasoning = Self::is_reasoning_model(model);
        Capabilities { supports_temperature: !reasoning, supports_json_schema: true, supports_reasoning_effort: reasoning, supports_streaming: true, max_context_tokens: 200_000 }
    }

    fn build_request(&self, env: &Envelope, p: &CallParams) -> Result<WireRequest, AdapterError> {
        if p.settings.web_search {
            return Err(AdapterError::UnsupportedSetting { model: p.model.clone(), setting: "web_search".into() });
        }
        let entry = self.catalog.get("openai").ok_or_else(|| AdapterError::UnknownProvider("openai".into()))?;
        let caps = self.capabilities(&p.model);
        let mut body = json!({
            "model": p.model,
            "instructions": shape::system_with_budget(env, p.settings.max_output_tokens),
            "input": [{ "role": "user", "content": shape::user_text(env) }],
            "max_output_tokens": p.settings.max_output_tokens + shape::JSON_OVERHEAD_TOKENS,
            "text": { "format": { "type": "json_schema", "name": p.schema_name, "schema": p.schema, "strict": true } },
            "store": false,
        });
        if let Some(t) = p.settings.temperature {
            if !caps.supports_temperature {
                return Err(AdapterError::UnsupportedSetting { model: p.model.clone(), setting: "temperature".into() });
            }
            body["temperature"] = json!(t);
        }
        if let Some(e) = &p.settings.reasoning_effort {
            if !caps.supports_reasoning_effort {
                return Err(AdapterError::UnsupportedSetting { model: p.model.clone(), setting: "reasoning_effort".into() });
            }
            body["reasoning"] = json!({ "effort": e });
        }
        Ok(WireRequest { provider: "openai".into(), model: p.model.clone(), url: entry.url(&p.model), body, headers: vec![("content-type".into(), "application/json".into())] })
    }

    async fn dispatch(&self, req: &WireRequest, secret: &Secret, deadline: Duration, cancel: CancellationToken) -> AttemptResult {
        let auth = ("authorization", format!("Bearer {}", secret.expose()));
        let raw = match post_json(&self.catalog, &req.url, &req.headers, auth, &req.body, deadline, cancel).await {
            HttpOutcome::Failed(f) => return f,
            HttpOutcome::Response(r) => r,
        };
        if raw.status < 200 || raw.status >= 300 {
            let msg = serde_json::from_str::<Value>(&raw.body).ok().and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or_default();
            return failure_from_status(&raw, &msg);
        }
        interpret(&raw.body, raw.request_id, raw.first_byte_ms, raw.total_ms, raw.status)
    }
}

/// Interpret a Responses API success body. Public so fixtures can exercise it.
pub fn interpret(body: &str, request_id: Option<String>, first_byte_ms: u64, total_ms: u64, status: u16) -> AttemptResult {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return AttemptResult::failed(AttemptOutcome::InvalidOutput, ErrorCategory::Malformed, format!("unparseable response: {e}"), total_ms, Some(status)),
    };
    let usage = Usage {
        provider_cost_micro: None,
        input_tokens: v["usage"]["input_tokens"].as_u64(),
        cached_input_tokens: v["usage"]["input_tokens_details"]["cached_tokens"].as_u64(),
        output_tokens: v["usage"]["output_tokens"].as_u64().map(|o| o.saturating_sub(v["usage"]["output_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0))),
        reasoning_tokens: v["usage"]["output_tokens_details"]["reasoning_tokens"].as_u64(),
    };
    let returned_model = v["model"].as_str().map(str::to_string);
    let base = |outcome, cat: Option<ErrorCategory>, raw: Option<String>, finish: Option<String>, msg: Option<String>| AttemptResult { web_sources: Vec::new(), outcome, raw_output: raw, usage, provider_request_id: request_id.clone(), returned_model: returned_model.clone(), finish_reason: finish, error_category: cat, error_sanitized: msg.map(crate::sanitize), first_byte_ms: Some(first_byte_ms), total_ms, http_status: Some(status) };

    let status_field = v["status"].as_str().unwrap_or("completed");
    let incomplete_reason = v["incomplete_details"]["reason"].as_str().map(str::to_string);
    // find refusal or text in output items
    let mut text: Option<String> = None;
    let mut refusal: Option<String> = None;
    for item in v["output"].as_array().map(|a| a.iter()).into_iter().flatten() {
        if item["type"] == "message" {
            for c in item["content"].as_array().map(|a| a.iter()).into_iter().flatten() {
                match c["type"].as_str() {
                    Some("output_text") => text = c["text"].as_str().map(str::to_string),
                    Some("refusal") => refusal = c["refusal"].as_str().map(str::to_string),
                    _ => {}
                }
            }
        }
    }
    if let Some(r) = refusal {
        return base(AttemptOutcome::Refused, Some(ErrorCategory::Refusal), None, Some("refusal".into()), Some(r));
    }
    if status_field == "incomplete" {
        let reason = incomplete_reason.unwrap_or_else(|| "incomplete".into());
        let outcome = if reason == "max_output_tokens" { AttemptOutcome::Truncated } else { AttemptOutcome::InvalidOutput };
        return base(outcome, Some(ErrorCategory::Truncated), text, Some(reason.clone()), Some(format!("incomplete: {reason}")));
    }
    match text {
        Some(t) => base(AttemptOutcome::Accepted, None, Some(crate::bound_output(t)), Some("stop".into()), None),
        None => base(AttemptOutcome::InvalidOutput, Some(ErrorCategory::Malformed), None, None, Some("no output text".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_success_refusal_and_truncation_fixtures() {
        let ok = r#"{"id":"resp_1","model":"gpt-5.6-2026-08-01","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"{\"a\":1}"}]}],"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":20},"output_tokens":50,"output_tokens_details":{"reasoning_tokens":10}}}"#;
        let r = interpret(ok, Some("req_1".into()), 10, 20, 200);
        assert_eq!(r.outcome, AttemptOutcome::Accepted);
        assert_eq!(r.raw_output.as_deref(), Some("{\"a\":1}"));
        assert_eq!(r.usage, Usage { provider_cost_micro: None, input_tokens: Some(100), cached_input_tokens: Some(20), output_tokens: Some(40), reasoning_tokens: Some(10) });
        assert_eq!(r.returned_model.as_deref(), Some("gpt-5.6-2026-08-01"));

        let refused = r#"{"status":"completed","output":[{"type":"message","content":[{"type":"refusal","refusal":"I can't help with that"}]}],"usage":{}}"#;
        assert_eq!(interpret(refused, None, 1, 2, 200).outcome, AttemptOutcome::Refused);

        let truncated = r#"{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"output":[{"type":"message","content":[{"type":"output_text","text":"{\"a\":"}]}],"usage":{}}"#;
        assert_eq!(interpret(truncated, None, 1, 2, 200).outcome, AttemptOutcome::Truncated);

        assert_eq!(interpret("not json", None, 1, 2, 200).outcome, AttemptOutcome::InvalidOutput);
    }

    #[test]
    fn build_request_rejects_temperature_on_reasoning_models_and_never_includes_history() {
        use rt_core::RoleSettings;
        let a = OpenAiAdapter { catalog: ProviderCatalog::bundled() };
        let env = Envelope { call: rt_envelope::CallSpec::Answer { seat: 0 }, stage: rt_core::Stage::Answer, instructions: "SYS".into(), messages: vec![rt_envelope::Message { role: "user".into(), content: "Q".into() }], schema_id: "answer.v1".into(), allowed_view_ids: vec![], alias_map: Default::default(), presentation_order: vec![], digest: String::new(), views: vec![], validation: Default::default() };
        let mut p = CallParams { model: "gpt-5.6".into(), settings: RoleSettings { temperature: Some(0.7), max_output_tokens: 100, reasoning_effort: None, web_search: false }, schema: json!({"type":"object"}), schema_name: "answer".into(), json_schema: true, truncation_retries: 0 };
        assert!(matches!(a.build_request(&env, &p), Err(AdapterError::UnsupportedSetting { .. })));
        p.settings.temperature = None;
        p.settings.reasoning_effort = Some("medium".into());
        let w = a.build_request(&env, &p).unwrap();
        assert_eq!(w.url, "https://api.openai.com/v1/responses");
        assert_eq!(w.body["store"], false);
        assert!(w.body.get("previous_response_id").is_none());
        assert_eq!(w.body["input"].as_array().unwrap().len(), 1);
        assert_eq!(w.body["reasoning"]["effort"], "medium");
        assert!(!serde_json::to_string(&w).unwrap().contains("Bearer"));
    }
}
