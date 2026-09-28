//! Provider adapters (backend plan section 7).
//!
//! An adapter turns an [`Envelope`] into a [`WireRequest`] without any secret, and then
//! dispatches it exactly once per attempt. Keys are injected only inside `dispatch`.
//! No adapter retries, follows redirects or adds hidden history (ISO-08, SEC-03).

pub mod anthropic;
pub mod catalog;
pub mod fake;
pub mod google;
pub mod http;
pub mod openai;
pub mod openai_compat;

use async_trait::async_trait;
use rt_budget::Usage;
use rt_core::{AttemptOutcome, RoleSettings};
use rt_envelope::Envelope;
use rt_secrets::Secret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub use catalog::{ProviderCatalog, ProviderEntry};

/// What a model supports. Used by preflight to reject unsupported settings (PAN-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub supports_temperature: bool,
    pub supports_json_schema: bool,
    pub supports_reasoning_effort: bool,
    pub supports_streaming: bool,
    pub max_context_tokens: u64,
}

/// Per-call parameters the engine passes alongside the envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallParams {
    pub model: String,
    pub settings: RoleSettings,
    /// The JSON Schema for the expected paper (already parsed).
    pub schema: Value,
    pub schema_name: String,
    /// False when the provider cannot enforce the schema for this model: adapters fall back to
    /// JSON mode with the schema in the instructions, and the native validator still decides.
    /// How many earlier attempts of this call were cut off at the output limit. Adapters that leave
    /// headroom for hidden reasoning double it per truncation, so a retry is not the same doomed request.
    #[serde(default)]
    pub truncation_retries: u8,
    #[serde(default = "default_true")]
    pub json_schema: bool,
}

fn default_true() -> bool {
    true
}

/// A fully built request with the credential removed. Persisted redacted (PRD 12.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireRequest {
    pub provider: String,
    pub model: String,
    pub url: String,
    pub body: Value,
    /// Non-secret headers (content-type, version headers). Never Authorization or api keys.
    pub headers: Vec<(String, String)>,
}

impl WireRequest {
    pub fn digest(&self) -> String {
        rt_core::digest(rt_core::canonical_json(&serde_json::json!({ "url": self.url, "body": self.body, "headers": self.headers })).as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Auth,
    Quota,
    RateLimit,
    ModelUnavailable,
    InvalidRequest,
    TransportBeforeSend,
    TransportAfterSend,
    Timeout,
    Cancelled,
    Refusal,
    Truncated,
    Malformed,
    Server,
}

/// The result of exactly one provider request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptResult {
    pub outcome: AttemptOutcome,
    /// Web pages the provider's search attached to this call (empty when search was off or unused).
    #[serde(default)]
    pub web_sources: Vec<WebSource>,
    /// Text the model produced, bounded by the adapter to MAX_RESPONSE_BYTES.
    pub raw_output: Option<String>,
    pub usage: Usage,
    pub provider_request_id: Option<String>,
    pub returned_model: Option<String>,
    pub finish_reason: Option<String>,
    pub error_category: Option<ErrorCategory>,
    /// Sanitized: status code and a short provider message, never headers or bodies with secrets.
    pub error_sanitized: Option<String>,
    pub first_byte_ms: Option<u64>,
    pub total_ms: u64,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSource {
    pub url: String,
    pub title: String,
}

impl AttemptResult {
    pub fn failed(outcome: AttemptOutcome, category: ErrorCategory, msg: impl Into<String>, total_ms: u64, status: Option<u16>) -> Self {
        AttemptResult { outcome, web_sources: Vec::new(), raw_output: None, usage: Usage::default(), provider_request_id: None, returned_model: None, finish_reason: None, error_category: Some(category), error_sanitized: Some(sanitize(msg.into())), first_byte_ms: None, total_ms, http_status: status }
    }
}

/// Keep error text short and free of anything that looks like a key or token (SEC-06).
pub fn sanitize(msg: String) -> String {
    let mut out = String::with_capacity(msg.len().min(300));
    for word in msg.split_whitespace() {
        let looks_secret = word.len() > 24 && word.chars().filter(|c| c.is_ascii_alphanumeric()).count() > 20 || word.starts_with("sk-") || word.starts_with("Bearer");
        if looks_secret {
            out.push_str("[redacted]");
        } else {
            out.push_str(word);
        }
        out.push(' ');
        if out.len() > 280 {
            out.push('…');
            break;
        }
    }
    out.trim_end().to_string()
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self, model: &str) -> Capabilities;
    /// Build the request. Must be a pure function of its inputs (ISO-10 relies on it).
    fn build_request(&self, env: &Envelope, params: &CallParams) -> Result<WireRequest, AdapterError>;
    /// Make exactly one HTTP request. The secret is used and dropped here.
    async fn dispatch(&self, req: &WireRequest, secret: &Secret, deadline: Duration, cancel: CancellationToken) -> AttemptResult;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AdapterError {
    #[error("host {0} is not in the provider catalog")]
    UnknownHost(String),
    #[error("model {model} does not support {setting}")]
    UnsupportedSetting { model: String, setting: String },
    #[error("provider {0} is not configured")]
    UnknownProvider(String),
}

/// Shared helpers for turning an envelope into provider-neutral pieces.
pub mod shape {
    use rt_envelope::Envelope;

    /// The single system/instruction string.
    pub fn system(env: &Envelope) -> &str {
        &env.instructions
    }

    /// Instructions plus an explicit output budget so models do not run past `max_output_tokens`.
    pub fn system_with_budget(env: &Envelope, max_output_tokens: u32) -> String {
        let words = (max_output_tokens as f64 / 1.4 * 0.7).round() as u32;
        format!("{}\n\n### OUTPUT BUDGET\nThe whole JSON response must stay under {max_output_tokens} tokens. Keep the answer field under about {words} words and every list short. A shorter complete response is far better than a longer truncated one.", env.instructions)
    }

    /// Tokens to allow beyond the paper itself: JSON wrapper, ids and lists.
    pub const JSON_OVERHEAD_TOKENS: u32 = 700;

    /// User content: every message joined with blank lines, in order. Providers that accept
    /// multiple user turns still receive one turn; there is no assistant history (ISO-01).
    pub fn user_text(env: &Envelope) -> String {
        env.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n\n")
    }
}

/// Map an HTTP status to an outcome and category shared by all adapters.
pub fn classify_status(status: u16, retry_after_ms: Option<u64>) -> (AttemptOutcome, ErrorCategory) {
    match status {
        401 | 403 => (AttemptOutcome::NonRetryable, ErrorCategory::Auth),
        402 => (AttemptOutcome::NonRetryable, ErrorCategory::Quota),
        404 => (AttemptOutcome::NonRetryable, ErrorCategory::ModelUnavailable),
        429 => (AttemptOutcome::RateLimited { retry_after_ms }, ErrorCategory::RateLimit),
        400 | 422 => (AttemptOutcome::NonRetryable, ErrorCategory::InvalidRequest),
        408 | 500 | 502 | 503 | 504 | 529 => (AttemptOutcome::TransientServer, ErrorCategory::Server),
        _ => (AttemptOutcome::NonRetryable, ErrorCategory::Server),
    }
}

/// Truncate raw output to the retention bound without breaking UTF-8.
pub fn bound_output(s: String) -> String {
    let max = rt_core::bounds::MAX_RESPONSE_BYTES;
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_redacts_key_like_words_and_bounds_length() {
        let s = sanitize("Invalid key sk-abcdefghijklmnopqrstuvwxyz0123456789 given Bearer xyz".into());
        assert!(!s.contains("sk-abc"));
        assert!(s.contains("[redacted]"));
        let long = sanitize("word ".repeat(200));
        assert!(long.len() <= 300);
    }

    #[test]
    fn status_classification_matches_retry_table() {
        assert_eq!(classify_status(401, None).0, AttemptOutcome::NonRetryable);
        assert_eq!(classify_status(429, Some(2000)).0, AttemptOutcome::RateLimited { retry_after_ms: Some(2000) });
        assert_eq!(classify_status(503, None).0, AttemptOutcome::TransientServer);
        assert_eq!(classify_status(400, None).1, ErrorCategory::InvalidRequest);
    }

    #[test]
    fn output_is_bounded_on_char_boundaries() {
        let s = "é".repeat(rt_core::bounds::MAX_RESPONSE_BYTES);
        let b = bound_output(s);
        assert!(b.len() <= rt_core::bounds::MAX_RESPONSE_BYTES);
        assert!(std::str::from_utf8(b.as_bytes()).is_ok());
    }
}
