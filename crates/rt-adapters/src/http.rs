//! One HTTP request per attempt. No retries, no redirects, fixed deadline, cancellable.

use crate::{bound_output, classify_status, AttemptResult, ErrorCategory, ProviderCatalog};
use rt_core::AttemptOutcome;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .use_rustls_tls()
            .https_only(true)
            .pool_max_idle_per_host(4)
            .user_agent("Roundtable/0.1 (Windows)")
            .build()
            .expect("http client")
    })
}

/// Raw response the adapter then interprets.
pub struct RawResponse {
    pub status: u16,
    pub body: String,
    pub request_id: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub first_byte_ms: u64,
    pub total_ms: u64,
}

pub enum HttpOutcome {
    Response(RawResponse),
    Failed(AttemptResult),
}

/// POST JSON with the given auth header. `auth` is (header name, header value) built by the adapter
/// from the secret; it exists only for the duration of this call.
pub async fn post_json(catalog: &ProviderCatalog, url: &str, headers: &[(String, String)], auth: (&str, String), body: &serde_json::Value, deadline: Duration, cancel: CancellationToken) -> HttpOutcome {
    let started = Instant::now();
    if !catalog.allows(url) {
        return HttpOutcome::Failed(AttemptResult::failed(AttemptOutcome::NonRetryable, ErrorCategory::InvalidRequest, format!("refused host in {url}"), 0, None));
    }
    let mut req = client().post(url).timeout(deadline).json(body);
    for (k, v) in headers {
        req = req.header(k, v);
    }
    req = req.header(auth.0, auth.1);
    let send = req.send();
    let resp = tokio::select! {
        _ = cancel.cancelled() => {
            return HttpOutcome::Failed(AttemptResult::failed(AttemptOutcome::Uncertain, ErrorCategory::Cancelled, "cancelled by user", started.elapsed().as_millis() as u64, None));
        }
        r = send => r,
    };
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            let ms = started.elapsed().as_millis() as u64;
            // reqwest cannot always tell whether bytes left the machine; connect errors are safe,
            // everything else after a connection is treated as uncertain (PRD 8.3).
            let (outcome, cat) = if e.is_connect() {
                (AttemptOutcome::TransportBeforeSend, ErrorCategory::TransportBeforeSend)
            } else if e.is_timeout() {
                (AttemptOutcome::Uncertain, ErrorCategory::Timeout)
            } else {
                (AttemptOutcome::Uncertain, ErrorCategory::TransportAfterSend)
            };
            return HttpOutcome::Failed(AttemptResult::failed(outcome, cat, e.to_string(), ms, None));
        }
    };
    let first_byte_ms = started.elapsed().as_millis() as u64;
    let status = resp.status().as_u16();
    let request_id = ["x-request-id", "request-id", "openai-request-id", "anthropic-request-id"].iter().find_map(|h| resp.headers().get(*h).and_then(|v| v.to_str().ok()).map(str::to_string));
    let retry_after_ms = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse::<f64>().ok()).map(|secs| (secs * 1000.0) as u64);
    let body = tokio::select! {
        _ = cancel.cancelled() => {
            return HttpOutcome::Failed(AttemptResult::failed(AttemptOutcome::Uncertain, ErrorCategory::Cancelled, "cancelled while reading response", started.elapsed().as_millis() as u64, Some(status)));
        }
        b = resp.text() => b,
    };
    let body = match body {
        Ok(b) => bound_output(b),
        Err(e) => {
            // Providers send the headers at once and the body when the model finishes. Hitting the time limit
            // while waiting surfaces from reqwest as "error decoding response body"; name it for what it is.
            let ms = started.elapsed().as_millis() as u64;
            let timed_out = e.is_timeout() || ms + 1_000 >= deadline.as_millis() as u64;
            let (cat, msg) = if timed_out {
                (ErrorCategory::Timeout, format!("no complete reply within {} seconds; the model was still writing", deadline.as_secs()))
            } else {
                (ErrorCategory::TransportAfterSend, e.to_string())
            };
            return HttpOutcome::Failed(AttemptResult::failed(AttemptOutcome::Uncertain, cat, msg, ms, Some(status)));
        }
    };
    HttpOutcome::Response(RawResponse { status, body, request_id, retry_after_ms, first_byte_ms, total_ms: started.elapsed().as_millis() as u64 })
}

/// Turn a non-2xx response into a failed attempt with the shared classification.
pub fn failure_from_status(r: &RawResponse, message: &str) -> AttemptResult {
    let (outcome, cat) = classify_status(r.status, r.retry_after_ms);
    let mut a = AttemptResult::failed(outcome, cat, format!("HTTP {}: {}", r.status, message), r.total_ms, Some(r.status));
    a.provider_request_id = r.request_id.clone();
    a.first_byte_ms = Some(r.first_byte_ms);
    a
}
