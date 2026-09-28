//! Command handlers. Every handler validates its arguments natively (SEC-10) and maps
//! errors to short sanitized strings.

use crate::types::*;
use crate::AppState;
use rt_core::{bounds, CredentialRef, FrozenSeat, LogicalCallId, MicroUsd, PaperId, PanelId, RoleSettings, RunId, RunState, SeatLimits, Stage};
use rt_engine::{Limits, RunInput, SynthesizerSeat};
use rt_envelope::instructions::SystemPrompts;
use rt_ledger::rusqlite::{params, OptionalExtension};
use rt_ledger::runs;
use rt_secrets::Secret;
use serde_json::{json, Value};
use std::str::FromStr;
use tauri::State;
use tokio_util::sync::CancellationToken;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    rt_adapters::sanitize(e.to_string())
}

fn parse_ref(s: &str) -> CmdResult<CredentialRef> {
    CredentialRef::from_str(s).map_err(|_| "invalid credential reference".to_string())
}

fn parse_run(s: &str) -> CmdResult<RunId> {
    RunId::from_str(s).map_err(|_| "invalid run id".to_string())
}

// ---------------------------------------------------------------- credentials

#[tauri::command]
pub fn list_credentials(state: State<'_, AppState>) -> CmdResult<Vec<CredentialMeta>> {
    let c = state.engine.ledger.writer().map_err(err)?;
    let mut stmt = c.prepare("SELECT ref, provider, label, created_at, last_tested_at, last_test_result FROM credentials_meta ORDER BY created_at").map_err(err)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?)))
        .map_err(err)?;
    let mut out = Vec::new();
    for row in rows {
        let (r, provider, label, created_at, last_tested_at, last_test_result) = row.map_err(err)?;
        let fingerprint = parse_ref(&r).ok().and_then(|cr| rt_secrets::read(cr).ok()).map(|s| {
            let k = s.expose();
            let n = k.chars().count();
            if n <= 16 { format!("{}…", k.chars().take(4).collect::<String>()) } else { format!("{}…{} ({} chars)", k.chars().take(12).collect::<String>(), k.chars().skip(n - 3).collect::<String>(), n) }
        });
        let present = fingerprint.is_some();
        out.push(CredentialMeta { r#ref: r, provider, label, created_at, last_tested_at, last_test_result, present, fingerprint });
    }
    Ok(out)
}

#[tauri::command]
pub fn save_credential(state: State<'_, AppState>, provider: String, label: String, secret: String) -> CmdResult<String> {
    if state.providers.get(&provider).is_none() {
        return Err("unknown provider".into());
    }
    if label.trim().is_empty() || label.len() > 80 {
        return Err("label must be 1 to 80 characters".into());
    }
    let trimmed = secret.trim().to_string();
    if trimmed.chars().any(char::is_whitespace) {
        return Err("The key contains spaces or line breaks. Copy it again and paste it once.".into());
    }
    if trimmed.matches("sk-").count() > 1 {
        return Err(format!("This looks like a key pasted twice ({} characters). Clear the field and paste it once.", trimmed.chars().count()));
    }
    if trimmed.chars().count() > 120 {
        return Err(format!("A key is normally under 100 characters; this one is {}. Check it and paste it once.", trimmed.chars().count()));
    }
    let secret = Secret::new(trimmed).map_err(err)?;
    let r = CredentialRef::new();
    rt_secrets::save(r, secret).map_err(err)?;
    let c = state.engine.ledger.writer().map_err(err)?;
    c.execute("INSERT INTO credentials_meta(ref, provider, label, created_at) VALUES(?1, ?2, ?3, ?4)", params![r.to_string(), provider, label.trim(), rt_ledger::now()]).map_err(err)?;
    Ok(r.to_string())
}

#[tauri::command]
pub fn delete_credential(state: State<'_, AppState>, r#ref: String) -> CmdResult<()> {
    let r = parse_ref(&r#ref)?;
    // stop any run that uses this credential before removing it (KEY-05)
    {
        let running = state.running.lock().map_err(|_| "lock")?;
        let c = state.engine.ledger.writer().map_err(err)?;
        for (run_id, token) in running.iter() {
            let uses: i64 = c.query_row("SELECT COUNT(*) FROM seats WHERE run_id = ?1 AND credential_ref = ?2", params![run_id.to_string(), r.to_string()], |row| row.get(0)).map_err(err)?;
            if uses > 0 {
                token.cancel();
            }
        }
    }
    rt_secrets::delete(r).map_err(err)?;
    let c = state.engine.ledger.writer().map_err(err)?;
    c.execute("DELETE FROM credentials_meta WHERE ref = ?1", params![r.to_string()]).map_err(err)?;
    Ok(())
}

/// A tiny real call (may be charged, KEY-04). Uses the answer schema with a short prompt.
#[tauri::command]
pub async fn test_credential(state: State<'_, AppState>, r#ref: String, provider: String, model_id: String) -> CmdResult<TestResult> {
    let r = parse_ref(&r#ref)?;
    let engine = state.engine.clone();
    let adapter = state.providers.get(&provider).ok_or("unknown provider")?.clone();
    let _ = adapter;
    let secret = rt_secrets::read(r).map_err(err)?;
    let started = std::time::Instant::now();
    let seat = FrozenSeat {
        seat_index: 0,
        label: "Test".into(),
        provider: provider.clone(),
        credential_ref: r,
        model_id: model_id.clone(),
        model_is_alias: false,
        answer: RoleSettings { temperature: None, max_output_tokens: 300, reasoning_effort: None, web_search: false },
        review: RoleSettings { temperature: None, max_output_tokens: 300, reasoning_effort: None, web_search: false },
        improve: RoleSettings { temperature: None, max_output_tokens: 300, reasoning_effort: None, web_search: false },
        prompt_answer: "This is a connection test. Answer in one short sentence with one claim.".into(),
        prompt_review: String::new(),
        prompt_improve: String::new(),
        limits: SeatLimits { cost_cap: MicroUsd(100_000) },
    };
    let seats = vec![seat.clone()];
    let ctx = rt_envelope::RunContext { run_id: RunId::new(), seed: "test", question: "Reply with the single word OK as your answer.", references: &[], seats: &seats, synthesizer_prompt: "", system: &SystemPrompts::default() };
    let env = rt_envelope::build(&ctx, rt_envelope::CallSpec::Answer { seat: 0 }, &rt_envelope::Papers::default()).map_err(err)?;
    let schema: Value = rt_schemas::schema_for_prompt(rt_schemas::PaperKind::Answer);
    let json_schema = engine.prices.get(&provider, &model_id).map(|r| r.structured_output).unwrap_or(true);
    let params = rt_adapters::CallParams { model: model_id, settings: seat.answer.clone(), schema, schema_name: "answer_v1".into(), json_schema, truncation_retries: 0 };
    let adapters = engine_adapter(&engine, &provider)?;
    let wire = adapters.build_request(&env, &params).map_err(err)?;
    let result = adapters.dispatch(&wire, &secret, std::time::Duration::from_secs(45), CancellationToken::new()).await;
    let ok = matches!(result.outcome, rt_core::AttemptOutcome::Accepted);
    let message = if ok { "Connected. The provider answered a structured test call.".to_string() } else { result.error_sanitized.clone().unwrap_or_else(|| format!("{:?}", result.outcome)) };
    let c = state.engine.ledger.writer().map_err(err)?;
    c.execute("UPDATE credentials_meta SET last_tested_at = ?2, last_test_result = ?3 WHERE ref = ?1", params![r.to_string(), rt_ledger::now(), if ok { "ok" } else { "failed" }]).map_err(err)?;
    Ok(TestResult { ok, category: result.error_category.map(|c| format!("{c:?}").to_lowercase()), message, returned_model: result.returned_model, latency_ms: started.elapsed().as_millis() as u64 })
}

fn engine_adapter(engine: &rt_engine::Engine, provider: &str) -> CmdResult<std::sync::Arc<dyn rt_adapters::ProviderAdapter>> {
    engine.adapter_for(provider).map_err(err)
}

// ---------------------------------------------------------------- providers, models, panels

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    Ok(state.providers.ids().into_iter().map(String::from).collect())
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>, provider: String) -> CmdResult<Vec<ModelInfo>> {
    let adapter = engine_adapter(&state.engine, &provider)?;
    let mut out: Vec<ModelInfo> = state
        .engine
        .prices
        .records()
        .filter(|r| r.provider == provider)
        .map(|r| {
            let caps = adapter.capabilities(&r.model);
            ModelInfo { provider: r.provider.clone(), model_id: r.model.clone(), price_verified: r.verified, supports_temperature: caps.supports_temperature, supports_reasoning_effort: caps.supports_reasoning_effort }
        })
        .collect();
    out.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    Ok(out)
}

fn validate_panel(doc: &PanelDoc) -> CmdResult<()> {
    if doc.name.trim().is_empty() || doc.name.len() > 80 {
        return Err("panel name must be 1 to 80 characters".into());
    }
    if !(bounds::MIN_SEATS..=bounds::MAX_SEATS).contains(&doc.seats.len()) {
        return Err(format!("a panel needs {} to {} seats", bounds::MIN_SEATS, bounds::MAX_SEATS));
    }
    for s in &doc.seats {
        let name = s.label.trim().chars().count();
        if name == 0 || name > 40 {
            return Err("every agent needs a name of 1 to 40 characters".into());
        }
        for p in [&s.prompt_answer, &s.prompt_review, &s.prompt_improve] {
            if p.chars().count() > bounds::MAX_PROMPT_CHARS {
                return Err("a role prompt is too long".into());
            }
        }
        for t in [&s.answer, &s.review, &s.improve] {
            if let Some(temp) = t.temperature {
                if !(0.0..=2.0).contains(&temp) {
                    return Err("temperature must be between 0 and 2".into());
                }
            }
            if !(50..=8000).contains(&t.max_output_words) {
                return Err("max output words must be between 50 and 8000".into());
            }
        }
        MicroUsd::parse_dollars(&s.cost_cap_usd).ok_or("cost cap must be a dollar amount like 0.40")?;
        parse_ref(&s.credential_ref)?;
    }
    parse_ref(&doc.synthesizer.credential_ref)?;
    Ok(())
}

#[tauri::command]
pub fn list_panels(state: State<'_, AppState>) -> CmdResult<Vec<PanelSummary>> {
    let panels = state.engine.ledger.list_panels().map_err(err)?;
    Ok(panels
        .into_iter()
        .filter_map(|(id, name, json)| serde_json::from_value::<PanelDoc>(json).ok().map(|doc| PanelSummary { id: id.to_string(), name, seat_count: doc.seats.len(), doc }))
        .collect())
}

#[tauri::command]
pub fn save_panel(state: State<'_, AppState>, doc: PanelDoc) -> CmdResult<String> {
    validate_panel(&doc)?;
    let id = match &doc.id {
        Some(s) => PanelId::from_str(s).map_err(|_| "invalid panel id")?,
        None => PanelId::new(),
    };
    let mut stored = doc.clone();
    stored.id = Some(id.to_string());
    state.engine.ledger.upsert_panel(id, doc.name.trim(), &serde_json::to_value(&stored).map_err(err)?, rt_engine::RUBRIC_VERSION).map_err(err)?;
    Ok(id.to_string())
}

#[tauri::command]
pub fn delete_panel(state: State<'_, AppState>, id: String) -> CmdResult<bool> {
    let id = PanelId::from_str(&id).map_err(|_| "invalid panel id")?;
    state.engine.ledger.delete_panel(id).map_err(err)
}

// ---------------------------------------------------------------- runs

fn to_role(t: &RoleTuning, web_search: bool) -> RoleSettings {
    RoleSettings { temperature: t.temperature, max_output_tokens: FrozenSeat::words_to_tokens(t.max_output_words), reasoning_effort: t.reasoning_effort.clone(), web_search }
}

const SYSTEM_PROMPTS_KEY: &str = "system_prompts";

/// The user's shared system prompts, or the bundled defaults when none were saved.
fn load_system_prompts(state: &AppState) -> CmdResult<SystemPrompts> {
    Ok(state.engine.ledger.get_setting(SYSTEM_PROMPTS_KEY).map_err(err)?.and_then(|v| serde_json::from_value::<SystemPrompts>(v).ok()).unwrap_or_default())
}

fn validate_system_prompts(p: &SystemPrompts) -> CmdResult<()> {
    if p.all().iter().any(|t| t.chars().count() > bounds::MAX_PROMPT_CHARS) {
        return Err(format!("each system prompt must be under {} characters", bounds::MAX_PROMPT_CHARS));
    }
    Ok(())
}

#[tauri::command]
pub fn get_system_prompts(state: State<'_, AppState>) -> CmdResult<Value> {
    let current = load_system_prompts(&state)?;
    Ok(json!({ "current": current, "defaults": SystemPrompts::default(), "isDefault": current.is_default(), "version": current.version() }))
}

/// Save the shared system prompts. `None` restores every default.
#[tauri::command]
pub fn save_system_prompts(state: State<'_, AppState>, prompts: Option<SystemPrompts>) -> CmdResult<()> {
    match prompts {
        Some(p) => {
            validate_system_prompts(&p)?;
            state.engine.ledger.set_setting(SYSTEM_PROMPTS_KEY, &serde_json::to_value(&p).map_err(err)?).map_err(err)
        }
        None => state.engine.ledger.set_setting(SYSTEM_PROMPTS_KEY, &Value::Null).map_err(err),
    }
}

fn to_run_input(doc: &PanelDoc, question: String, reference_text: Option<String>, ceiling: MicroUsd, system_prompts: SystemPrompts) -> CmdResult<RunInput> {
    let seats = doc
        .seats
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Ok(FrozenSeat {
                seat_index: i as u8,
                label: s.label.clone(),
                provider: s.provider.clone(),
                credential_ref: parse_ref(&s.credential_ref)?,
                model_id: s.model_id.clone(),
                model_is_alias: s.model_is_alias,
                answer: to_role(&s.answer, doc.web_search.answer),
                review: to_role(&s.review, doc.web_search.review),
                improve: to_role(&s.improve, doc.web_search.improve),
                prompt_answer: s.prompt_answer.clone(),
                prompt_review: s.prompt_review.clone(),
                prompt_improve: s.prompt_improve.clone(),
                limits: SeatLimits { cost_cap: MicroUsd::parse_dollars(&s.cost_cap_usd).unwrap_or(MicroUsd(400_000)) },
            })
        })
        .collect::<CmdResult<Vec<_>>>()?;
    Ok(RunInput {
        system_prompts,
        question,
        reference_text,
        seats,
        synthesizer: SynthesizerSeat { provider: doc.synthesizer.provider.clone(), credential_ref: parse_ref(&doc.synthesizer.credential_ref)?, model_id: doc.synthesizer.model_id.clone(), model_is_alias: doc.synthesizer.model_is_alias, settings: to_role(&doc.synthesizer.tuning, doc.web_search.synthesize), prompt: doc.synthesizer.prompt.clone() },
        ceiling,
        seed: None,
        is_sample: false,
        // Real runs through gateways: thinking models are slow and the gateway tolerates parallelism.
        // Searching adds a retrieval step and a larger prompt, so those runs get more time per attempt and overall.
        // Thinking models through a gateway can take several minutes for one long reply (GLM: ~28 tokens/s,
        // 4-7k tokens). A call gets 7 minutes (8 with search); the run 40 (50), restarted on every resume.
        limits: Limits { global_in_flight: 8, per_provider_in_flight: 4, attempt_deadline_secs: if doc.web_search.any() { 480 } else { 420 }, run_deadline_secs: if doc.web_search.any() { 3000 } else { 2400 } },
    })
}

fn load_panel(state: &AppState, id: &str) -> CmdResult<PanelDoc> {
    let pid = PanelId::from_str(id).map_err(|_| "invalid panel id")?;
    let panels = state.engine.ledger.list_panels().map_err(err)?;
    let (_, _, json) = panels.into_iter().find(|(i, _, _)| *i == pid).ok_or("panel not found")?;
    serde_json::from_value(json).map_err(err)
}

#[tauri::command]
pub fn preflight_run(state: State<'_, AppState>, req: PreflightRequest) -> CmdResult<PreflightReport> {
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    let q = req.question.trim();
    if q.is_empty() {
        problems.push("Add a question before starting.".into());
    }
    if q.chars().count() > bounds::MAX_QUESTION_CHARS {
        problems.push(format!("The question is longer than {} characters.", bounds::MAX_QUESTION_CHARS));
    }
    if req.reference_text.as_ref().map(|r| r.chars().count()).unwrap_or(0) > bounds::MAX_REFERENCE_CHARS {
        problems.push(format!("Reference text is longer than {} characters.", bounds::MAX_REFERENCE_CHARS));
    }
    let ceiling = MicroUsd::parse_dollars(&req.ceiling_usd).filter(|c| *c > MicroUsd::ZERO);
    if ceiling.is_none() {
        problems.push("Set a spending ceiling above zero.".into());
    }
    let doc = load_panel(&state, &req.panel_id)?;
    if let Err(e) = validate_panel(&doc) {
        problems.push(e);
    }
    let n = doc.seats.len();
    let mut seat_prices = Vec::new();
    let mut unverified = Vec::new();
    for s in &doc.seats {
        match parse_ref(&s.credential_ref).ok().and_then(|r| rt_secrets::exists(r).ok()) {
            Some(true) => {}
            _ => problems.push(format!("Seat '{}' has no saved API key.", s.label)),
        }
        match state.engine.prices.get(&s.provider, &s.model_id) {
            Some(p) => {
                if !p.verified {
                    unverified.push(p.key());
                }
                seat_prices.push((p.clone(), FrozenSeat::words_to_tokens(s.answer.max_output_words.max(s.review.max_output_words).max(s.improve.max_output_words)) as u64, if s.answer.reasoning_effort.is_some() { 4000 } else { 0 }));
            }
            None => problems.push(format!("No price record for {}/{}. Add one before running.", s.provider, s.model_id)),
        }
        if let Ok(adapter) = engine_adapter(&state.engine, &s.provider) {
            let caps = adapter.capabilities(&s.model_id);
            if !caps.supports_temperature && [&s.answer, &s.review, &s.improve].iter().any(|t| t.temperature.is_some()) {
                problems.push(format!("{} does not accept a temperature; clear it in the seat editor.", s.model_id));
            }
        } else {
            problems.push(format!("Provider {} is not available.", s.provider));
        }
    }
    if doc.web_search.any() {
        let seat_stages = doc.web_search.answer || doc.web_search.review || doc.web_search.improve;
        for s in doc.seats.iter().filter(|s| seat_stages && s.provider != "openrouter") {
            problems.push(format!("Web search runs through OpenRouter only; seat '{}' uses {}.", s.label, s.provider));
        }
        if doc.web_search.synthesize && doc.synthesizer.provider != "openrouter" {
            problems.push("Web search runs through OpenRouter only; the final writer uses another provider.".into());
        }
    }
    let web_calls = doc.web_search.calls(n);
    let web_fee = MicroUsd(rt_budget::WEB_SEARCH_FEE.0 * web_calls as i64);
    let synth_price = state.engine.prices.get(&doc.synthesizer.provider, &doc.synthesizer.model_id).cloned();
    if synth_price.is_none() {
        problems.push("No price record for the final writer's model.".into());
    }
    if !unverified.is_empty() {
        problems.push("Some prices are unverified. Confirm them in Settings before a paid run.".into());
    }
    let (low, high, base_calls, max_attempts) = match synth_price {
        Some(sp) if seat_prices.len() == n => {
            let seats: Vec<rt_budget::SeatPricing<'_>> = seat_prices.iter().map(|(p, out, reason)| rt_budget::SeatPricing { price: p, output_bound: *out, reasoning_bound: *reason }).collect();
            let synth = rt_budget::SeatPricing { price: &sp, output_bound: FrozenSeat::words_to_tokens(doc.synthesizer.tuning.max_output_words) as u64, reasoning_bound: 0 };
            let mut est = rt_budget::estimate_run(&seats, &synth, rt_budget::estimate_tokens(q), rt_budget::estimate_tokens(req.reference_text.as_deref().unwrap_or("")));
            est.low = est.low.saturating_add(web_fee);
            est.high = est.high.saturating_add(web_fee);
            if let Some(c) = ceiling {
                if est.low > c {
                    problems.push("Even the low estimate exceeds the spending ceiling.".into());
                } else if est.high > c {
                    warnings.push("The high estimate exceeds the ceiling; the run may block before it finishes.".into());
                }
            }
            (est.low, est.high, est.base_calls, est.max_attempts)
        }
        _ => (MicroUsd::ZERO, MicroUsd::ZERO, rt_core::stage::base_calls(n.max(2)), 0),
    };
    Ok(PreflightReport { ok: problems.is_empty(), seat_count: n, base_calls, max_attempts, estimate_low_usd: usd(low), estimate_high_usd: usd(high), unverified_prices: unverified, problems, warnings, web_search_calls: web_calls, web_search_fee_usd: usd(web_fee) })
}

#[tauri::command]
pub fn start_run(state: State<'_, AppState>, req: PreflightRequest) -> CmdResult<String> {
    let report = preflight_run(state.clone(), req.clone())?;
    if !report.ok {
        return Err(report.problems.join(" "));
    }
    if !state.running.lock().map_err(|_| "lock")?.is_empty() {
        return Err("Another run is still active. Wait for it or cancel it first.".into());
    }
    let doc = load_panel(&state, &req.panel_id)?;
    let ceiling = MicroUsd::parse_dollars(&req.ceiling_usd).ok_or("bad ceiling")?;
    let input = to_run_input(&doc, req.question.trim().to_string(), req.reference_text.filter(|r| !r.trim().is_empty()), ceiling, load_system_prompts(&state)?)?;
    let run_id = state.engine.create_run(input).map_err(err)?;
    spawn_execute(&state, run_id);
    Ok(run_id.to_string())
}

fn spawn_execute(state: &AppState, run_id: RunId) {
    let token = CancellationToken::new();
    state.running.lock().unwrap().insert(run_id, token.clone());
    let engine = state.engine.clone();
    let running = state.running.clone();
    tauri::async_runtime::spawn(async move {
        match engine.execute(run_id, token).await {
            Ok(state) => tracing::info!(%run_id, ?state, "run finished"),
            Err(e) => tracing::error!(%run_id, %e, "run failed"),
        }
        // a run that has stopped (blocked, completed, failed, cancelled) is no longer active, so it can be resumed
        running.lock().unwrap().remove(&run_id);
    });
}

#[tauri::command]
pub fn cancel_run(state: State<'_, AppState>, run_id: String) -> CmdResult<()> {
    let id = parse_run(&run_id)?;
    let token = state.running.lock().map_err(|_| "lock")?.remove(&id);
    match token {
        Some(t) => {
            t.cancel();
            Ok(())
        }
        None => Err("that run is not active".into()),
    }
}

#[tauri::command]
pub fn resume_run(state: State<'_, AppState>, run_id: String) -> CmdResult<()> {
    let id = parse_run(&run_id)?;
    if state.running.lock().map_err(|_| "lock")?.contains_key(&id) {
        return Err("that run is already active".into());
    }
    {
        let c = state.engine.ledger.writer().map_err(err)?;
        let run = runs::get_run(&c, id).map_err(err)?;
        if !matches!(run.state, RunState::Blocked | RunState::Interrupted) {
            return Err(format!("run is {:?}; only blocked or interrupted runs can resume", run.state));
        }
        // runs started under older, shorter time limits get today's limits when resumed
        let searching = runs::list_seats(&c, id).map_err(err)?.iter().any(|s| s.answer.web_search || s.review.web_search || s.improve.web_search);
        let (attempt, total) = if searching { (480u64, 3000u64) } else { (420, 2400) };
        let mut limits = run.limits_json.clone();
        let cur_attempt = limits["limits"]["attempt_deadline_secs"].as_u64().unwrap_or(0);
        let cur_total = limits["limits"]["run_deadline_secs"].as_u64().unwrap_or(0);
        if cur_attempt < attempt || cur_total < total {
            limits["limits"]["attempt_deadline_secs"] = json!(cur_attempt.max(attempt));
            limits["limits"]["run_deadline_secs"] = json!(cur_total.max(total));
            runs::set_run_limits(&c, id, &limits).map_err(err)?;
        }
    }
    spawn_execute(&state, id);
    Ok(())
}

#[tauri::command]
pub fn retry_uncertain(state: State<'_, AppState>, call_id: String) -> CmdResult<()> {
    let id = LogicalCallId::from_str(&call_id).map_err(|_| "invalid call id")?;
    state.engine.retry_uncertain(id).map_err(err)
}

fn summary(c: &rt_ledger::rusqlite::Connection, run: &runs::RunRow) -> CmdResult<RunSummary> {
    let seat_count = run.panel_snapshot_json["seat_count"].as_u64().unwrap_or(0) as usize;
    let (settled, unresolved) = runs::budget_totals(c, run.id).map_err(err)?;
    Ok(RunSummary { id: run.id, question: run.question.clone(), state: run.state, stage: run.stage, reason_code: run.reason_code.clone(), reason_text: run.reason_text.clone(), created_at: run.created_at.clone(), finished_at: run.finished_at.clone(), seat_count, is_sample: run.is_sample, settled_usd: usd(settled), unresolved_usd: usd(unresolved) })
}

#[tauri::command]
pub fn get_run_snapshot(state: State<'_, AppState>, run_id: String) -> CmdResult<RunSnapshot> {
    let id = parse_run(&run_id)?;
    let c = state.engine.ledger.writer().map_err(err)?;
    let run = runs::get_run(&c, id).map_err(err)?;
    let seats = runs::list_seats(&c, id).map_err(err)?;
    let returned: Vec<Option<String>> = {
        let mut stmt = c.prepare("SELECT model_returned_id FROM seats WHERE run_id = ?1 ORDER BY seat_index").map_err(err)?;
        let rows = stmt.query_map(params![id.to_string()], |r| r.get(0)).map_err(err)?;
        let v: Vec<Option<String>> = rows.collect::<Result<_, _>>().map_err(err)?;
        v
    };
    let calls = runs::list_all_calls(&c, id).map_err(err)?;
    let papers = runs::list_papers(&c, id).map_err(err)?;
    let last_seq: i64 = c.query_row("SELECT COALESCE(MAX(seq), 0) FROM run_events WHERE run_id = ?1", params![id.to_string()], |r| r.get(0)).map_err(err)?;
    let coverage = Stage::ALL
        .iter()
        .map(|s| Coverage { stage: *s, planned: s.call_count(seats.len()), accepted: calls.iter().filter(|c| c.stage == *s && c.state == rt_core::CallState::Accepted).count() })
        .collect();
    let final_paper = papers.iter().find(|p| p.kind == "final").map(|p| p.body.clone());
    let usage: Value = c
        .query_row(
            "SELECT COALESCE(SUM(json_extract(a.usage_json, '$.input_tokens')), 0), COALESCE(SUM(json_extract(a.usage_json, '$.output_tokens')), 0), COALESCE(SUM(json_extract(a.usage_json, '$.reasoning_tokens')), 0), COUNT(a.id) FROM attempts a JOIN logical_calls l ON l.id = a.logical_call_id WHERE l.run_id = ?1",
            params![id.to_string()],
            |r| Ok(json!({ "inputTokens": r.get::<_, i64>(0)?, "outputTokens": r.get::<_, i64>(1)?, "reasoningTokens": r.get::<_, i64>(2)?, "attempts": r.get::<_, i64>(3)? })),
        )
        .map_err(err)?;
    let in_flight: Vec<Value> = {
        let mut stmt = c
            .prepare("SELECT l.reviewer_seat, l.stage, a.ordinal, a.dispatched_at FROM attempts a JOIN logical_calls l ON l.id = a.logical_call_id WHERE l.run_id = ?1 AND a.completed_at IS NULL AND l.state = 'in_flight' ORDER BY a.dispatched_at")
            .map_err(err)?;
        let rows = stmt
            .query_map(params![id.to_string()], |r| Ok(json!({ "seat": r.get::<_, i64>(0)?, "stage": r.get::<_, String>(1)?, "attempt": r.get::<_, i64>(2)?, "since": r.get::<_, String>(3)? })))
            .map_err(err)?;
        let v: Vec<Value> = rows.collect::<Result<_, _>>().map_err(err)?;
        v
    };
    Ok(RunSnapshot {
        in_flight,
        usage,
        run: summary(&c, &run)?,
        seats: seats.iter().enumerate().map(|(i, s)| SeatSummary { index: s.seat_index, label: s.label.clone(), provider: s.provider.clone(), model_id: s.model_id.clone(), model_returned_id: returned.get(i).cloned().flatten() }).collect(),
        synthesizer: json!({ "provider": run.panel_snapshot_json["synthesizer"]["provider"], "modelId": run.panel_snapshot_json["synthesizer"]["model_id"] }),
        calls: calls.iter().map(|c| CallSummary { id: c.id, stage: c.stage, reviewer_seat: c.reviewer_seat, target_seat: c.target_seat, rotation: c.rotation, state: c.state, attempts_used: c.attempts_used, accepted_paper_id: c.accepted_paper_id }).collect(),
        papers: papers.iter().map(|p| PaperMeta { id: p.id, kind: p.kind.clone(), author_seat: p.author_seat, target_seat: p.target_seat }).collect(),
        coverage,
        last_seq,
        final_paper,
        web_search: WebSearchDoc { answer: seats.iter().any(|s| s.answer.web_search), review: seats.iter().any(|s| s.review.web_search), improve: seats.iter().any(|s| s.improve.web_search), synthesize: run.panel_snapshot_json["synthesizer"]["settings"]["web_search"].as_bool().unwrap_or(false) },
    })
}

/// Every accepted paper of a run with its validated body, for the stage browser.
#[tauri::command]
pub fn list_run_papers(state: State<'_, AppState>, run_id: String) -> CmdResult<Vec<Value>> {
    let id = parse_run(&run_id)?;
    let c = state.engine.ledger.writer().map_err(err)?;
    let papers = runs::list_papers(&c, id).map_err(err)?;
    let mut out = Vec::with_capacity(papers.len());
    for p in papers {
        let sources: Value = c
            .query_row("SELECT a.usage_json FROM papers p JOIN attempts a ON a.id = p.attempt_id WHERE p.id = ?1", params![p.id.to_string()], |r| r.get::<_, Option<String>>(0))
            .optional()
            .map_err(err)?
            .flatten()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .map(|u| u["web_sources"].clone())
            .filter(|v| v.is_array())
            .unwrap_or_else(|| json!([]));
        out.push(json!({ "id": p.id, "kind": p.kind, "authorSeat": p.author_seat, "targetSeat": p.target_seat, "body": p.body, "sources": sources }));
    }
    Ok(out)
}

#[tauri::command]
pub fn get_events(state: State<'_, AppState>, run_id: String, after_seq: i64) -> CmdResult<Vec<EventOut>> {
    let id = parse_run(&run_id)?;
    let evs = state.engine.ledger.events_since(id, after_seq, 2000).map_err(err)?;
    Ok(evs.into_iter().map(|(seq, kind, payload)| EventOut { seq, kind, payload }).collect())
}

#[tauri::command]
pub fn inspect_paper(state: State<'_, AppState>, paper_id: String) -> CmdResult<PaperDetail> {
    let pid = PaperId::from_str(&paper_id).map_err(|_| "invalid paper id")?;
    let c = state.engine.ledger.writer().map_err(err)?;
    let row = c
        .query_row("SELECT run_id, kind, author_seat, parent_paper_id, body_json, digest, attempt_id FROM papers WHERE id = ?1", params![pid.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, u8>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?)))
        .optional()
        .map_err(err)?
        .ok_or("paper not found")?;
    let target_seat = row.3.as_deref().and_then(|s| s.strip_prefix("seat:")).and_then(|s| s.parse().ok());
    let body: Value = serde_json::from_str(&row.4).map_err(err)?;
    let views: Vec<Value> = {
        let mut stmt = c.prepare("SELECT id, transform_version, view_text, digest, possibly_identifiable FROM artifact_views WHERE paper_id = ?1").map_err(err)?;
        let rows = stmt.query_map(params![pid.to_string()], |r| Ok(json!({ "id": r.get::<_, String>(0)?, "transformVersion": r.get::<_, String>(1)?, "text": r.get::<_, String>(2)?, "digest": r.get::<_, String>(3)?, "possiblyIdentifiable": r.get::<_, i64>(4)? != 0 }))).map_err(err)?;
        let v: Vec<Value> = rows.collect::<Result<_, _>>().map_err(err)?;
        v
    };
    let call_id: Option<String> = c.query_row("SELECT logical_call_id FROM attempts WHERE id = ?1", params![row.6], |r| r.get(0)).optional().map_err(err)?;
    let (envelope, attempts) = match call_id {
        Some(cid) => {
            let env: Option<Value> = c
                .query_row("SELECT stage, instructions, messages_json, schema_id, alias_map_json, digest FROM envelopes WHERE logical_call_id = ?1", params![cid], |r| Ok(json!({ "stage": r.get::<_, String>(0)?, "instructions": r.get::<_, String>(1)?, "messages": serde_json::from_str::<Value>(&r.get::<_, String>(2)?).unwrap_or(Value::Null), "schemaId": r.get::<_, String>(3)?, "aliasMap": serde_json::from_str::<Value>(&r.get::<_, String>(4)?).unwrap_or(Value::Null), "digest": r.get::<_, String>(5)? })))
                .optional()
                .map_err(err)?;
            let mut stmt = c.prepare("SELECT ordinal, kind, outcome, finish_reason, usage_json, error_category, error_sanitized, provider_request_id, dispatched_at, completed_at FROM attempts WHERE logical_call_id = ?1 ORDER BY ordinal").map_err(err)?;
            let rows = stmt
                .query_map(params![cid], |r| Ok(json!({ "ordinal": r.get::<_, u8>(0)?, "kind": r.get::<_, String>(1)?, "outcome": r.get::<_, Option<String>>(2)?, "finishReason": r.get::<_, Option<String>>(3)?, "usage": r.get::<_, Option<String>>(4)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()), "errorCategory": r.get::<_, Option<String>>(5)?, "error": r.get::<_, Option<String>>(6)?, "providerRequestId": r.get::<_, Option<String>>(7)?, "dispatchedAt": r.get::<_, String>(8)?, "completedAt": r.get::<_, Option<String>>(9)? })))
                .map_err(err)?;
            let attempts: Vec<Value> = rows.collect::<Result<_, _>>().map_err(err)?;
            (env, attempts)
        }
        None => (None, vec![]),
    };
    Ok(PaperDetail { paper: PaperMeta { id: pid, kind: row.1, author_seat: row.2, target_seat }, body, digest: row.5, views, envelope, attempts })
}

#[tauri::command]
pub fn list_runs(state: State<'_, AppState>, query: Option<String>, limit: Option<usize>) -> CmdResult<Vec<RunSummary>> {
    let c = state.engine.ledger.writer().map_err(err)?;
    let limit = limit.unwrap_or(100).min(500);
    let rows = match query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => {
            let safe = format!("\"{}\"", q.replace('"', " "));
            let mut stmt = c.prepare("SELECT run_id FROM runs_fts WHERE runs_fts MATCH ?1 LIMIT ?2").map_err(err)?;
            let rows = stmt.query_map(params![safe, limit as i64], |r| r.get(0)).map_err(err)?;
            let ids: Vec<String> = rows.collect::<Result<_, _>>().map_err(err)?;
            ids.iter().filter_map(|id| parse_run(id).ok()).filter_map(|id| runs::get_run(&c, id).ok()).collect::<Vec<_>>()
        }
        None => runs::list_runs(&c, limit).map_err(err)?,
    };
    rows.iter().map(|r| summary(&c, r)).collect()
}

#[tauri::command]
pub fn delete_run(state: State<'_, AppState>, run_id: String) -> CmdResult<bool> {
    let id = parse_run(&run_id)?;
    if let Some(t) = state.running.lock().map_err(|_| "lock")?.remove(&id) {
        t.cancel();
    }
    state.engine.delete_run(id).map_err(err)
}

// ---------------------------------------------------------------- settings, info

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> CmdResult<Value> {
    Ok(state.engine.ledger.get_setting("ui").map_err(err)?.unwrap_or_else(|| json!({})))
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Value) -> CmdResult<()> {
    if serde_json::to_string(&settings).map(|s| s.len()).unwrap_or(0) > 64 * 1024 {
        return Err("settings too large".into());
    }
    state.engine.ledger.set_setting("ui", &settings).map_err(err)
}

#[tauri::command]
pub fn app_info(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<Value> {
    Ok(json!({
        "appVersion": app.package_info().version.to_string(),
        "priceCatalog": state.engine.prices.version,
        "providerCatalog": state.providers.version,
        "dataDir": state.data_dir.to_string_lossy(),
        "schemaVersion": rt_ledger::SCHEMA_VERSION,
    }))
}

// ---------------------------------------------------------------- prompt preview

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub doc: PanelDoc,
    /// Seat index; `None` previews the final writer.
    pub seat: Option<usize>,
    /// "answer", "review", "improve" or "synthesize".
    pub stage: String,
    /// The prompts on screen, saved or not.
    pub system: SystemPrompts,
    pub question: Option<String>,
}

/// System and user text for one call, pulled out of the provider-shaped request body.
fn wire_texts(body: &Value) -> (String, String) {
    let text_of = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(|b| b["text"].as_str().or(b.as_str())).collect::<Vec<_>>().join("\n\n"),
            _ => String::new(),
        }
    };
    if let Some(s) = body["systemInstruction"]["parts"][0]["text"].as_str() {
        let user = body["contents"].as_array().map(|c| c.iter().map(|m| text_of(&m["parts"])).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
        return (s.to_string(), user);
    }
    if let Some(s) = body["instructions"].as_str() {
        let user = body["input"].as_array().map(|c| c.iter().map(|m| text_of(&m["content"])).collect::<Vec<_>>().join("\n\n")).unwrap_or_else(|| text_of(&body["input"]));
        return (s.to_string(), user);
    }
    let msgs = body["messages"].as_array().cloned().unwrap_or_default();
    let user = msgs.iter().filter(|m| m["role"] == "user").map(|m| text_of(&m["content"])).collect::<Vec<_>>().join("\n\n");
    if let Some(s) = body["system"].as_str() {
        return (s.to_string(), user);
    }
    let system = msgs.iter().filter(|m| m["role"] == "system").map(|m| text_of(&m["content"])).collect::<Vec<_>>().join("\n\n");
    (system, user)
}

/// Build the real request one agent would receive at one stage, using placeholder papers for the
/// material it would be shown. Nothing is sent. The key is never part of the body.
#[tauri::command]
pub fn preview_prompt(state: State<'_, AppState>, req: PreviewRequest) -> CmdResult<Value> {
    use rt_core::Role;
    validate_system_prompts(&req.system)?;
    let question = req.question.as_deref().map(str::trim).filter(|q| !q.is_empty()).unwrap_or("(Your question appears here.)").to_string();
    let input = to_run_input(&req.doc, question, None, MicroUsd(1_000_000), req.system.clone())?;
    let n = input.seats.len();
    if n < bounds::MIN_SEATS {
        return Err("add at least two agents to preview a review".into());
    }
    let stage = match req.stage.as_str() {
        "answer" => Stage::Answer,
        "review" => Stage::Review,
        "improve" => Stage::Improve,
        "synthesize" => Stage::Synthesize,
        _ => return Err("unknown stage".into()),
    };
    let call = match (stage, req.seat) {
        (Stage::Synthesize, _) => rt_envelope::CallSpec::Synthesize,
        (_, None) => return Err("choose an agent".into()),
        (_, Some(s)) if s >= n => return Err("no such agent".into()),
        (Stage::Answer, Some(s)) => rt_envelope::CallSpec::Answer { seat: s as u8 },
        (Stage::Review, Some(s)) => rt_envelope::CallSpec::Review { reviewer: s as u8, target: ((s + 1) % n) as u8 },
        (Stage::Improve, Some(s)) => rt_envelope::CallSpec::Improve { seat: s as u8 },
    };
    let run_id = RunId::new();
    let paper = |kind: rt_envelope::PaperKind, author: usize, target: Option<usize>, body: Value| rt_envelope::Paper { id: PaperId::new(), run_id, kind, author_seat: author as u8, target_seat: target.map(|t| t as u8), body };
    let mut papers = rt_envelope::Papers::default();
    for i in 0..n {
        papers.answers.insert(i as u8, paper(rt_envelope::PaperKind::Answer, i, None, json!({"schema_version":"answer.v1","answer":"[An agent's full answer appears here, with names removed.]","assumptions":["[its assumptions]"],"claims":[{"id":"C1","text":"[each claim, numbered C1, C2, ...]"}],"limitations":["[its limitations]"]})));
        papers.improved.insert(i as u8, paper(rt_envelope::PaperKind::Improved, i, None, json!({"schema_version":"improved.v1","decisions":[],"answer":"[An agent's improved answer appears here, with names removed.]","claims":[{"id":"C1","text":"[each revised claim]"}],"unresolved":["[anything still unresolved]"]})));
        for t in 0..n {
            if t != i {
                papers.reviews.insert((i as u8, t as u8), paper(rt_envelope::PaperKind::Review, i, Some(t), json!({"schema_version":"review.v1","target_alias":"A-preview","correct_claim_ids":["C1"],"issues":[{"id":"I1","target_claim_id":"C1","explanation":"[each issue the reviewer found appears here]"}],"missing":["[anything the reviewer said was missing]"],"needs_external_verification":false})));
            }
        }
    }
    let ctx = rt_envelope::RunContext { run_id, seed: "preview", question: &input.question, references: &[], seats: &input.seats, synthesizer_prompt: &input.synthesizer.prompt, system: &req.system };
    let env = rt_envelope::build(&ctx, call, &papers).map_err(err)?;
    let (provider, model, settings) = match call {
        rt_envelope::CallSpec::Synthesize => (input.synthesizer.provider.clone(), input.synthesizer.model_id.clone(), input.synthesizer.settings.clone()),
        _ => {
            let s = &input.seats[req.seat.unwrap_or(0)];
            let role = match stage { Stage::Answer => Role::Answer, Stage::Review => Role::Review, _ => Role::Improve };
            (s.provider.clone(), s.model_id.clone(), s.settings_for(role).clone())
        }
    };
    let kind = match stage { Stage::Answer => rt_schemas::PaperKind::Answer, Stage::Review => rt_schemas::PaperKind::Review, Stage::Improve => rt_schemas::PaperKind::Improved, Stage::Synthesize => rt_schemas::PaperKind::Final };
    let json_schema = state.engine.prices.get(&provider, &model).map(|p| p.structured_output).unwrap_or(true);
    let adapter = engine_adapter(&state.engine, &provider)?;
    let params = rt_adapters::CallParams { model: model.clone(), settings, schema: rt_schemas::schema_for_prompt(kind), schema_name: kind.schema_id().replace('.', "_"), json_schema, truncation_retries: 0 };
    let wire = adapter.build_request(&env, &params).map_err(err)?;
    let (system, user) = wire_texts(&wire.body);
    Ok(json!({ "provider": provider, "model": model, "url": wire.url, "system": system, "user": user, "request": wire.body }))
}
