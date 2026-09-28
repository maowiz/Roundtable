//! Google Gemini generateContent adapter: `responseMimeType` JSON + `responseSchema`.
//! Gemini supports a subset of JSON Schema; the paper schemas stay within it (no `const`
//! is sent: it is rewritten to an enum with one value, `additionalProperties` is dropped).

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

pub struct GoogleAdapter {
    pub catalog: ProviderCatalog,
}

/// Rewrite a draft 2020-12 schema into the Gemini-supported subset.
pub fn to_gemini_schema(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, val) in m {
                match k.as_str() {
                    "$schema" | "$id" | "additionalProperties" | "title" | "pattern" | "minLength" | "maxLength" | "maxItems" | "minItems" => continue,
                    "const" => {
                        out.insert("type".into(), json!("string"));
                        out.insert("enum".into(), json!([val]));
                    }
                    "type" if val.is_array() => {
                        // ["string","null"] -> string + nullable
                        let types: Vec<&str> = val.as_array().unwrap().iter().filter_map(Value::as_str).collect();
                        let non_null: Vec<&&str> = types.iter().filter(|t| **t != "null").collect();
                        out.insert("type".into(), json!(non_null.first().copied().unwrap_or(&"string")));
                        if types.contains(&"null") {
                            out.insert("nullable".into(), json!(true));
                        }
                    }
                    _ => {
                        out.insert(k.clone(), to_gemini_schema(val));
                    }
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(to_gemini_schema).collect()),
        other => other.clone(),
    }
}

#[async_trait]
impl ProviderAdapter for GoogleAdapter {
    fn id(&self) -> &str {
        "google"
    }

    fn capabilities(&self, _model: &str) -> Capabilities {
        Capabilities { supports_temperature: true, supports_json_schema: true, supports_reasoning_effort: true, supports_streaming: true, max_context_tokens: 1_000_000 }
    }

    fn build_request(&self, env: &Envelope, p: &CallParams) -> Result<WireRequest, AdapterError> {
        if p.settings.web_search {
            return Err(AdapterError::UnsupportedSetting { model: p.model.clone(), setting: "web_search".into() });
        }
        let entry = self.catalog.get("google").ok_or_else(|| AdapterError::UnknownProvider("google".into()))?;
        let mut gen = json!({
            "maxOutputTokens": p.settings.max_output_tokens + shape::JSON_OVERHEAD_TOKENS,
            "responseMimeType": "application/json",
            "responseSchema": to_gemini_schema(&p.schema),
        });
        if let Some(t) = p.settings.temperature {
            gen["temperature"] = json!(t);
        }
        if let Some(e) = &p.settings.reasoning_effort {
            let budget = match e.as_str() { "low" => 1024, "high" => 16384, _ => 4096 };
            gen["thinkingConfig"] = json!({ "thinkingBudget": budget });
        }
        let body = json!({
            "systemInstruction": { "parts": [{ "text": shape::system_with_budget(env, p.settings.max_output_tokens) }] },
            "contents": [{ "role": "user", "parts": [{ "text": shape::user_text(env) }] }],
            "generationConfig": gen,
        });
        Ok(WireRequest { provider: "google".into(), model: p.model.clone(), url: entry.url(&p.model), body, headers: vec![("content-type".into(), "application/json".into())] })
    }

    async fn dispatch(&self, req: &WireRequest, secret: &Secret, deadline: Duration, cancel: CancellationToken) -> AttemptResult {
        let auth = ("x-goog-api-key", secret.expose().to_string());
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
    let um = &v["usageMetadata"];
    let usage = Usage {
        provider_cost_micro: None,
        input_tokens: um["promptTokenCount"].as_u64(),
        cached_input_tokens: um["cachedContentTokenCount"].as_u64(),
        output_tokens: um["candidatesTokenCount"].as_u64(),
        reasoning_tokens: um["thoughtsTokenCount"].as_u64(),
    };
    let returned_model = v["modelVersion"].as_str().map(str::to_string);
    let cand = &v["candidates"][0];
    let finish = cand["finishReason"].as_str().map(str::to_string);
    let base = |outcome, cat: Option<ErrorCategory>, raw: Option<String>, msg: Option<String>| AttemptResult { web_sources: Vec::new(), outcome, raw_output: raw, usage, provider_request_id: request_id.clone(), returned_model: returned_model.clone(), finish_reason: finish.clone(), error_category: cat, error_sanitized: msg.map(crate::sanitize), first_byte_ms: Some(first_byte_ms), total_ms, http_status: Some(status) };
    if v["promptFeedback"]["blockReason"].is_string() {
        return base(AttemptOutcome::Refused, Some(ErrorCategory::Refusal), None, Some("prompt blocked".into()));
    }
    match finish.as_deref() {
        Some("SAFETY") | Some("RECITATION") | Some("PROHIBITED_CONTENT") => base(AttemptOutcome::Refused, Some(ErrorCategory::Refusal), None, Some("blocked by safety".into())),
        Some("MAX_TOKENS") => base(AttemptOutcome::Truncated, Some(ErrorCategory::Truncated), None, Some("max tokens".into())),
        _ => {
            let text = cand["content"]["parts"].as_array().map(|a| a.iter()).into_iter().flatten().filter(|p| p["thought"] != true).filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("");
            if text.is_empty() {
                base(AttemptOutcome::InvalidOutput, Some(ErrorCategory::Malformed), None, Some("no text part".into()))
            } else {
                base(AttemptOutcome::Accepted, None, Some(crate::bound_output(text)), None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_rewritten_into_the_gemini_subset() {
        let s: Value = serde_json::from_str(rt_schemas::REVIEW_V1).unwrap();
        let g = to_gemini_schema(&s);
        let text = g.to_string();
        assert!(!text.contains("additionalProperties"));
        assert!(!text.contains("\"const\""));
        assert!(!text.contains("\"pattern\""));
        assert_eq!(g["properties"]["schema_version"]["enum"][0], "review.v1");
        assert_eq!(g["properties"]["self_rating"]["nullable"], true);
        assert_eq!(g["properties"]["self_rating"]["type"], "number");
    }

    #[test]
    fn interprets_candidates_and_finish_reasons() {
        let ok = r#"{"modelVersion":"gemini-x-001","candidates":[{"finishReason":"STOP","content":{"parts":[{"thought":true,"text":"hmm"},{"text":"{\"a\":1}"}]}}],"usageMetadata":{"promptTokenCount":50,"candidatesTokenCount":10,"thoughtsTokenCount":5}}"#;
        let r = interpret(ok, None, 1, 2, 200);
        assert_eq!(r.outcome, AttemptOutcome::Accepted);
        assert_eq!(r.raw_output.as_deref(), Some("{\"a\":1}"), "thought parts are excluded");
        assert_eq!(r.usage.reasoning_tokens, Some(5));
        let blocked = r#"{"promptFeedback":{"blockReason":"SAFETY"},"candidates":[]}"#;
        assert_eq!(interpret(blocked, None, 1, 2, 200).outcome, AttemptOutcome::Refused);
        let trunc = r#"{"candidates":[{"finishReason":"MAX_TOKENS","content":{"parts":[{"text":"{"}]}}]}"#;
        assert_eq!(interpret(trunc, None, 1, 2, 200).outcome, AttemptOutcome::Truncated);
    }
}
