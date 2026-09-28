//! Run executor (backend plan section 8).
//!
//! One `Engine` owns adapters, secrets, prices and the ledger. `create_run` freezes a run;
//! `execute` drives it stage by stage with the four-transaction commit protocol:
//!   tx1 envelope + call ready · tx2 attempt intent + reservation · tx3 outcome + paper + settlement · tx4 barrier.
//! Events are appended inside the same transactions and broadcast only after commit.

use rt_adapters::{AttemptResult, CallParams, ProviderAdapter, WireRequest};
use rt_budget::{Catalog, RunBudget, Usage};
use rt_core::{bounds, next_call_state, AttemptId, AttemptOutcome, BudgetEntryId, CallState, CredentialRef, EnvelopeId, FrozenSeat, LogicalCallId, MicroUsd, PaperId, RoleSettings, RunEventKind, RunId, RunState, Stage};
use rt_envelope::{CallSpec, Envelope, Paper, PaperKind, Papers, RunContext};
use rt_ledger::runs::{self, AttemptOutcomeRow, AttemptRow, CallRow, PaperRow, RunRow};
use rt_ledger::rusqlite;
use rt_ledger::{Ledger, LedgerError};
use rt_secrets::{Secret, SecretsError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RUBRIC_VERSION: &str = "rubric.v1";

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("ledger: {0}")]
    Ledger(#[from] LedgerError),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("provider {0} is not configured")]
    UnknownProvider(String),
    #[error("secrets: {0}")]
    Secrets(#[from] SecretsError),
    #[error("budget: {0}")]
    Budget(#[from] rt_budget::BudgetError),
    #[error("envelope: {0}")]
    Envelope(#[from] rt_envelope::EnvelopeError),
    #[error("adapter: {0}")]
    Adapter(#[from] rt_adapters::AdapterError),
    #[error("run {0} is {1:?}; cannot execute")]
    WrongState(RunId, RunState),
    #[error("envelope digest mismatch for call {0}: ledger has been altered")]
    DigestMismatch(LogicalCallId),
}

pub type Result<T> = std::result::Result<T, EngineError>;

/// Where secrets come from. Production uses Credential Manager; tests use a map.
pub trait SecretSource: Send + Sync {
    fn read(&self, r: CredentialRef) -> std::result::Result<Secret, SecretsError>;
}

pub struct CredentialManagerSecrets;
impl SecretSource for CredentialManagerSecrets {
    fn read(&self, r: CredentialRef) -> std::result::Result<Secret, SecretsError> {
        rt_secrets::read(r)
    }
}

pub struct MapSecrets(pub HashMap<CredentialRef, String>);
impl SecretSource for MapSecrets {
    fn read(&self, r: CredentialRef) -> std::result::Result<Secret, SecretsError> {
        self.0.get(&r).cloned().ok_or(SecretsError::NotFound(r)).and_then(Secret::new)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Limits {
    pub global_in_flight: usize,
    pub per_provider_in_flight: usize,
    pub attempt_deadline_secs: u64,
    pub run_deadline_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { global_in_flight: bounds::DEFAULT_GLOBAL_IN_FLIGHT, per_provider_in_flight: bounds::DEFAULT_PER_PROVIDER_IN_FLIGHT, attempt_deadline_secs: bounds::DEFAULT_ATTEMPT_DEADLINE_SECS, run_deadline_secs: bounds::DEFAULT_RUN_DEADLINE_SECS }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SynthesizerSeat {
    pub provider: String,
    pub credential_ref: CredentialRef,
    pub model_id: String,
    pub model_is_alias: bool,
    pub settings: RoleSettings,
    pub prompt: String,
}

#[derive(Debug, Clone)]
pub struct RunInput {
    /// Shared system prompts for this run (defaults when the user has not edited them).
    pub system_prompts: rt_envelope::instructions::SystemPrompts,
    pub question: String,
    pub reference_text: Option<String>,
    pub seats: Vec<FrozenSeat>,
    pub synthesizer: SynthesizerSeat,
    pub ceiling: MicroUsd,
    pub seed: Option<String>,
    pub is_sample: bool,
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEvent {
    pub run_id: RunId,
    pub seq: i64,
    pub kind: String,
    pub payload: Value,
}

pub struct Engine {
    pub ledger: Arc<Ledger>,
    adapters: HashMap<String, Arc<dyn ProviderAdapter>>,
    secrets: Arc<dyn SecretSource>,
    pub prices: Catalog,
    events: broadcast::Sender<RunEvent>,
    /// Tests run with unverified seed prices; production leaves this false (BUD-04).
    pub allow_unverified_prices: bool,
}

/// Shared per-run execution state.
struct RunShared {
    engine: Arc<Engine>,
    run: RunRow,
    seats: Vec<FrozenSeat>,
    synth: SynthesizerSeat,
    references: Vec<(String, String)>,
    system: rt_envelope::instructions::SystemPrompts,
    budget: Mutex<RunBudget>,
    limits: Limits,
    cancel: CancellationToken,
    /// Internal stop (budget/deadline) distinct from the user's cancel.
    stop: CancellationToken,
    stop_reason: Mutex<Option<(String, String)>>,
    global: Arc<Semaphore>,
    per_provider: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl Engine {
    pub fn new(ledger: Arc<Ledger>, adapters: Vec<Arc<dyn ProviderAdapter>>, secrets: Arc<dyn SecretSource>, prices: Catalog) -> Arc<Engine> {
        let (events, _) = broadcast::channel(4096);
        Arc::new(Engine { ledger, adapters: adapters.into_iter().map(|a| (a.id().to_string(), a)).collect(), secrets, prices, events, allow_unverified_prices: false })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RunEvent> {
        self.events.subscribe()
    }

    fn adapter(&self, provider: &str) -> Result<Arc<dyn ProviderAdapter>> {
        self.adapters.get(provider).cloned().ok_or_else(|| EngineError::UnknownProvider(provider.into()))
    }

    /// Adapter lookup for hosts (connection tests, capability checks).
    pub fn adapter_for(&self, provider: &str) -> Result<Arc<dyn ProviderAdapter>> {
        self.adapter(provider)
    }

    fn emit(&self, run: RunId, seq: i64, kind: &str, payload: Value) {
        let _ = self.events.send(RunEvent { run_id: run, seq, kind: kind.into(), payload });
    }

    /// Append an event inside `c` (must be the writer connection inside a transaction) and return it for emitting after commit.
    fn record(c: &rusqlite::Connection, run: RunId, kind: &str, payload: Value) -> Result<(i64, String, Value)> {
        let seq = Ledger::append_event(c, run, kind, &payload)?;
        Ok((seq, kind.to_string(), payload))
    }

    /// Validate inputs, freeze the run, plan the answer stage. No paid call happens here.
    pub fn create_run(self: &Arc<Self>, input: RunInput) -> Result<RunId> {
        let n = input.seats.len();
        if !(bounds::MIN_SEATS..=bounds::MAX_SEATS).contains(&n) {
            return Err(EngineError::Invalid(format!("seat count {n} outside 2..8")));
        }
        if input.question.trim().is_empty() || input.question.chars().count() > bounds::MAX_QUESTION_CHARS {
            return Err(EngineError::Invalid("question empty or too long".into()));
        }
        if input.reference_text.as_ref().map(|r| r.chars().count()).unwrap_or(0) > bounds::MAX_REFERENCE_CHARS {
            return Err(EngineError::Invalid("reference text too long".into()));
        }
        for (i, s) in input.seats.iter().enumerate() {
            if s.seat_index as usize != i {
                return Err(EngineError::Invalid(format!("seat {i} has index {}", s.seat_index)));
            }
            for p in [&s.prompt_answer, &s.prompt_review, &s.prompt_improve] {
                if p.chars().count() > bounds::MAX_PROMPT_CHARS {
                    return Err(EngineError::Invalid(format!("prompt too long for seat {i}")));
                }
            }
            let adapter = self.adapter(&s.provider)?;
            let caps = adapter.capabilities(&s.model_id);
            for rs in [&s.answer, &s.review, &s.improve] {
                if rs.temperature.is_some() && !caps.supports_temperature {
                    return Err(EngineError::Invalid(format!("{} does not support temperature", s.model_id)));
                }
                if rs.reasoning_effort.is_some() && !caps.supports_reasoning_effort {
                    return Err(EngineError::Invalid(format!("{} does not support reasoning effort", s.model_id)));
                }
            }
            self.price_for(&s.provider, &s.model_id)?;
            if !input.is_sample {
                self.secrets.read(s.credential_ref).map_err(|_| EngineError::Invalid(format!("no credential for seat {i}")))?;
            }
        }
        self.adapter(&input.synthesizer.provider)?;
        self.price_for(&input.synthesizer.provider, &input.synthesizer.model_id)?;
        if input.ceiling <= MicroUsd::ZERO {
            return Err(EngineError::Invalid("spending ceiling must be positive".into()));
        }
        if input.system_prompts.all().iter().any(|p| p.chars().count() > bounds::MAX_PROMPT_CHARS) {
            return Err(EngineError::Invalid("a system prompt is too long".into()));
        }

        let run_id = RunId::new();
        let seed = input.seed.clone().unwrap_or_else(|| run_id.to_string());
        let deadline = time::OffsetDateTime::now_utc() + time::Duration::seconds(input.limits.run_deadline_secs as i64);
        let row = RunRow {
            id: run_id,
            state: RunState::Answering,
            stage: Stage::Answer,
            reason_code: None,
            reason_text: None,
            question: input.question.clone(),
            reference_text: input.reference_text.clone(),
            panel_snapshot_json: json!({ "seat_count": n, "synthesizer": input.synthesizer, "rubric_version": RUBRIC_VERSION, "instructions_version": input.system_prompts.version(), "system_prompts": input.system_prompts, "transform_version": rt_envelope::TRANSFORM_VERSION }),
            limits_json: json!({ "ceiling_usd_micro": input.ceiling, "limits": input.limits }),
            price_catalog_version: self.prices.version.clone(),
            schema_version: "papers.v1".into(),
            app_version: APP_VERSION.into(),
            adapter_versions_json: json!({ "providers": rt_adapters::ProviderCatalog::bundled().version }),
            seed,
            is_sample: input.is_sample,
            created_at: rt_ledger::now(),
            started_at: Some(rt_ledger::now()),
            deadline_at: Some(deadline.format(&time::format_description::well_known::Rfc3339).unwrap()),
            finished_at: None,
        };
        let mut pending = Vec::new();
        {
            let mut c = self.ledger.writer()?;
            let tx = c.transaction().map_err(LedgerError::from)?;
            runs::insert_run(&tx, &row)?;
            for s in &input.seats {
                let digests = json!({ "answer": rt_core::digest(s.prompt_answer.as_bytes()), "review": rt_core::digest(s.prompt_review.as_bytes()), "improve": rt_core::digest(s.prompt_improve.as_bytes()) });
                runs::insert_seat(&tx, run_id, s, &digests)?;
            }
            pending.push(Self::record(&tx, run_id, "run.state_changed", json!({ "state": "answering", "stage": "answer" }))?);
            Self::plan_stage(&tx, run_id, Stage::Answer, n as u8, &mut pending)?;
            tx.commit().map_err(LedgerError::from)?;
        }
        for (seq, kind, payload) in pending {
            self.emit(run_id, seq, &kind, payload);
        }
        Ok(run_id)
    }

    fn price_for(&self, provider: &str, model: &str) -> Result<rt_budget::PriceRecord> {
        if self.allow_unverified_prices {
            return self.prices.get(provider, model).cloned().ok_or_else(|| rt_budget::BudgetError::NoPrice { provider: provider.into(), model: model.into() }.into());
        }
        Ok(self.prices.for_dispatch(provider, model)?.clone())
    }

    /// Create the logical calls for a stage (planned) and record stage.started.
    fn plan_stage(c: &rusqlite::Connection, run: RunId, stage: Stage, n: u8, pending: &mut Vec<(i64, String, Value)>) -> Result<()> {
        let mut specs: Vec<(u8, Option<u8>, Option<u8>)> = Vec::new();
        match stage {
            Stage::Answer | Stage::Improve => specs.extend((0..n).map(|s| (s, None, None))),
            Stage::Review => specs.extend(rt_core::review_plan(n as usize).into_iter().map(|p| (p.reviewer, Some(p.target), Some(p.rotation)))),
            Stage::Synthesize => specs.push((0, None, None)),
        }
        for (reviewer, target, rotation) in &specs {
            let call = CallRow { id: LogicalCallId::new(), run_id: run, stage, reviewer_seat: *reviewer, target_seat: *target, rotation: *rotation, state: CallState::Planned, attempts_used: 0, schema_retries_used: 0, accepted_paper_id: None, envelope_id: None };
            runs::insert_call(c, &call)?;
            pending.push(Self::record(c, run, "call.queued", json!({ "call_id": call.id, "stage": stage, "reviewer": reviewer, "target": target, "rotation": rotation }))?);
        }
        pending.push(Self::record(c, run, "stage.started", json!({ "stage": stage, "planned_calls": specs.len() }))?);
        Ok(())
    }

    /// Mark every non-terminal run as interrupted (call at launch, PRD 8.2).
    pub fn mark_interrupted_at_launch(&self) -> Result<Vec<RunId>> {
        let mut c = self.ledger.writer()?;
        let tx = c.transaction().map_err(LedgerError::from)?;
        let ids = runs::non_terminal_runs(&tx)?;
        let mut pending = Vec::new();
        for id in &ids {
            let run = runs::get_run(&tx, *id)?;
            if let Ok(next) = run.state.apply(RunEventKind::Interrupt, run.stage) {
                runs::update_run_state(&tx, *id, next, run.stage, Some("interrupted"), Some("The app was closed or lost power during this run."))?;
                for (attempt, call) in runs::unfinished_attempts(&tx, *id)? {
                    runs::finish_attempt(&tx, attempt, &AttemptOutcomeRow { outcome: "uncertain".into(), finish_reason: None, usage_json: json!({}), raw_output_bounded: None, error_category: Some("interrupted".into()), error_sanitized: Some("no response was recorded before the app stopped".into()), provider_request_id: None, first_byte_ms: None, total_ms: 0 })?;
                    let row = runs::get_call(&tx, call)?;
                    runs::update_call(&tx, call, CallState::Uncertain, row.attempts_used, row.schema_retries_used, None)?;
                }
                pending.push((*id, Self::record(&tx, *id, "run.state_changed", json!({ "state": next, "stage": run.stage, "reason": "interrupted" }))?));
            }
        }
        tx.commit().map_err(LedgerError::from)?;
        for (id, (seq, kind, payload)) in pending {
            self.emit(id, seq, &kind, payload);
        }
        Ok(ids)
    }

    /// Drive a run until it completes, blocks, is cancelled or fails. Safe to call again after
    /// a block or interruption: accepted papers are reused, never regenerated.
    pub async fn execute(self: &Arc<Self>, run_id: RunId, cancel: CancellationToken) -> Result<RunState> {
        let (run, seats, synth, budget) = {
            let c = self.ledger.writer()?;
            let run = runs::get_run(&c, run_id)?;
            let seats = runs::list_seats(&c, run_id)?;
            let synth: SynthesizerSeat = serde_json::from_value(run.panel_snapshot_json["synthesizer"].clone()).map_err(|e| EngineError::Invalid(e.to_string()))?;
            let ceiling = MicroUsd(run.limits_json["ceiling_usd_micro"].as_i64().unwrap_or(0));
            let (settled, unresolved) = runs::budget_totals(&c, run_id)?;
            let mut b = RunBudget::new(ceiling);
            b.settled = settled;
            b.unresolved = unresolved;
            (run, seats, synth, b)
        };
        if run.state.is_terminal() {
            return Ok(run.state);
        }
        let limits: Limits = serde_json::from_value(run.limits_json["limits"].clone()).unwrap_or_default();
        // resume from blocked/interrupted
        let mut run = run;
        if matches!(run.state, RunState::Blocked | RunState::Interrupted) {
            let next = run.state.apply(RunEventKind::Resume, run.stage).map_err(|_| EngineError::WrongState(run_id, run.state))?;
            self.set_state(run_id, next, run.stage, None, None)?;
            // the time limit counts from the moment the user resumes, not from the original start
            let deadline = (time::OffsetDateTime::now_utc() + time::Duration::seconds(limits.run_deadline_secs as i64)).format(&time::format_description::well_known::Rfc3339).unwrap();
            {
                let c = self.ledger.writer()?;
                runs::set_run_deadline(&c, run_id, &deadline)?;
            }
            run.deadline_at = Some(deadline);
            // Resume means "try again": calls that ran out of attempts get a fresh round. Nothing accepted is resent.
            let requeued = {
                let mut c = self.ledger.writer()?;
                let tx = c.transaction().map_err(LedgerError::from)?;
                let mut evs = Vec::new();
                for call in runs::list_all_calls(&tx, run_id)?.into_iter().filter(|c| c.state == CallState::Failed) {
                    runs::update_call(&tx, call.id, CallState::Ready, call.attempts_used, call.schema_retries_used, None)?;
                    evs.push(Self::record(&tx, run_id, "call.queued", json!({ "call_id": call.id, "stage": call.stage, "reviewer": call.reviewer_seat, "target": call.target_seat, "manual_retry": true, "reason": "resume" }))?);
                }
                tx.commit().map_err(LedgerError::from)?;
                evs
            };
            for (seq, k, p) in requeued {
                self.emit(run_id, seq, &k, p);
            }
        } else if !run.state.is_active_stage() {
            return Err(EngineError::WrongState(run_id, run.state));
        }
        let references = split_references(run.reference_text.as_deref());
        let shared = Arc::new(RunShared {
            engine: self.clone(),
            run: run.clone(),
            seats,
            synth,
            references,
            system: serde_json::from_value(run.panel_snapshot_json["system_prompts"].clone()).unwrap_or_default(),
            budget: Mutex::new(budget),
            limits: limits.clone(),
            cancel: cancel.clone(),
            stop: CancellationToken::new(),
            stop_reason: Mutex::new(None),
            global: Arc::new(Semaphore::new(limits.global_in_flight.clamp(1, bounds::HARD_MAX_IN_FLIGHT))),
            per_provider: Mutex::new(HashMap::new()),
        });

        let mut stage = run.stage;
        loop {
            if cancel.is_cancelled() {
                return self.finish_cancel(run_id, stage);
            }
            // resume-time uncertain attempts block before any new dispatch
            let calls = {
                let c = self.ledger.writer()?;
                runs::list_calls(&c, run_id, stage)?
            };
            let uncertain: Vec<_> = calls.iter().filter(|c| c.state == CallState::Uncertain).map(|c| c.id).collect();
            if !uncertain.is_empty() {
                return self.block(run_id, stage, "uncertain", &format!("{} call(s) have an unknown outcome and may already be billed. Retry them explicitly or cancel.", uncertain.len()));
            }
            let mut set = JoinSet::new();
            for call in calls.iter().filter(|c| !c.state.is_terminal()) {
                let shared = shared.clone();
                let call = call.clone();
                set.spawn(async move { Self::process_call(shared, call).await });
            }
            let mut first_err: Option<EngineError> = None;
            while let Some(res) = set.join_next().await {
                match res {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        first_err.get_or_insert(e);
                    }
                    Err(e) => {
                        first_err.get_or_insert(EngineError::Invalid(format!("task panicked: {e}")));
                    }
                }
            }
            if let Some(e) = first_err {
                let _ = self.block(run_id, stage, "persistence", &format!("stopped: {e}"));
                return Err(e);
            }
            if cancel.is_cancelled() {
                return self.finish_cancel(run_id, stage);
            }
            if let Some((code, text)) = shared.stop_reason.lock().unwrap().clone() {
                return self.block(run_id, stage, &code, &text);
            }
            // barrier check
            let calls = {
                let c = self.ledger.writer()?;
                runs::list_calls(&c, run_id, stage)?
            };
            let accepted = calls.iter().filter(|c| c.state == CallState::Accepted).count();
            if accepted == calls.len() {
                match self.barrier(run_id, stage, shared.seats.len() as u8)? {
                    Some(next) => stage = next,
                    None => return Ok(RunState::Completed),
                }
                continue;
            }
            let refused = calls.iter().filter(|c| c.state == CallState::Refused).count();
            let failed = calls.iter().filter(|c| c.state == CallState::Failed).count();
            let uncertain = calls.iter().filter(|c| c.state == CallState::Uncertain).count();
            let (code, text) = if uncertain > 0 {
                ("uncertain", format!("{uncertain} call(s) have an unknown outcome and may already be billed."))
            } else if refused > 0 {
                ("refused", format!("{refused} call(s) were refused by the provider. Strict mode needs every seat."))
            } else if failed > 0 {
                ("exhausted", format!("{failed} call(s) failed after the attempt limit."))
            } else {
                ("budget_or_deadline", "Dispatch stopped by the spending ceiling or deadline.".to_string())
            };
            return self.block(run_id, stage, code, &text);
        }
    }

    fn set_state(&self, run_id: RunId, state: RunState, stage: Stage, code: Option<&str>, text: Option<&str>) -> Result<()> {
        let pending = {
            let mut c = self.ledger.writer()?;
            let tx = c.transaction().map_err(LedgerError::from)?;
            runs::update_run_state(&tx, run_id, state, stage, code, text)?;
            let p = Self::record(&tx, run_id, "run.state_changed", json!({ "state": state, "stage": stage, "reason_code": code, "reason_text": text }))?;
            tx.commit().map_err(LedgerError::from)?;
            p
        };
        self.emit(run_id, pending.0, &pending.1, pending.2);
        Ok(())
    }

    fn block(&self, run_id: RunId, stage: Stage, code: &str, text: &str) -> Result<RunState> {
        self.set_state(run_id, RunState::Blocked, stage, Some(code), Some(text))?;
        Ok(RunState::Blocked)
    }

    fn finish_cancel(&self, run_id: RunId, stage: Stage) -> Result<RunState> {
        self.set_state(run_id, RunState::Cancelling, stage, Some("cancelled"), Some("Cancelled by the user."))?;
        self.set_state(run_id, RunState::Cancelled, stage, Some("cancelled"), Some("Cancelled by the user. Committed papers are kept."))?;
        Ok(RunState::Cancelled)
    }

    /// tx4: all calls of `stage` accepted. Plan the next stage or complete the run.
    fn barrier(&self, run_id: RunId, stage: Stage, n: u8) -> Result<Option<Stage>> {
        let mut pending = Vec::new();
        let next = {
            let mut c = self.ledger.writer()?;
            let tx = c.transaction().map_err(LedgerError::from)?;
            let calls = runs::list_calls(&tx, run_id, stage)?;
            if calls.iter().any(|c| c.state != CallState::Accepted) {
                return Err(EngineError::Invalid("barrier called with unaccepted calls".into()));
            }
            pending.push(Self::record(&tx, run_id, "stage.completed", json!({ "stage": stage, "accepted": calls.len() }))?);
            let next = stage.next();
            match next {
                Some(ns) => {
                    runs::update_run_state(&tx, run_id, RunState::for_stage(ns), ns, None, None)?;
                    pending.push(Self::record(&tx, run_id, "run.state_changed", json!({ "state": RunState::for_stage(ns), "stage": ns }))?);
                    Self::plan_stage(&tx, run_id, ns, n, &mut pending)?;
                }
                None => {
                    runs::update_run_state(&tx, run_id, RunState::Completed, stage, None, None)?;
                    let final_paper = runs::list_papers(&tx, run_id)?.into_iter().find(|p| p.kind == "final").map(|p| p.id);
                    pending.push(Self::record(&tx, run_id, "run.completed", json!({ "final_paper_id": final_paper }))?);
                }
            }
            tx.commit().map_err(LedgerError::from)?;
            next
        };
        for (seq, kind, payload) in pending {
            self.emit(run_id, seq, &kind, payload);
        }
        Ok(next)
    }

    fn call_spec(call: &CallRow) -> CallSpec {
        match call.stage {
            Stage::Answer => CallSpec::Answer { seat: call.reviewer_seat },
            Stage::Review => CallSpec::Review { reviewer: call.reviewer_seat, target: call.target_seat.unwrap_or(0) },
            Stage::Improve => CallSpec::Improve { seat: call.reviewer_seat },
            Stage::Synthesize => CallSpec::Synthesize,
        }
    }

    fn load_papers(c: &rusqlite::Connection, run_id: RunId) -> Result<Papers> {
        let mut out = Papers::default();
        for p in runs::list_papers(c, run_id)? {
            let kind = match p.kind.as_str() { "answer" => PaperKind::Answer, "review" => PaperKind::Review, "improved" => PaperKind::Improved, _ => PaperKind::Final };
            let paper = Paper { id: p.id, run_id: p.run_id, kind, author_seat: p.author_seat, target_seat: p.target_seat, body: p.body };
            match kind {
                PaperKind::Answer => { out.answers.insert(p.author_seat, paper); }
                PaperKind::Review => { out.reviews.insert((p.author_seat, p.target_seat.unwrap_or(0)), paper); }
                PaperKind::Improved => { out.improved.insert(p.author_seat, paper); }
                PaperKind::Final => {}
            }
        }
        Ok(out)
    }

    async fn process_call(shared: Arc<RunShared>, call: CallRow) -> Result<()> {
        let engine = shared.engine.clone();
        let run_id = shared.run.id;
        let spec = Self::call_spec(&call);
        let (provider, credential_ref, model, settings, prompt_seat): (String, CredentialRef, String, RoleSettings, Option<&FrozenSeat>) = match spec {
            CallSpec::Synthesize => (shared.synth.provider.clone(), shared.synth.credential_ref, shared.synth.model_id.clone(), shared.synth.settings.clone(), None),
            _ => {
                let s = &shared.seats[call.reviewer_seat as usize];
                (s.provider.clone(), s.credential_ref, s.model_id.clone(), s.settings_for(spec.role()).clone(), Some(s))
            }
        };
        let _ = prompt_seat;
        let adapter = engine.adapter(&provider)?;
        let price = engine.price_for(&provider, &model)?;
        let kind = match call.stage { Stage::Answer => rt_schemas::PaperKind::Answer, Stage::Review => rt_schemas::PaperKind::Review, Stage::Improve => rt_schemas::PaperKind::Improved, Stage::Synthesize => rt_schemas::PaperKind::Final };
        let schema: Value = rt_schemas::schema_for_prompt(kind);
        let mut params = CallParams { model: model.clone(), settings: settings.clone(), schema, schema_name: kind.schema_id().replace('.', "_"), json_schema: price.structured_output, truncation_retries: 0 };
        // a call that was cut off before (in this dispatch or an earlier one) gets more room straight away
        params.truncation_retries = {
            let c = engine.ledger.writer()?;
            runs::count_attempts_with_outcome(&c, call.id, "truncated")?.min(3) as u8
        };

        // tx1: envelope (built from committed papers only), call ready
        let envelope: Envelope = {
            let mut c = engine.ledger.writer()?;
            let papers = Self::load_papers(&c, run_id)?;
            let ctx = RunContext { run_id, seed: &shared.run.seed, question: &shared.run.question, references: &shared.references, seats: &shared.seats, synthesizer_prompt: &shared.synth.prompt, system: &shared.system };
            let env = rt_envelope::build(&ctx, spec, &papers)?;
            match runs::get_envelope_digest(&c, call.id)? {
                Some(existing) => {
                    if existing != env.digest {
                        return Err(EngineError::DigestMismatch(call.id));
                    }
                }
                None => {
                    let tx = c.transaction().map_err(LedgerError::from)?;
                    for v in &env.views {
                        runs::insert_view(&tx, v.id, v.paper_id, v.transform_version, &v.text, &v.digest, v.possibly_identifiable)?;
                    }
                    runs::insert_envelope(&tx, EnvelopeId::new(), run_id, call.id, call.stage, &env.instructions, &json!(env.messages), &env.schema_id, &env.allowed_view_ids, &json!({ "aliases": env.alias_map, "order": env.presentation_order }), &env.digest)?;
                    runs::update_call(&tx, call.id, CallState::Ready, call.attempts_used, call.schema_retries_used, None)?;
                    tx.commit().map_err(LedgerError::from)?;
                }
            }
            env
        };
        let mut wire: WireRequest = adapter.build_request(&envelope, &params)?;
        let input_tokens = rt_budget::estimate_tokens(&serde_json::to_string(&wire.body).unwrap_or_default());
        let mut state = if call.state == CallState::Planned { CallState::Ready } else { call.state };
        let mut attempts_used = call.attempts_used;
        let mut schema_retries = call.schema_retries_used;
        // Ready with attempts behind it means the user sent the call back (Resume, Retry): a fresh round.
        let requeued = call.state == CallState::Ready && call.attempts_used > 0;
        let round_base = if requeued { call.attempts_used } else { 0 };
        let schema_base = if requeued { call.schema_retries_used } else { 0 };

        while state.can_auto_dispatch() && attempts_used < round_base + bounds::MAX_ATTEMPTS_PER_CALL {
            if shared.cancel.is_cancelled() || shared.stop.is_cancelled() {
                return Ok(());
            }
            if let Some(dl) = &shared.run.deadline_at {
                if let Ok(t) = time::OffsetDateTime::parse(dl, &time::format_description::well_known::Rfc3339) {
                    if time::OffsetDateTime::now_utc() > t {
                        shared.stop_reason.lock().unwrap().get_or_insert(("deadline".into(), "The run deadline passed before every call finished.".into()));
                        shared.stop.cancel();
                        return Ok(());
                    }
                }
            }
            // concurrency limits
            let _g = shared.global.clone().acquire_owned().await.map_err(|_| EngineError::Invalid("semaphore closed".into()))?;
            let sem = { let mut m = shared.per_provider.lock().unwrap(); m.entry(provider.clone()).or_insert_with(|| Arc::new(Semaphore::new(shared.limits.per_provider_in_flight.max(1)))).clone() };
            let _p = sem.acquire_owned().await.map_err(|_| EngineError::Invalid("semaphore closed".into()))?;

            // tx2: reservation + attempt intent
            let reasoning_bound = if settings.reasoning_effort.is_some() || params.truncation_retries > 0 { settings.max_output_tokens as u64 * (1 + params.truncation_retries as u64) } else { 0 };
            let mut max_cost = rt_budget::max_attempt_cost(&price, input_tokens, settings.max_output_tokens as u64, reasoning_bound);
            if settings.web_search {
                max_cost = max_cost.saturating_add(rt_budget::WEB_SEARCH_FEE);
            }
            let reservation = { shared.budget.lock().unwrap().try_reserve(max_cost) };
            let reservation = match reservation {
                Ok(r) => r,
                Err(e) => {
                    shared.stop_reason.lock().unwrap().get_or_insert(("budget".into(), e.to_string()));
                    shared.stop.cancel(); // stop siblings from dispatching further
                    return Ok(());
                }
            };
            attempts_used += 1;
            let attempt_kind = if attempts_used == 1 { "primary" } else if state == CallState::Invalid { "schema_retry" } else { "transport_retry" };
            if attempt_kind == "schema_retry" {
                schema_retries += 1;
            }
            let attempt_id = AttemptId::new();
            {
                let mut c = engine.ledger.writer()?;
                let tx = c.transaction().map_err(LedgerError::from)?;
                runs::insert_attempt(&tx, &AttemptRow { id: attempt_id, logical_call_id: call.id, ordinal: attempts_used, kind: attempt_kind.into(), request_digest: wire.digest(), serialized_body_redacted: wire.body.clone() })?;
                runs::insert_budget_entry(&tx, BudgetEntryId::new(), run_id, Some(call.id), Some(attempt_id), "reserve", max_cost, &price.key())?;
                runs::update_call(&tx, call.id, CallState::InFlight, attempts_used, schema_retries, None)?;
                let ev = Self::record(&tx, run_id, "call.started", json!({ "call_id": call.id, "stage": call.stage, "reviewer": call.reviewer_seat, "target": call.target_seat, "attempt": attempts_used, "kind": attempt_kind }))?;
                tx.commit().map_err(LedgerError::from)?;
                engine.emit(run_id, ev.0, &ev.1, ev.2);
            }

            // the request: the secret exists only inside this scope
            let result: AttemptResult = {
                let secret = engine.secrets.read(credential_ref)?;
                adapter.dispatch(&wire, &secret, Duration::from_secs(shared.limits.attempt_deadline_secs), shared.cancel.child_token()).await
            };
            drop(_p);
            drop(_g);

            // tx3: outcome, validation, paper, settlement
            let mut outcome = result.outcome.clone();
            let mut validated: Option<Value> = None;
            let mut validation_error: Option<String> = None;
            if outcome == AttemptOutcome::Accepted {
                match rt_schemas::validate(kind, result.raw_output.as_deref().unwrap_or(""), &envelope.validation) {
                    Ok(v) => validated = Some(v),
                    Err(e) => {
                        outcome = AttemptOutcome::InvalidOutput;
                        validation_error = Some(e.to_string());
                    }
                }
            }
            let next_state = next_call_state(&outcome, attempts_used - round_base, schema_retries - schema_base);
            // a cut-off answer is retried with more room for hidden reasoning, not with the identical request
            if outcome == AttemptOutcome::Truncated && next_state.can_auto_dispatch() {
                params.truncation_retries = params.truncation_retries.saturating_add(1);
                wire = adapter.build_request(&envelope, &params)?;
            }
            // the search fee is charged whenever the provider answered; a request that never reached a model has no usage
            // OpenRouter reports its real charge (search included); otherwise tokens × price, plus the search fee
            let actual_cost = rt_budget::settle_cost(&price, &result.usage).map(|c| if settings.web_search && result.usage.provider_cost_micro.is_none() { c.saturating_add(rt_budget::WEB_SEARCH_FEE) } else { c });
            let usage_json = { let mut u = json!(result.usage); if !result.web_sources.is_empty() { u["web_sources"] = json!(result.web_sources); } u };
            let pending_events = {
                let mut c = engine.ledger.writer()?;
                let tx = c.transaction().map_err(LedgerError::from)?;
                // a late response after cancel/delete must not advance the run
                let current = runs::get_run(&tx, run_id)?;
                if current.state.is_terminal() || current.state == RunState::Cancelling {
                    runs::finish_attempt(&tx, attempt_id, &AttemptOutcomeRow { outcome: "late".into(), finish_reason: result.finish_reason.clone(), usage_json: usage_json.clone(), raw_output_bounded: result.raw_output.clone(), error_category: Some("late_response".into()), error_sanitized: Some("response arrived after the run stopped".into()), provider_request_id: result.provider_request_id.clone(), first_byte_ms: result.first_byte_ms, total_ms: result.total_ms })?;
                    tx.commit().map_err(LedgerError::from)?;
                    return Ok(());
                }
                runs::finish_attempt(&tx, attempt_id, &AttemptOutcomeRow { outcome: serde_json::to_value(&outcome).unwrap()["kind"].as_str().unwrap_or("unknown").to_string(), finish_reason: result.finish_reason.clone(), usage_json: usage_json.clone(), raw_output_bounded: result.raw_output.clone(), error_category: result.error_category.as_ref().map(|c| format!("{c:?}").to_lowercase()).or(validation_error.as_ref().map(|_| "validation".into())), error_sanitized: result.error_sanitized.clone().or(validation_error.clone()), provider_request_id: result.provider_request_id.clone(), first_byte_ms: result.first_byte_ms, total_ms: result.total_ms })?;
                if let Some(m) = &result.returned_model {
                    if let Some(seat) = spec.seat() {
                        runs::set_returned_model(&tx, run_id, seat, m)?;
                    }
                }
                let mut evs = Vec::new();
                // budget movement
                {
                    let mut b = shared.budget.lock().unwrap();
                    match (&outcome, actual_cost) {
                        (AttemptOutcome::Uncertain, _) => {
                            b.mark_unresolved(reservation).ok();
                            runs::insert_budget_entry(&tx, BudgetEntryId::new(), run_id, Some(call.id), Some(attempt_id), "unresolved", max_cost, &price.key())?;
                        }
                        (AttemptOutcome::TransportBeforeSend | AttemptOutcome::NonRetryable | AttemptOutcome::RateLimited { .. } | AttemptOutcome::TransientServer, _) if result.usage == Usage::default() => {
                            b.release(reservation).ok();
                            runs::insert_budget_entry(&tx, BudgetEntryId::new(), run_id, Some(call.id), Some(attempt_id), "release", max_cost, &price.key())?;
                        }
                        (_, Some(cost)) => {
                            b.settle(reservation, cost).ok();
                            runs::insert_budget_entry(&tx, BudgetEntryId::new(), run_id, Some(call.id), Some(attempt_id), "settle", cost, &price.key())?;
                        }
                        (_, None) => {
                            b.mark_unresolved(reservation).ok();
                            runs::insert_budget_entry(&tx, BudgetEntryId::new(), run_id, Some(call.id), Some(attempt_id), "unresolved", max_cost, &price.key())?;
                        }
                    }
                    let snap = b.snapshot();
                    evs.push(Self::record(&tx, run_id, "budget.updated", json!(snap))?);
                }
                let mut accepted_paper = None;
                if let Some(body) = &validated {
                    let paper_id = PaperId::new();
                    let target_paper_id = match spec {
                        CallSpec::Review { target, .. } => Self::load_papers(&tx, run_id)?.answers.get(&target).map(|p| p.id),
                        _ => None,
                    };
                    runs::insert_paper(&tx, &PaperRow { id: paper_id, run_id, kind: kind.schema_id().split('.').next().unwrap_or("answer").to_string(), author_seat: call.reviewer_seat, target_paper_id, target_seat: call.target_seat, schema_version: kind.schema_id().into(), body: body.clone(), digest: rt_core::digest(rt_core::canonical_json(body).as_bytes()), attempt_id })?;
                    accepted_paper = Some(paper_id);
                    evs.push(Self::record(&tx, run_id, "call.accepted", json!({ "call_id": call.id, "stage": call.stage, "reviewer": call.reviewer_seat, "target": call.target_seat, "paper_id": paper_id, "kind": kind.schema_id() }))?);
                } else {
                    let kind_name = match next_state { CallState::Uncertain => "call.uncertain", CallState::Refused => "call.refused", CallState::Failed => "call.failed", CallState::RetryWait => "call.retry_wait", _ => "call.invalid" };
                    evs.push(Self::record(&tx, run_id, kind_name, json!({ "call_id": call.id, "stage": call.stage, "reviewer": call.reviewer_seat, "target": call.target_seat, "attempt": attempts_used, "category": result.error_category, "message": result.error_sanitized.clone().or(validation_error.clone()) }))?);
                }
                runs::update_call(&tx, call.id, next_state, attempts_used, schema_retries, accepted_paper)?;
                tx.commit().map_err(LedgerError::from)?;
                evs
            };
            for (seq, k, p) in pending_events {
                engine.emit(run_id, seq, &k, p);
            }
            state = next_state;
            if state == CallState::RetryWait {
                let wait = match &outcome {
                    AttemptOutcome::RateLimited { retry_after_ms: Some(ms) } => Duration::from_millis((*ms).min(60_000)),
                    AttemptOutcome::RateLimited { retry_after_ms: None } => Duration::from_millis(6_000 * attempts_used as u64 + rand::random::<u64>() % 1_500),
                    _ => {
                        let base = 1000u64 * (1u64 << (attempts_used as u32 - 1).min(4));
                        let jitter = rand::random::<u64>() % 400;
                        Duration::from_millis(base + jitter)
                    }
                };
                tokio::select! {
                    _ = shared.cancel.cancelled() => return Ok(()),
                    _ = shared.stop.cancelled() => return Ok(()),
                    _ = tokio::time::sleep(wait) => {}
                }
            }
        }
        if state == CallState::RetryWait || state == CallState::Invalid {
            // attempts exhausted while still retryable
            let mut c = engine.ledger.writer()?;
            let tx = c.transaction().map_err(LedgerError::from)?;
            runs::update_call(&tx, call.id, CallState::Failed, attempts_used, schema_retries, None)?;
            let ev = Self::record(&tx, run_id, "call.failed", json!({ "call_id": call.id, "stage": call.stage, "reviewer": call.reviewer_seat, "target": call.target_seat, "attempt": attempts_used, "message": "attempt limit reached" }))?;
            tx.commit().map_err(LedgerError::from)?;
            engine.emit(run_id, ev.0, &ev.1, ev.2);
        }
        Ok(())
    }

    /// The user explicitly retries an uncertain call after seeing the billing warning (RUN-02).
    pub fn retry_uncertain(&self, call_id: LogicalCallId) -> Result<()> {
        let mut c = self.ledger.writer()?;
        let tx = c.transaction().map_err(LedgerError::from)?;
        let call = runs::get_call(&tx, call_id)?;
        if call.state != CallState::Uncertain {
            return Err(EngineError::Invalid("call is not uncertain".into()));
        }
        runs::update_call(&tx, call_id, CallState::Ready, call.attempts_used, call.schema_retries_used, None)?;
        let ev = Self::record(&tx, call.run_id, "call.queued", json!({ "call_id": call_id, "stage": call.stage, "reviewer": call.reviewer_seat, "target": call.target_seat, "manual_retry": true }))?;
        tx.commit().map_err(LedgerError::from)?;
        self.emit(call.run_id, ev.0, &ev.1, ev.2);
        Ok(())
    }

    /// Delete a run and everything under it (DATA-01). The caller must have cancelled execution first.
    pub fn delete_run(&self, run_id: RunId) -> Result<bool> {
        Ok(self.ledger.maintenance(|tx| runs::delete_run(tx, run_id))?)
    }
}

/// Split reference text into (Ref-n, text) blocks. Blank-line separated paragraphs become references.
pub fn split_references(text: Option<&str>) -> Vec<(String, String)> {
    let Some(t) = text else { return vec![] };
    t.split("\n\n").map(str::trim).filter(|p| !p.is_empty()).enumerate().map(|(i, p)| (format!("Ref-{}", i + 1), p.to_string())).collect()
}

#[cfg(test)]
mod tests;
