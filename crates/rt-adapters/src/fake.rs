//! Fake provider for engine tests: scripted outcomes, recorded wire requests (for canary
//! checks), simulated latency and cancellation.

use crate::{AdapterError, AttemptResult, CallParams, Capabilities, ErrorCategory, ProviderAdapter, WireRequest};
use async_trait::async_trait;
use rt_budget::Usage;
use rt_core::AttemptOutcome;
use rt_envelope::Envelope;
use rt_secrets::Secret;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// How the fake should answer one request.
#[derive(Debug, Clone)]
pub enum Scripted {
    /// Produce this JSON body as the model output.
    Ok(Value),
    /// Produce raw text (possibly invalid JSON).
    Raw(String),
    Fail(AttemptOutcome, ErrorCategory),
    /// Never respond until cancelled or the deadline passes.
    Hang,
}

/// The fake decides per request. `pick` receives the envelope's call and the wire request.
pub type Decider = Arc<dyn Fn(&WireRequest) -> Scripted + Send + Sync>;

pub struct FakeProvider {
    pub id: String,
    pub latency: Duration,
    pub decider: Decider,
    /// Every request in dispatch order, with the secret that was used (to prove keys never leak elsewhere).
    pub requests: Arc<Mutex<Vec<WireRequest>>>,
    pub queue: Arc<Mutex<VecDeque<Scripted>>>,
}

impl FakeProvider {
    /// A fake that answers each request from a queue, falling back to the decider.
    pub fn new(id: &str, decider: Decider) -> Self {
        FakeProvider { id: id.into(), latency: Duration::from_millis(5), decider, requests: Arc::new(Mutex::new(Vec::new())), queue: Arc::new(Mutex::new(VecDeque::new())) }
    }

    pub fn push(&self, s: Scripted) {
        self.queue.lock().unwrap().push_back(s);
    }

    pub fn recorded(&self) -> Vec<WireRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl ProviderAdapter for FakeProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self, _model: &str) -> Capabilities {
        Capabilities { supports_temperature: true, supports_json_schema: true, supports_reasoning_effort: true, supports_streaming: false, max_context_tokens: 100_000 }
    }

    fn build_request(&self, env: &Envelope, p: &CallParams) -> Result<WireRequest, AdapterError> {
        // The fake serializes the envelope faithfully so canary tests inspect what a real body would carry.
        let body = json!({
            "model": p.model,
            "system": env.instructions,
            "messages": env.messages,
            "schema": p.schema_name,
            "temperature": p.settings.temperature,
            "max_tokens": p.settings.max_output_tokens,
            "call": env.call,
        });
        Ok(WireRequest { provider: self.id.clone(), model: p.model.clone(), url: format!("https://fake.invalid/{}", self.id), body, headers: vec![] })
    }

    async fn dispatch(&self, req: &WireRequest, secret: &Secret, deadline: Duration, cancel: CancellationToken) -> AttemptResult {
        assert!(!secret.expose().is_empty(), "dispatch always receives a secret");
        self.requests.lock().unwrap().push(req.clone());
        let scripted = self.queue.lock().unwrap().pop_front().unwrap_or_else(|| (self.decider)(req));
        let wait = match &scripted {
            Scripted::Hang => deadline,
            _ => self.latency,
        };
        tokio::select! {
            _ = cancel.cancelled() => {
                return AttemptResult::failed(AttemptOutcome::Uncertain, ErrorCategory::Cancelled, "cancelled", 0, None);
            }
            _ = tokio::time::sleep(wait) => {}
        }
        let ms = wait.as_millis() as u64;
        match scripted {
            Scripted::Hang => AttemptResult::failed(AttemptOutcome::Uncertain, ErrorCategory::Timeout, "deadline", ms, None),
            Scripted::Fail(outcome, cat) => AttemptResult::failed(outcome, cat, "scripted failure", ms, Some(500)),
            Scripted::Raw(text) => AttemptResult { web_sources: Vec::new(), outcome: AttemptOutcome::Accepted, raw_output: Some(text), usage: Usage { provider_cost_micro: None, input_tokens: Some(100), cached_input_tokens: None, output_tokens: Some(50), reasoning_tokens: None }, provider_request_id: Some("fake-req".into()), returned_model: Some(req.model.clone()), finish_reason: Some("stop".into()), error_category: None, error_sanitized: None, first_byte_ms: Some(1), total_ms: ms, http_status: Some(200) },
            Scripted::Ok(v) => AttemptResult { web_sources: Vec::new(), outcome: AttemptOutcome::Accepted, raw_output: Some(v.to_string()), usage: Usage { provider_cost_micro: None, input_tokens: Some(100), cached_input_tokens: None, output_tokens: Some(50), reasoning_tokens: None }, provider_request_id: Some("fake-req".into()), returned_model: Some(req.model.clone()), finish_reason: Some("stop".into()), error_category: None, error_sanitized: None, first_byte_ms: Some(1), total_ms: ms, http_status: Some(200) },
        }
    }
}
