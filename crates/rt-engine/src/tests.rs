//! End-to-end engine tests against the fake provider: routing, commit protocol, retries,
//! cancellation, blocking, resume, budget and wire-level canaries.

use super::*;
use rt_adapters::fake::{FakeProvider, Scripted};
use rt_adapters::ErrorCategory;
use rt_core::{RoleSettings, SeatLimits};
use rt_testkit::bodies;
use std::sync::Arc;

fn seat(i: u8, provider: &str, cred: CredentialRef) -> FrozenSeat {
    let rs = RoleSettings { temperature: Some(0.5), max_output_tokens: 800, reasoning_effort: None, web_search: false };
    FrozenSeat { seat_index: i, label: format!("Seat{i}"), provider: provider.into(), credential_ref: cred, model_id: "m".into(), model_is_alias: false, answer: rs.clone(), review: rs.clone(), improve: rs, prompt_answer: "ap".into(), prompt_review: "rp".into(), prompt_improve: "ip".into(), limits: SeatLimits { cost_cap: MicroUsd(10_000_000) } }
}

fn extract_aliases(text: &str, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    let pat = format!("[{prefix}-");
    let mut search = 0;
    while let Some(pos) = text[search..].find(&pat) {
        let start = search + pos + 1;
        let end = text[start..].find(']').map(|e| start + e).unwrap_or(text.len());
        out.push(text[start..end].to_string());
        search = end;
    }
    out
}

/// A decider that answers every request with a valid paper derived from what it was shown,
/// carrying canaries so wire bodies can be checked for leaks.
fn valid_decider() -> rt_adapters::fake::Decider {
    Arc::new(|req: &WireRequest| {
        let call = &req.body["call"];
        let text = req.body["messages"].to_string();
        match call["kind"].as_str().unwrap_or("") {
            "answer" => Scripted::Ok(bodies::answer(&rt_testkit::canary("answer", call["seat"].as_u64().unwrap() as u8, ""))),
            "review" => {
                let alias = extract_aliases(&text, "A").into_iter().next().unwrap();
                Scripted::Ok(bodies::review(&alias, &rt_testkit::canary("review", call["reviewer"].as_u64().unwrap() as u8, &format!("OF_{}", call["target"]))))
            }
            "improve" => {
                let decisions: Vec<(String, String)> = extract_aliases(&text, "R").into_iter().map(|a| (a, "I1".to_string())).collect();
                Scripted::Ok(bodies::improved(&decisions, &rt_testkit::canary("improved", call["seat"].as_u64().unwrap() as u8, "")))
            }
            _ => {
                let alias = extract_aliases(&text, "C").into_iter().next().unwrap();
                Scripted::Ok(bodies::final_paper(&alias, "CANARY_FINAL"))
            }
        }
    })
}

/// Like `valid_decider`, but the first `bad` answer requests from seat 0 return unparseable text.
fn seat0_invalid_decider(bad: usize) -> rt_adapters::fake::Decider {
    let valid = valid_decider();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    Arc::new(move |req: &WireRequest| {
        if req.body["call"]["kind"] == "answer" && req.body["call"]["seat"] == 0 && count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < bad {
            Scripted::Raw("{".into())
        } else {
            valid(req)
        }
    })
}

fn seat0_failing_decider(outcome: AttemptOutcome, category: ErrorCategory, times: usize) -> rt_adapters::fake::Decider {
    let valid = valid_decider();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    Arc::new(move |req: &WireRequest| {
        if req.body["call"]["kind"] == "answer" && req.body["call"]["seat"] == 0 && count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < times {
            Scripted::Fail(outcome.clone(), category.clone())
        } else {
            valid(req)
        }
    })
}

struct Harness {
    engine: Arc<Engine>,
    fake: Arc<FakeProvider>,
    cred: CredentialRef,
}

fn harness(decider: rt_adapters::fake::Decider) -> Harness {
    let ledger = Arc::new(Ledger::in_memory().unwrap());
    let fake = Arc::new(FakeProvider::new("openai", decider));
    let cred = CredentialRef::new();
    let secrets = MapSecrets([(cred, "sk-test-key".to_string())].into());
    let catalog = Catalog::bundled().unwrap();
    let mut engine = Arc::try_unwrap(Engine::new(ledger, vec![fake.clone()], Arc::new(secrets), catalog)).ok().unwrap();
    engine.allow_unverified_prices = true;
    Harness { engine: Arc::new(engine), fake, cred }
}

fn input(h: &Harness, n: u8) -> RunInput {
    RunInput {
        system_prompts: Default::default(),
        question: "How should we launch?".into(),
        reference_text: Some("First source.\n\nSecond source.".into()),
        seats: (0..n).map(|i| seat(i, "openai", h.cred)).collect(),
        synthesizer: SynthesizerSeat { provider: "openai".into(), credential_ref: h.cred, model_id: "gpt-5.6".into(), model_is_alias: true, settings: RoleSettings { temperature: None, max_output_tokens: 1200, reasoning_effort: None, web_search: false }, prompt: "synth".into() },
        ceiling: MicroUsd::parse_dollars("5.00").unwrap(),
        seed: Some("seed".into()),
        is_sample: false,
        limits: Limits { global_in_flight: 8, per_provider_in_flight: 4, attempt_deadline_secs: 5, run_deadline_secs: 300 },
    }
}

fn set_seat_model(mut i: RunInput, model: &str) -> RunInput {
    for s in &mut i.seats {
        s.model_id = model.into();
    }
    i
}

#[tokio::test(start_paused = true)]
async fn four_seat_run_completes_with_exact_coverage_and_no_leaks() {
    let h = harness(valid_decider());
    let mut rx = h.engine.subscribe();
    let run = h.engine.create_run(set_seat_model(input(&h, 4), "gpt-5.6")).unwrap();
    let state = h.engine.execute(run, CancellationToken::new()).await.unwrap();
    assert_eq!(state, RunState::Completed);

    let c = h.engine.ledger.writer().unwrap();
    let calls = runs::list_all_calls(&c, run).unwrap();
    assert_eq!(calls.len(), 21);
    assert!(calls.iter().all(|c| c.state == CallState::Accepted && c.attempts_used == 1));
    let papers = runs::list_papers(&c, run).unwrap();
    assert_eq!(papers.iter().filter(|p| p.kind == "answer").count(), 4);
    assert_eq!(papers.iter().filter(|p| p.kind == "review").count(), 12);
    assert_eq!(papers.iter().filter(|p| p.kind == "improved").count(), 4);
    assert_eq!(papers.iter().filter(|p| p.kind == "final").count(), 1);
    let row = runs::get_run(&c, run).unwrap();
    assert_eq!(row.state, RunState::Completed);
    assert!(row.finished_at.is_some());

    // wire-level canaries: 21 requests, each carrying only what its stage allows (ISO-03/04/05)
    let reqs = h.fake.recorded();
    assert_eq!(reqs.len(), 21);
    for r in &reqs {
        let text = r.body.to_string();
        assert!(!text.contains("sk-test-key"), "secret leaked into a request body");
        match r.body["call"]["kind"].as_str().unwrap() {
            "answer" => assert!(!text.contains("CANARY_")),
            "review" => {
                let target = r.body["call"]["target"].as_u64().unwrap();
                assert!(text.contains(&format!("CANARY_ANSWER_{target}")));
                for s in 0..4 {
                    if s != target {
                        assert!(!text.contains(&format!("CANARY_ANSWER_{s}")));
                    }
                }
                assert!(!text.contains("CANARY_REVIEW_"));
            }
            "improve" => {
                let seat = r.body["call"]["seat"].as_u64().unwrap();
                assert!(text.contains(&format!("CANARY_ANSWER_{seat}")));
                for s in 0..4 {
                    if s != seat {
                        assert!(!text.contains(&format!("CANARY_ANSWER_{s}")));
                        assert!(text.contains(&format!("CANARY_REVIEW_{s}_OF_{seat}")));
                    }
                }
            }
            "synthesize" => {
                for s in 0..4 {
                    assert!(text.contains(&format!("CANARY_IMPROVED_{s}")));
                    assert!(!text.contains(&format!("CANARY_ANSWER_{s}")));
                }
                assert!(!text.contains("CANARY_REVIEW_"));
            }
            other => panic!("unexpected {other}"),
        }
    }

    // events are sequenced and end with run.completed; budget settled, nothing unresolved
    let mut seqs = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        seqs.push((ev.seq, ev.kind));
    }
    assert!(seqs.windows(2).all(|w| w[1].0 > w[0].0), "events are strictly increasing");
    assert_eq!(seqs.last().unwrap().1, "run.completed");
    assert_eq!(seqs.iter().filter(|(_, k)| k == "call.accepted").count(), 21);
    assert_eq!(seqs.iter().filter(|(_, k)| k == "stage.completed").count(), 4);
    let (settled, unresolved) = runs::budget_totals(&c, run).unwrap();
    assert!(settled > MicroUsd::ZERO);
    assert_eq!(unresolved, MicroUsd::ZERO);
    drop(c);
    let persisted = h.engine.ledger.events_since(run, 0, 500).unwrap();
    assert_eq!(persisted.len(), seqs.len(), "every broadcast event was committed first");
}

#[tokio::test(start_paused = true)]
async fn edited_system_prompts_reach_exactly_their_stage_and_are_frozen_in_the_run() {
    let h = harness(valid_decider());
    let mut i = set_seat_model(input(&h, 2), "gpt-5.6");
    i.system_prompts = rt_envelope::instructions::SystemPrompts { common: "CUSTOM_COMMON_RULE".into(), answer: "CUSTOM_ANSWER_RULE".into(), ..Default::default() };
    let run = h.engine.create_run(i).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed);
    let reqs = h.fake.recorded();
    assert_eq!(reqs.len(), 2 * 2 + 2 + 1);
    for r in &reqs {
        let text = r.body.to_string();
        assert!(text.contains("CUSTOM_COMMON_RULE"), "every call carries the edited common block");
        let is_answer = r.body["call"]["kind"] == "answer";
        assert_eq!(text.contains("CUSTOM_ANSWER_RULE"), is_answer, "the answer rule reaches answer calls only");
    }
    let c = h.engine.ledger.writer().unwrap();
    let row = runs::get_run(&c, run).unwrap();
    assert_eq!(row.panel_snapshot_json["system_prompts"]["answer"], "CUSTOM_ANSWER_RULE");
    assert!(row.panel_snapshot_json["instructions_version"].as_str().unwrap().starts_with("custom:"));
}

#[tokio::test]
async fn two_and_eight_seats_complete_with_the_right_call_counts() {
    for (n, expected) in [(2u8, 7usize), (8u8, 73usize)] {
        let h = harness(valid_decider());
        let run = h.engine.create_run(set_seat_model(input(&h, n), "gpt-5.6")).unwrap();
        assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed);
        assert_eq!(h.fake.recorded().len(), expected);
    }
}

#[tokio::test(start_paused = true)]
async fn invalid_output_gets_exactly_one_schema_retry_with_identical_request() {
    let h = harness(seat0_invalid_decider(1));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed);
    let reqs = h.fake.recorded();
    assert_eq!(reqs.len(), 8, "7 calls plus one schema retry");
    let seat0: Vec<&WireRequest> = reqs.iter().filter(|r| r.body["call"]["kind"] == "answer" && r.body["call"]["seat"] == 0).collect();
    assert_eq!(seat0.len(), 2);
    assert_eq!(seat0[0].digest(), seat0[1].digest(), "schema retry resends the identical envelope (ISO-10)");
    let c = h.engine.ledger.writer().unwrap();
    let retried = runs::list_all_calls(&c, run).unwrap().into_iter().find(|c| c.attempts_used == 2).unwrap();
    assert_eq!(retried.schema_retries_used, 1);
}

#[tokio::test(start_paused = true)]
async fn two_invalid_outputs_fail_the_call_and_block_the_run() {
    let h = harness(seat0_invalid_decider(2));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    let c = h.engine.ledger.writer().unwrap();
    let row = runs::get_run(&c, run).unwrap();
    assert_eq!(row.reason_code.as_deref(), Some("exhausted"));
    let calls = runs::list_all_calls(&c, run).unwrap();
    assert_eq!(calls.iter().filter(|c| c.state == CallState::Failed).count(), 1);
    assert_eq!(calls.iter().filter(|c| c.state == CallState::Accepted).count(), 1, "the other seat's answer is kept");
    assert_eq!(runs::list_papers(&c, run).unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn rate_limit_waits_and_retries_then_completes() {
    let h = harness(valid_decider());
    h.fake.push(Scripted::Fail(AttemptOutcome::RateLimited { retry_after_ms: Some(1500) }, ErrorCategory::RateLimit));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed);
    assert_eq!(h.fake.recorded().len(), 8);
    let c = h.engine.ledger.writer().unwrap();
    let (settled, unresolved) = runs::budget_totals(&c, run).unwrap();
    assert_eq!(unresolved, MicroUsd::ZERO);
    assert!(settled > MicroUsd::ZERO);
    let released: i64 = c.query_row("SELECT COUNT(*) FROM budget_entries WHERE run_id = ?1 AND kind = 'release'", [run.to_string()], |r| r.get(0)).unwrap();
    assert_eq!(released, 1, "the rate-limited attempt released its reservation");
}

#[tokio::test(start_paused = true)]
async fn refusal_and_auth_errors_never_retry() {
    let h = harness(valid_decider());
    h.fake.push(Scripted::Fail(AttemptOutcome::Refused, ErrorCategory::Refusal));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    assert_eq!(h.fake.recorded().len(), 2, "no retry after refusal");
    {
        let c = h.engine.ledger.writer().unwrap();
        assert_eq!(runs::get_run(&c, run).unwrap().reason_code.as_deref(), Some("refused"));
    }

    let h2 = harness(valid_decider());
    h2.fake.push(Scripted::Fail(AttemptOutcome::NonRetryable, ErrorCategory::Auth));
    let run2 = h2.engine.create_run(set_seat_model(input(&h2, 2), "gpt-5.6")).unwrap();
    assert_eq!(h2.engine.execute(run2, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    assert_eq!(h2.fake.recorded().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn a_lost_reply_is_retried_automatically_and_its_possible_charge_is_kept() {
    let h = harness(valid_decider());
    h.fake.push(Scripted::Fail(AttemptOutcome::Uncertain, ErrorCategory::Timeout));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed, "one hiccup no longer stops the run");
    let c = h.engine.ledger.writer().unwrap();
    let (_, unresolved) = runs::budget_totals(&c, run).unwrap();
    assert!(unresolved > MicroUsd::ZERO, "the possible charge of the lost reply stays on the books");
}

#[tokio::test(start_paused = true)]
async fn uncertain_outcome_blocks_keeps_liability_and_needs_manual_retry() {
    let h = harness(seat0_failing_decider(AttemptOutcome::Uncertain, ErrorCategory::Timeout, 3));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    let (uncertain_call, unresolved) = {
        let c = h.engine.ledger.writer().unwrap();
        assert_eq!(runs::get_run(&c, run).unwrap().reason_code.as_deref(), Some("uncertain"));
        let call = runs::list_all_calls(&c, run).unwrap().into_iter().find(|c| c.state == CallState::Uncertain).unwrap();
        let (_, unresolved) = runs::budget_totals(&c, run).unwrap();
        (call.id, unresolved)
    };
    assert!(unresolved > MicroUsd::ZERO, "the possible charge stays on the books");
    // resuming without a manual retry blocks again immediately, no new dispatch
    let before = h.fake.recorded().len();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    assert_eq!(h.fake.recorded().len(), before);
    // explicit retry, then the run completes
    h.engine.retry_uncertain(uncertain_call).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed);
}

#[tokio::test(start_paused = true)]
async fn cancel_stops_dispatch_and_keeps_committed_papers() {
    let h = harness(valid_decider());
    for _ in 0..4 {
        h.fake.push(Scripted::Hang);
    }
    let run = h.engine.create_run(set_seat_model(input(&h, 4), "gpt-5.6")).unwrap();
    let token = CancellationToken::new();
    let engine = h.engine.clone();
    let t = token.clone();
    let task = tokio::spawn(async move { engine.execute(run, t).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(50)).await;
    token.cancel();
    assert_eq!(task.await.unwrap(), RunState::Cancelled);
    {
        let c = h.engine.ledger.writer().unwrap();
        let row = runs::get_run(&c, run).unwrap();
        assert_eq!(row.state, RunState::Cancelled);
        assert!(row.finished_at.is_some());
    }
    assert!(h.engine.execute(run, CancellationToken::new()).await.is_ok(), "terminal runs are a no-op");
}

#[tokio::test(start_paused = true)]
async fn budget_ceiling_blocks_before_dispatch() {
    let h = harness(valid_decider());
    let mut i = set_seat_model(input(&h, 2), "gpt-5.6");
    i.ceiling = MicroUsd(10); // ten micro-dollars
    let run = h.engine.create_run(i).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    assert_eq!(h.fake.recorded().len(), 0, "nothing was sent");
    let c = h.engine.ledger.writer().unwrap();
    assert_eq!(runs::get_run(&c, run).unwrap().reason_code.as_deref(), Some("budget"));
}

#[tokio::test(start_paused = true)]
async fn interrupted_run_resumes_and_reuses_accepted_papers() {
    // reviews hang until we flip the switch, so we can abort the executor mid-stage like a crash
    let hang = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let valid = valid_decider();
    let flag = hang.clone();
    let decider: rt_adapters::fake::Decider = Arc::new(move |req: &WireRequest| {
        if req.body["call"]["kind"] == "review" && flag.load(std::sync::atomic::Ordering::SeqCst) { Scripted::Hang } else { valid(req) }
    });
    let h = harness(decider);
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    let engine = h.engine.clone();
    let task = tokio::spawn(async move { engine.execute(run, CancellationToken::new()).await });
    // let the answer stage finish and the review attempts get dispatched, then "crash"
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if h.fake.recorded().iter().filter(|r| r.body["call"]["kind"] == "review").count() == 2 {
            break;
        }
    }
    task.abort();
    let _ = task.await;
    assert_eq!(h.fake.recorded().iter().filter(|r| r.body["call"]["kind"] == "answer").count(), 2);

    // launch-time recovery: run interrupted, in-flight attempts uncertain
    let ids = h.engine.mark_interrupted_at_launch().unwrap();
    assert_eq!(ids, vec![run]);
    let uncertain: Vec<LogicalCallId> = {
        let c = h.engine.ledger.writer().unwrap();
        assert_eq!(runs::get_run(&c, run).unwrap().state, RunState::Interrupted);
        assert_eq!(runs::list_papers(&c, run).unwrap().iter().filter(|p| p.kind == "answer").count(), 2, "answers survived");
        runs::list_calls(&c, run, Stage::Review).unwrap().iter().filter(|c| c.state == CallState::Uncertain).map(|c| c.id).collect()
    };
    assert_eq!(uncertain.len(), 2);
    // resume without retrying blocks immediately and sends nothing
    let before = h.fake.recorded().len();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    assert_eq!(h.fake.recorded().len(), before);
    // user retries the uncertain calls; reviews now succeed; answers are never re-dispatched
    hang.store(false, std::sync::atomic::Ordering::SeqCst);
    for id in uncertain {
        h.engine.retry_uncertain(id).unwrap();
    }
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed);
    let reqs = h.fake.recorded();
    assert_eq!(reqs.iter().filter(|r| r.body["call"]["kind"] == "answer").count(), 2, "accepted answers reused");
    assert_eq!(reqs.iter().filter(|r| r.body["call"]["kind"] == "review").count(), 4, "two hung, two retried");
    assert_eq!(reqs.len(), 2 + 4 + 2 + 1);
}

#[tokio::test(start_paused = true)]
async fn late_response_after_cancel_cannot_advance_the_run() {
    let h = harness(valid_decider());
    h.fake.push(Scripted::Hang);
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    let token = CancellationToken::new();
    let engine = h.engine.clone();
    let t = token.clone();
    let task = tokio::spawn(async move { engine.execute(run, t).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(20)).await;
    token.cancel();
    assert_eq!(task.await.unwrap(), RunState::Cancelled);
    let c = h.engine.ledger.writer().unwrap();
    let calls = runs::list_all_calls(&c, run).unwrap();
    assert!(calls.iter().all(|c| c.state != CallState::Accepted || c.accepted_paper_id.is_some()));
    assert_eq!(runs::get_run(&c, run).unwrap().state, RunState::Cancelled);
}

#[test]
fn create_run_validates_inputs_without_dispatch() {
    let h = harness(valid_decider());
    let mut i = set_seat_model(input(&h, 2), "gpt-5.6");
    i.question = String::new();
    assert!(matches!(h.engine.create_run(i.clone()), Err(EngineError::Invalid(_))));
    i.question = "q".into();
    i.seats[1].credential_ref = CredentialRef::new();
    assert!(matches!(h.engine.create_run(i.clone()), Err(EngineError::Invalid(_))));
    i.seats[1].credential_ref = h.cred;
    i.seats[1].provider = "nobody".into();
    assert!(matches!(h.engine.create_run(i.clone()), Err(EngineError::UnknownProvider(_))));
    i.seats[1].provider = "openai".into();
    i.seats[1].model_id = "unpriced-model".into();
    assert!(matches!(h.engine.create_run(i), Err(EngineError::Budget(_))));
    assert_eq!(h.fake.recorded().len(), 0);
}

#[test]
fn launch_marks_active_runs_interrupted_and_unfinished_attempts_uncertain() {
    let h = harness(valid_decider());
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    {
        let c = h.engine.ledger.writer().unwrap();
        let call = runs::list_calls(&c, run, Stage::Answer).unwrap()[0].clone();
        runs::insert_attempt(&c, &AttemptRow { id: AttemptId::new(), logical_call_id: call.id, ordinal: 1, kind: "primary".into(), request_digest: "d".into(), serialized_body_redacted: json!({}) }).unwrap();
        runs::update_call(&c, call.id, CallState::InFlight, 1, 0, None).unwrap();
    }
    let ids = h.engine.mark_interrupted_at_launch().unwrap();
    assert_eq!(ids, vec![run]);
    let c = h.engine.ledger.writer().unwrap();
    assert_eq!(runs::get_run(&c, run).unwrap().state, RunState::Interrupted);
    let calls = runs::list_calls(&c, run, Stage::Answer).unwrap();
    assert_eq!(calls.iter().filter(|c| c.state == CallState::Uncertain).count(), 1);
    assert!(runs::unfinished_attempts(&c, run).unwrap().is_empty());
}

#[test]
fn delete_run_removes_everything_through_the_maintenance_path() {
    let h = harness(valid_decider());
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert!(h.engine.delete_run(run).unwrap());
    {
        let c = h.engine.ledger.writer().unwrap();
        assert!(matches!(runs::get_run(&c, run), Err(LedgerError::NotFound(_))));
        let n: i64 = c.query_row("SELECT COUNT(*) FROM logical_calls", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
    assert!(!h.engine.delete_run(run).unwrap());
}

#[test]
fn references_split_into_numbered_blocks() {
    assert_eq!(split_references(Some("a\n\n\nb\n\n")), vec![("Ref-1".into(), "a".into()), ("Ref-2".into(), "b".into())]);
    assert!(split_references(None).is_empty());
}

#[tokio::test(start_paused = true)]
async fn resume_gives_calls_that_ran_out_of_attempts_a_fresh_round() {
    let h = harness(seat0_failing_decider(AttemptOutcome::Truncated, ErrorCategory::Truncated, 3));
    let run = h.engine.create_run(set_seat_model(input(&h, 2), "gpt-5.6")).unwrap();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Blocked);
    let before = h.fake.recorded().len();
    assert_eq!(h.engine.execute(run, CancellationToken::new()).await.unwrap(), RunState::Completed, "resume retries the failed call");
    let reqs = h.fake.recorded();
    assert!(reqs.len() > before);
    let c = h.engine.ledger.writer().unwrap();
    let failed = runs::list_all_calls(&c, run).unwrap().into_iter().find(|c| c.attempts_used > 3).expect("the resumed call kept counting attempts");
    assert_eq!(failed.state, CallState::Accepted);
}
