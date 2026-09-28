//! IPC payload types. camelCase on the wire; every field the UI needs and nothing secret.

use rt_core::{CallState, LogicalCallId, MicroUsd, PaperId, RunId, RunState, Stage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMeta {
    pub r#ref: String,
    pub provider: String,
    pub label: String,
    pub created_at: String,
    pub last_tested_at: Option<String>,
    pub last_test_result: Option<String>,
    /// Whether Credential Manager actually holds a secret for this reference.
    pub present: bool,
    /// First 12 and last 3 characters of the stored key, like OpenRouter's own key list. Never the key.
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    pub category: Option<String>,
    pub message: String,
    pub returned_model: Option<String>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleTuning {
    pub temperature: Option<f32>,
    pub max_output_words: u32,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatDoc {
    pub label: String,
    pub provider: String,
    pub credential_ref: String,
    pub model_id: String,
    #[serde(default)]
    pub model_is_alias: bool,
    pub answer: RoleTuning,
    pub review: RoleTuning,
    pub improve: RoleTuning,
    pub prompt_answer: String,
    pub prompt_review: String,
    pub prompt_improve: String,
    /// Dollars as a string, e.g. "0.40" (exact).
    pub cost_cap_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizerDoc {
    pub provider: String,
    pub credential_ref: String,
    pub model_id: String,
    #[serde(default)]
    pub model_is_alias: bool,
    pub tuning: RoleTuning,
    pub prompt: String,
}

/// Which stages may use provider-side web search. Panel-wide: every seat's call in that stage.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchDoc {
    #[serde(default)]
    pub answer: bool,
    #[serde(default)]
    pub review: bool,
    #[serde(default)]
    pub improve: bool,
    #[serde(default)]
    pub synthesize: bool,
}

impl WebSearchDoc {
    /// Calls that will carry the search fee for a panel of `n` seats.
    pub fn calls(&self, n: usize) -> usize {
        (if self.answer { n } else { 0 }) + (if self.review { n * n.saturating_sub(1) } else { 0 }) + (if self.improve { n } else { 0 }) + usize::from(self.synthesize)
    }
    pub fn any(&self) -> bool {
        self.answer || self.review || self.improve || self.synthesize
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelDoc {
    pub id: Option<String>,
    pub name: String,
    pub seats: Vec<SeatDoc>,
    pub synthesizer: SynthesizerDoc,
    #[serde(default)]
    pub web_search: WebSearchDoc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelSummary {
    pub id: String,
    pub name: String,
    pub seat_count: usize,
    pub doc: PanelDoc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub model_id: String,
    pub price_verified: bool,
    pub supports_temperature: bool,
    pub supports_reasoning_effort: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightRequest {
    pub question: String,
    pub reference_text: Option<String>,
    pub panel_id: String,
    pub ceiling_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightReport {
    pub ok: bool,
    pub seat_count: usize,
    pub base_calls: usize,
    pub max_attempts: usize,
    pub estimate_low_usd: String,
    pub estimate_high_usd: String,
    pub unverified_prices: Vec<String>,
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    /// Calls that will run with web search, and their flat fee (already inside the estimates).
    pub web_search_calls: usize,
    pub web_search_fee_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub id: RunId,
    pub question: String,
    pub state: RunState,
    pub stage: Stage,
    pub reason_code: Option<String>,
    pub reason_text: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub seat_count: usize,
    pub is_sample: bool,
    pub settled_usd: String,
    pub unresolved_usd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatSummary {
    pub index: u8,
    pub label: String,
    pub provider: String,
    pub model_id: String,
    pub model_returned_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallSummary {
    pub id: LogicalCallId,
    pub stage: Stage,
    pub reviewer_seat: u8,
    pub target_seat: Option<u8>,
    pub rotation: Option<u8>,
    pub state: CallState,
    pub attempts_used: u8,
    pub accepted_paper_id: Option<PaperId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperMeta {
    pub id: PaperId,
    pub kind: String,
    pub author_seat: u8,
    pub target_seat: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Coverage {
    pub stage: Stage,
    pub planned: usize,
    pub accepted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSnapshot {
    pub run: RunSummary,
    pub seats: Vec<SeatSummary>,
    pub synthesizer: Value,
    pub calls: Vec<CallSummary>,
    pub papers: Vec<PaperMeta>,
    pub coverage: Vec<Coverage>,
    pub last_seq: i64,
    pub final_paper: Option<Value>,
    pub web_search: WebSearchDoc,
    /// Tokens reported by the providers so far, across every attempt of this run.
    pub usage: Value,
    /// Requests out right now: which seat, which step, which attempt, since when.
    pub in_flight: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventOut {
    pub seq: i64,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaperDetail {
    pub paper: PaperMeta,
    pub body: Value,
    pub digest: String,
    /// The anonymous views built from this paper (what other models saw).
    pub views: Vec<Value>,
    /// The envelope the producing call received.
    pub envelope: Option<Value>,
    pub attempts: Vec<Value>,
}

pub fn usd(m: MicroUsd) -> String {
    let v = m.0.unsigned_abs();
    format!("{}{}.{:02}", if m.0 < 0 { "-" } else { "" }, v / 1_000_000, (v % 1_000_000) / 10_000)
}
