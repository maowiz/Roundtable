//! Run persistence used by the engine. Every function takes a connection or transaction so the
//! engine composes the commit protocol's transactions itself (plan section 8.2).

use crate::{now, LedgerError, Result};
use rt_core::{AttemptId, BudgetEntryId, CallState, EnvelopeId, LogicalCallId, PaperId, RunId, RunState, Stage, ViewId};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRow {
    pub id: RunId,
    pub state: RunState,
    pub stage: Stage,
    pub reason_code: Option<String>,
    pub reason_text: Option<String>,
    pub question: String,
    pub reference_text: Option<String>,
    pub panel_snapshot_json: serde_json::Value,
    pub limits_json: serde_json::Value,
    pub price_catalog_version: String,
    pub schema_version: String,
    pub app_version: String,
    pub adapter_versions_json: serde_json::Value,
    pub seed: String,
    pub is_sample: bool,
    pub created_at: String,
    pub started_at: Option<String>,
    pub deadline_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallRow {
    pub id: LogicalCallId,
    pub run_id: RunId,
    pub stage: Stage,
    pub reviewer_seat: u8,
    pub target_seat: Option<u8>,
    pub rotation: Option<u8>,
    pub state: CallState,
    pub attempts_used: u8,
    pub schema_retries_used: u8,
    pub accepted_paper_id: Option<PaperId>,
    pub envelope_id: Option<EnvelopeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaperRow {
    pub id: PaperId,
    pub run_id: RunId,
    pub kind: String,
    pub author_seat: u8,
    pub target_paper_id: Option<PaperId>,
    pub target_seat: Option<u8>,
    pub schema_version: String,
    pub body: serde_json::Value,
    pub digest: String,
    pub attempt_id: AttemptId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptRow {
    pub id: AttemptId,
    pub logical_call_id: LogicalCallId,
    pub ordinal: u8,
    pub kind: String,
    pub request_digest: String,
    pub serialized_body_redacted: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttemptOutcomeRow {
    pub outcome: String,
    pub finish_reason: Option<String>,
    pub usage_json: serde_json::Value,
    pub raw_output_bounded: Option<String>,
    pub error_category: Option<String>,
    pub error_sanitized: Option<String>,
    pub provider_request_id: Option<String>,
    pub first_byte_ms: Option<u64>,
    pub total_ms: u64,
}

fn parse_stage(s: &str) -> Result<Stage> {
    match s {
        "answer" => Ok(Stage::Answer),
        "review" => Ok(Stage::Review),
        "improve" => Ok(Stage::Improve),
        "synthesize" => Ok(Stage::Synthesize),
        other => Err(LedgerError::Corrupt(format!("bad stage {other}"))),
    }
}

fn parse_run_state(s: &str) -> Result<RunState> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| LedgerError::Corrupt(format!("bad run state {s}")))
}

fn parse_call_state(s: &str) -> Result<CallState> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| LedgerError::Corrupt(format!("bad call state {s}")))
}

fn parse_id<T: std::str::FromStr>(s: &str) -> Result<T> {
    s.parse().map_err(|_| LedgerError::Corrupt(format!("bad id {s}")))
}

pub fn insert_run(c: &Connection, r: &RunRow) -> Result<()> {
    c.execute(
        "INSERT INTO runs(id,state,stage,reason_code,reason_text,question,reference_text,panel_snapshot_json,limits_json,price_catalog_version,schema_version,app_version,adapter_versions_json,seed,is_sample,created_at,started_at,deadline_at,finished_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![r.id.to_string(), r.state.as_str(), r.stage.as_str(), r.reason_code, r.reason_text, r.question, r.reference_text, r.panel_snapshot_json.to_string(), r.limits_json.to_string(), r.price_catalog_version, r.schema_version, r.app_version, r.adapter_versions_json.to_string(), r.seed, r.is_sample as i64, r.created_at, r.started_at, r.deadline_at, r.finished_at],
    )?;
    c.execute("INSERT INTO runs_fts(run_id, question, final_answer) VALUES(?1, ?2, '')", params![r.id.to_string(), r.question])?;
    Ok(())
}

pub fn get_run(c: &Connection, id: RunId) -> Result<RunRow> {
    c.query_row(
        "SELECT id,state,stage,reason_code,reason_text,question,reference_text,panel_snapshot_json,limits_json,price_catalog_version,schema_version,app_version,adapter_versions_json,seed,is_sample,created_at,started_at,deadline_at,finished_at FROM runs WHERE id = ?1",
        params![id.to_string()],
        |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, String>(5)?, r.get::<_, Option<String>>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?, r.get::<_, String>(9)?, r.get::<_, String>(10)?, r.get::<_, String>(11)?, r.get::<_, String>(12)?, r.get::<_, String>(13)?, r.get::<_, i64>(14)?, r.get::<_, String>(15)?, r.get::<_, Option<String>>(16)?, r.get::<_, Option<String>>(17)?, r.get::<_, Option<String>>(18)?))
        },
    )
    .optional()?
    .ok_or_else(|| LedgerError::NotFound(format!("run {id}")))
    .and_then(|t| {
        Ok(RunRow { id: parse_id(&t.0)?, state: parse_run_state(&t.1)?, stage: parse_stage(&t.2)?, reason_code: t.3, reason_text: t.4, question: t.5, reference_text: t.6, panel_snapshot_json: serde_json::from_str(&t.7)?, limits_json: serde_json::from_str(&t.8)?, price_catalog_version: t.9, schema_version: t.10, app_version: t.11, adapter_versions_json: serde_json::from_str(&t.12)?, seed: t.13, is_sample: t.14 != 0, created_at: t.15, started_at: t.16, deadline_at: t.17, finished_at: t.18 })
    })
}

pub fn update_run_state(c: &Connection, id: RunId, state: RunState, stage: Stage, reason_code: Option<&str>, reason_text: Option<&str>) -> Result<()> {
    let finished = if state.is_terminal() { Some(now()) } else { None };
    let n = c.execute(
        "UPDATE runs SET state = ?2, stage = ?3, reason_code = ?4, reason_text = ?5, started_at = COALESCE(started_at, CASE WHEN ?2 = 'answering' THEN ?6 END), finished_at = COALESCE(?7, finished_at) WHERE id = ?1",
        params![id.to_string(), state.as_str(), stage.as_str(), reason_code, reason_text, now(), finished],
    )?;
    if n == 0 {
        return Err(LedgerError::NotFound(format!("run {id}")));
    }
    Ok(())
}

pub fn list_runs(c: &Connection, limit: usize) -> Result<Vec<RunRow>> {
    let mut stmt = c.prepare("SELECT id FROM runs ORDER BY created_at DESC LIMIT ?1")?;
    let ids: Vec<String> = stmt.query_map(params![limit as i64], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    ids.iter().map(|id| get_run(c, parse_id(id)?)).collect()
}

/// Runs that are neither terminal nor draft: candidates for `interrupted` at launch (PRD 8.2).
pub fn non_terminal_runs(c: &Connection) -> Result<Vec<RunId>> {
    let mut stmt = c.prepare("SELECT id FROM runs WHERE state NOT IN ('draft','completed','cancelled','failed')")?;
    let ids: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<std::result::Result<_, _>>()?;
    ids.iter().map(|s| parse_id(s)).collect()
}

pub fn insert_seat(c: &Connection, run: RunId, seat: &rt_core::FrozenSeat, prompt_digests: &serde_json::Value) -> Result<()> {
    let settings = serde_json::json!({ "answer": seat.answer, "review": seat.review, "improve": seat.improve, "limits": seat.limits, "model_is_alias": seat.model_is_alias });
    c.execute(
        "INSERT INTO seats(run_id,seat_index,label,provider,credential_ref,model_id,settings_json,prompt_answer,prompt_review,prompt_improve,prompt_digests_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![run.to_string(), seat.seat_index, seat.label, seat.provider, seat.credential_ref.to_string(), seat.model_id, settings.to_string(), seat.prompt_answer, seat.prompt_review, seat.prompt_improve, prompt_digests.to_string()],
    )?;
    Ok(())
}

pub fn set_returned_model(c: &Connection, run: RunId, seat: u8, model: &str) -> Result<()> {
    c.execute("UPDATE seats SET model_returned_id = COALESCE(model_returned_id, ?3) WHERE run_id = ?1 AND seat_index = ?2", params![run.to_string(), seat, model])?;
    Ok(())
}

pub fn insert_call(c: &Connection, call: &CallRow) -> Result<()> {
    c.execute(
        "INSERT INTO logical_calls(id,run_id,stage,reviewer_seat,target_seat,rotation,state,attempts_used,schema_retries_used,accepted_paper_id,envelope_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
        params![call.id.to_string(), call.run_id.to_string(), call.stage.as_str(), call.reviewer_seat, call.target_seat, call.rotation, call.state.as_str(), call.attempts_used, call.schema_retries_used, call.accepted_paper_id.map(|p| p.to_string()), call.envelope_id.map(|e| e.to_string()), now()],
    )?;
    Ok(())
}

type CallTuple = (String, String, String, u8, Option<u8>, Option<u8>, String, u8, u8, Option<String>, Option<String>);

fn call_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<CallTuple> {
    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?))
}

fn call_convert(t: CallTuple) -> Result<CallRow> {
    Ok(CallRow { id: parse_id(&t.0)?, run_id: parse_id(&t.1)?, stage: parse_stage(&t.2)?, reviewer_seat: t.3, target_seat: t.4, rotation: t.5, state: parse_call_state(&t.6)?, attempts_used: t.7, schema_retries_used: t.8, accepted_paper_id: t.9.as_deref().map(parse_id).transpose()?, envelope_id: t.10.as_deref().map(parse_id).transpose()? })
}

const CALL_COLS: &str = "id,run_id,stage,reviewer_seat,target_seat,rotation,state,attempts_used,schema_retries_used,accepted_paper_id,envelope_id";

pub fn list_calls(c: &Connection, run: RunId, stage: Stage) -> Result<Vec<CallRow>> {
    let mut stmt = c.prepare(&format!("SELECT {CALL_COLS} FROM logical_calls WHERE run_id = ?1 AND stage = ?2 ORDER BY rotation, reviewer_seat"))?;
    let rows = stmt.query_map(params![run.to_string(), stage.as_str()], call_from_row)?;
    rows.map(|r| call_convert(r?)).collect()
}

pub fn list_all_calls(c: &Connection, run: RunId) -> Result<Vec<CallRow>> {
    let mut stmt = c.prepare(&format!("SELECT {CALL_COLS} FROM logical_calls WHERE run_id = ?1 ORDER BY created_at, rotation, reviewer_seat"))?;
    let rows = stmt.query_map(params![run.to_string()], call_from_row)?;
    rows.map(|r| call_convert(r?)).collect()
}

pub fn get_call(c: &Connection, id: LogicalCallId) -> Result<CallRow> {
    let row = c.query_row(&format!("SELECT {CALL_COLS} FROM logical_calls WHERE id = ?1"), params![id.to_string()], call_from_row).optional()?;
    row.map(call_convert).transpose()?.ok_or_else(|| LedgerError::NotFound(format!("call {id}")))
}

/// Replace a run's limits (used when a resume raises time limits that proved too short).
pub fn set_run_limits(c: &Connection, id: RunId, limits_json: &serde_json::Value) -> Result<()> {
    c.execute("UPDATE runs SET limits_json = ?2 WHERE id = ?1", params![id.to_string(), limits_json.to_string()])?;
    Ok(())
}

/// Move a run's deadline (a resumed run gets a fresh time allowance).
pub fn set_run_deadline(c: &Connection, id: RunId, deadline_at: &str) -> Result<()> {
    c.execute("UPDATE runs SET deadline_at = ?2 WHERE id = ?1", params![id.to_string(), deadline_at])?;
    Ok(())
}

/// How many attempts of one call ended with the given outcome (e.g. "truncated").
pub fn count_attempts_with_outcome(c: &Connection, call: LogicalCallId, outcome: &str) -> Result<u32> {
    Ok(c.query_row("SELECT COUNT(*) FROM attempts WHERE logical_call_id = ?1 AND outcome = ?2", params![call.to_string(), outcome], |r| r.get::<_, i64>(0))? as u32)
}

pub fn update_call(c: &Connection, id: LogicalCallId, state: CallState, attempts_used: u8, schema_retries_used: u8, accepted: Option<PaperId>) -> Result<()> {
    c.execute(
        "UPDATE logical_calls SET state = ?2, attempts_used = ?3, schema_retries_used = ?4, accepted_paper_id = COALESCE(?5, accepted_paper_id), updated_at = ?6 WHERE id = ?1",
        params![id.to_string(), state.as_str(), attempts_used, schema_retries_used, accepted.map(|p| p.to_string()), now()],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_envelope(c: &Connection, id: EnvelopeId, run: RunId, call: LogicalCallId, stage: Stage, instructions: &str, messages: &serde_json::Value, schema_id: &str, allowed_view_ids: &[ViewId], alias_map: &serde_json::Value, digest: &str) -> Result<()> {
    let views: Vec<String> = allowed_view_ids.iter().map(|v| v.to_string()).collect();
    c.execute(
        "INSERT INTO envelopes(id,run_id,logical_call_id,stage,instructions,messages_json,schema_id,allowed_view_ids_json,alias_map_json,digest,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![id.to_string(), run.to_string(), call.to_string(), stage.as_str(), instructions, messages.to_string(), schema_id, serde_json::to_string(&views)?, alias_map.to_string(), digest, now()],
    )?;
    c.execute("UPDATE logical_calls SET envelope_id = ?2, updated_at = ?3 WHERE id = ?1", params![call.to_string(), id.to_string(), now()])?;
    Ok(())
}

pub fn get_envelope_digest(c: &Connection, call: LogicalCallId) -> Result<Option<String>> {
    Ok(c.query_row("SELECT digest FROM envelopes WHERE logical_call_id = ?1", params![call.to_string()], |r| r.get(0)).optional()?)
}

pub fn insert_view(c: &Connection, id: ViewId, paper: PaperId, transform_version: &str, text: &str, digest: &str, possibly_identifiable: bool) -> Result<()> {
    c.execute(
        "INSERT INTO artifact_views(id,paper_id,transform_version,view_text,digest,possibly_identifiable) VALUES(?1,?2,?3,?4,?5,?6)",
        params![id.to_string(), paper.to_string(), transform_version, text, digest, possibly_identifiable as i64],
    )?;
    Ok(())
}

pub fn insert_attempt(c: &Connection, a: &AttemptRow) -> Result<()> {
    c.execute(
        "INSERT INTO attempts(id,logical_call_id,ordinal,kind,request_digest,serialized_body_redacted,dispatched_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![a.id.to_string(), a.logical_call_id.to_string(), a.ordinal, a.kind, a.request_digest, a.serialized_body_redacted.to_string(), now()],
    )?;
    Ok(())
}

pub fn finish_attempt(c: &Connection, id: AttemptId, o: &AttemptOutcomeRow) -> Result<()> {
    c.execute(
        "UPDATE attempts SET completed_at = ?2, provider_request_id = ?3, outcome = ?4, finish_reason = ?5, usage_json = ?6, raw_output_bounded = ?7, error_category = ?8, error_sanitized = ?9, first_byte_at = ?10 WHERE id = ?1",
        params![id.to_string(), now(), o.provider_request_id, o.outcome, o.finish_reason, o.usage_json.to_string(), o.raw_output_bounded, o.error_category, o.error_sanitized, o.first_byte_ms.map(|ms| ms.to_string())],
    )?;
    Ok(())
}

/// Attempts that were dispatched but never finished: uncertain after a crash (PRD 8.4 step 2).
pub fn unfinished_attempts(c: &Connection, run: RunId) -> Result<Vec<(AttemptId, LogicalCallId)>> {
    let mut stmt = c.prepare("SELECT a.id, a.logical_call_id FROM attempts a JOIN logical_calls l ON l.id = a.logical_call_id WHERE l.run_id = ?1 AND a.completed_at IS NULL")?;
    let rows = stmt.query_map(params![run.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    rows.map(|r| { let (a, l) = r?; Ok((parse_id(&a)?, parse_id(&l)?)) }).collect()
}

pub fn insert_paper(c: &Connection, p: &PaperRow) -> Result<()> {
    c.execute(
        "INSERT INTO papers(id,run_id,kind,author_seat,target_paper_id,parent_paper_id,schema_version,body_json,digest,attempt_id,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![p.id.to_string(), p.run_id.to_string(), p.kind, p.author_seat, p.target_paper_id.map(|x| x.to_string()), p.target_seat.map(|s| format!("seat:{s}")), p.schema_version, p.body.to_string(), p.digest, p.attempt_id.to_string(), now()],
    )?;
    if p.kind == "final" {
        c.execute("UPDATE runs_fts SET final_answer = ?2 WHERE run_id = ?1", params![p.run_id.to_string(), p.body["answer"].as_str().unwrap_or("")])?;
    }
    Ok(())
}

pub fn list_papers(c: &Connection, run: RunId) -> Result<Vec<PaperRow>> {
    let mut stmt = c.prepare("SELECT id,run_id,kind,author_seat,target_paper_id,parent_paper_id,schema_version,body_json,digest,attempt_id FROM papers WHERE run_id = ?1 ORDER BY created_at")?;
    let rows = stmt.query_map(params![run.to_string()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, u8>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?, r.get::<_, String>(9)?)))?;
    rows.map(|r| {
        let t = r?;
        let target_seat = t.5.as_deref().and_then(|s| s.strip_prefix("seat:")).and_then(|s| s.parse().ok());
        Ok(PaperRow { id: parse_id(&t.0)?, run_id: parse_id(&t.1)?, kind: t.2, author_seat: t.3, target_paper_id: t.4.as_deref().map(parse_id).transpose()?, target_seat, schema_version: t.6, body: serde_json::from_str(&t.7)?, digest: t.8, attempt_id: parse_id(&t.9)? })
    })
    .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn insert_budget_entry(c: &Connection, id: BudgetEntryId, run: RunId, call: Option<LogicalCallId>, attempt: Option<AttemptId>, kind: &str, amount: rt_core::MicroUsd, price_ref: &str) -> Result<()> {
    c.execute(
        "INSERT INTO budget_entries(id,run_id,logical_call_id,attempt_id,kind,amount_usd_micro,price_ref,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id.to_string(), run.to_string(), call.map(|x| x.to_string()), attempt.map(|x| x.to_string()), kind, amount.0, price_ref, now()],
    )?;
    Ok(())
}

/// Sum of settled and unresolved entries so a resumed run starts from the persisted truth.
pub fn budget_totals(c: &Connection, run: RunId) -> Result<(rt_core::MicroUsd, rt_core::MicroUsd)> {
    let settled: i64 = c.query_row("SELECT COALESCE(SUM(amount_usd_micro),0) FROM budget_entries WHERE run_id = ?1 AND kind = 'settle'", params![run.to_string()], |r| r.get(0))?;
    let unresolved: i64 = c.query_row("SELECT COALESCE(SUM(amount_usd_micro),0) FROM budget_entries WHERE run_id = ?1 AND kind = 'unresolved'", params![run.to_string()], |r| r.get(0))?;
    Ok((rt_core::MicroUsd(settled), rt_core::MicroUsd(unresolved)))
}

/// Delete a run and everything under it. Must run inside `Ledger::maintenance` (DATA-01).
pub fn delete_run(tx: &Connection, run: RunId) -> Result<bool> {
    tx.execute("DELETE FROM runs_fts WHERE run_id = ?1", params![run.to_string()])?;
    let n = tx.execute("DELETE FROM runs WHERE id = ?1", params![run.to_string()])?;
    Ok(n == 1)
}

/// Read the frozen seats of a run back into core types.
pub fn list_seats(c: &Connection, run: RunId) -> Result<Vec<rt_core::FrozenSeat>> {
    let mut stmt = c.prepare("SELECT seat_index,label,provider,credential_ref,model_id,settings_json,prompt_answer,prompt_review,prompt_improve FROM seats WHERE run_id = ?1 ORDER BY seat_index")?;
    let rows = stmt.query_map(params![run.to_string()], |r| Ok((r.get::<_, u8>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?)))?;
    rows.map(|r| {
        let t = r?;
        let s: serde_json::Value = serde_json::from_str(&t.5)?;
        let role = |k: &str| -> Result<rt_core::RoleSettings> { serde_json::from_value(s[k].clone()).map_err(LedgerError::Json) };
        Ok(rt_core::FrozenSeat {
            seat_index: t.0,
            label: t.1,
            provider: t.2,
            credential_ref: parse_id(&t.3)?,
            model_id: t.4,
            model_is_alias: s["model_is_alias"].as_bool().unwrap_or(false),
            answer: role("answer")?,
            review: role("review")?,
            improve: role("improve")?,
            prompt_answer: t.6,
            prompt_review: t.7,
            prompt_improve: t.8,
            limits: serde_json::from_value(s["limits"].clone())?,
        })
    })
    .collect()
}
