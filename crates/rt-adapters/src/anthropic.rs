//! Anthropic Messages API adapter. Structured output through a single forced tool whose
//! input schema is the paper schema; the tool input is the paper. This works on every
//! Claude model that supports tools and avoids depending on a newer output_format feature.

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

pub struct AnthropicAdapter {
    pub catalog: ProviderCatalog,
}

const TOOL_NAME: &str = "submit_paper";

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn capabilities(&self, _model: &str) -> Capabilities {
        Capabilities { supports_temperature: true, supports_json_schema: true, supports_reasoning_effort: true, supports_streaming: true, max_context_tokens: 200_000 }
    }

    fn build_request(&self, env: &Envelope, p: &CallParams) -> Result<WireRequest, AdapterError> {
        if p.settings.web_search {
            return Err(AdapterError::UnsupportedSetting { model: p.model.clone(), setting: "web_search".into() });
        }
        let entry = self.catalog.get("anthropic").ok_or_else(|| AdapterError::UnknownProvider("anthropic".into()))?;
        let mut body = json!({
            "model": p.model,
            "max_tokens": p.settings.max_output_tokens + shape::JSON_OVERHEAD_TOKENS,
            "system": shape::system_with_budget(env, p.settings.max_output_tokens),
            "messages": [{ "role": "user", "content": shape::user_text(env) }],
            "tools": [{ "name": TOOL_NAME, "description": "Submit the paper as structured JSON.", "input_schema": p.schema }],
            "tool_choice": { "type": "tool", "name": TOOL_NAME },
        });
        if let Some(t) = p.settings.temperature {
            body["temperature"] = json!(t);
        }
        if let Some(e) = &p.settings.reasoning_effort {
            // extended thinking budget scaled from effort; temperature must be unset with thinking
            let budget = match e.as_str() { "low" => 2048, "high" => 16000, _ => 8000 };
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
            body.as_object_mut().unwrap().remove("temperature");
        }
        let mut headers = vec![("content-type".into(), "application/json".into())];
        if let Some(v) = &entry.version_header {
            headers.push(("anthropic-version".into(), v.clone()));
        }
        Ok(WireRequest { provider: "anthropic".into(), model: p.model.clone(), url: entry.url(&p.model), body, headers })
    }

    async fn dispatch(&self, req: &WireRequest, secret: &Secret, deadline: Duration, cancel: CancellationToken) -> AttemptResult {
        let auth = ("x-api-key", secret.expose().to_string());
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

pub fn interpret(body: &str, request_id: Option<String>, first_byte_ms: u64, total_ms: u64, status: u16) -> AttemptResult {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return AttemptResult::failed(AttemptOutcome::InvalidOutput, ErrorCategory::Malformed, format!("unparseable response: {e}"), total_ms, Some(status)),
    };
    let usage = Usage {
        provider_cost_micro: None,
        input_tokens: v["usage"]["input_tokens"].as_u64().map(|i| i + v["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0)),
        cached_input_tokens: v["usage"]["cache_read_input_tokens"].as_u64(),
        output_tokens: v["usage"]["output_tokens"].as_u64(),
        reasoning_tokens: None,
    };
    let returned_model = v["model"].as_str().map(str::to_string);
    let stop = v["stop_reason"].as_str().map(str::to_string);
    let base = |outcome, cat: Option<ErrorCategory>, raw: Option<String>, msg: Option<String>| AttemptResult { web_sources: Vec::new(), outcome, raw_output: raw, usage, provider_request_id: request_id.clone(), returned_model: returned_model.clone(), finish_reason: stop.clone(), error_category: cat, error_sanitized: msg.map(crate::sanitize), first_byte_ms: Some(first_byte_ms), total_ms, http_status: Some(status) };

    let tool_input = v["content"].as_array().map(|a| a.iter()).into_iter().flatten().find(|c| c["type"] == "tool_use" && c["name"] == TOOL_NAME).map(|c| c["input"].clone());
    match (stop.as_deref(), tool_input) {
        (Some("refusal"), _) => base(AttemptOutcome::Refused, Some(ErrorCategory::Refusal), None, Some("model refused".into())),
        (Some("max_tokens"), _) => base(AttemptOutcome::Truncated, Some(ErrorCategory::Truncated), None, Some("max_tokens reached".into())),
        (_, Some(input)) => base(AttemptOutcome::Accepted, None, Some(crate::bound_output(input.to_string())), None),
        (_, None) => base(AttemptOutcome::InvalidOutput, Some(ErrorCategory::Malformed), None, Some("no tool_use block".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_tool_use_success_and_stop_reasons() {
        let ok = r#"{"model":"claude-x","stop_reason":"tool_use","content":[{"type":"text","text":"thinking"},{"type":"tool_use","name":"submit_paper","input":{"schema_version":"answer.v1"}}],"usage":{"input_tokens":80,"cache_read_input_tokens":20,"output_tokens":30}}"#;
        let r = interpret(ok, None, 1, 2, 200);
        assert_eq!(r.outcome, AttemptOutcome::Accepted);
        assert!(r.raw_output.unwrap().contains("answer.v1"));
        assert_eq!(r.usage.input_tokens, Some(100));
        assert_eq!(r.usage.cached_input_tokens, Some(20));
        let trunc = r#"{"stop_reason":"max_tokens","content":[],"usage":{}}"#;
        assert_eq!(interpret(trunc, None, 1, 2, 200).outcome, AttemptOutcome::Truncated);
        let refused = r#"{"stop_reason":"refusal","content":[],"usage":{}}"#;
        assert_eq!(interpret(refused, None, 1, 2, 200).outcome, AttemptOutcome::Refused);
    }

    #[test]
    fn build_request_uses_version_header_and_forced_tool() {
        use rt_core::RoleSettings;
        let a = AnthropicAdapter { catalog: ProviderCatalog::bundled() };
        let env = Envelope { call: rt_envelope::CallSpec::Answer { seat: 0 }, stage: rt_core::Stage::Answer, instructions: "SYS".into(), messages: vec![rt_envelope::Message { role: "user".into(), content: "Q".into() }], schema_id: "answer.v1".into(), allowed_view_ids: vec![], alias_map: Default::default(), presentation_order: vec![], digest: String::new(), views: vec![], validation: Default::default() };
        let p = CallParams { model: "claude-sonnet-5".into(), settings: RoleSettings { temperature: Some(0.2), max_output_tokens: 500, reasoning_effort: None, web_search: false }, schema: json!({"type":"object"}), schema_name: "answer".into(), json_schema: true, truncation_retries: 0 };
        let w = a.build_request(&env, &p).unwrap();
        assert!(w.headers.iter().any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
        assert_eq!(w.body["tool_choice"]["name"], "submit_paper");
        assert_eq!(w.body["messages"].as_array().unwrap().len(), 1);
        assert!(!serde_json::to_string(&w).unwrap().contains("x-api-key"));
    }
}
