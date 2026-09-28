//! Envelope policy (backend plan section 5).
//!
//! The only code that turns ledger content into model input. It derives the allowed
//! papers from the run by role; callers cannot pass arbitrary paper IDs. Every envelope
//! is deterministic for a given run seed, so automatic retries produce an identical digest.

pub mod anonymize;
pub mod instructions;

use rand::seq::SliceRandom;
use rand::SeedableRng;
use rt_core::{canonical_json, digest, FrozenSeat, PaperId, Role, RunId, Stage, ViewId};
use rt_schemas::EnvelopeContext;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnvelopeError {
    #[error("paper {0} belongs to another run")]
    ForeignRun(PaperId),
    #[error("seat {0} is out of range")]
    SeatOutOfRange(u8),
    #[error("a seat cannot review its own answer")]
    SelfReview,
    #[error("missing {kind} paper for seat {seat}")]
    MissingPaper { kind: &'static str, seat: u8 },
    #[error("missing review of seat {target} by seat {reviewer}")]
    MissingReview { reviewer: u8, target: u8 },
    #[error("paper {0} has the wrong kind for this stage")]
    WrongKind(PaperId),
    #[error("paper {0} is malformed: {1}")]
    Malformed(PaperId, String),
}

/// An accepted paper as the engine hands it to the builder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paper {
    pub id: PaperId,
    pub run_id: RunId,
    pub kind: PaperKind,
    pub author_seat: u8,
    /// For reviews: the seat whose answer was reviewed.
    pub target_seat: Option<u8>,
    pub body: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperKind {
    Answer,
    Review,
    Improved,
    Final,
}

/// Everything about the run the builder may use. Frozen at run start.
#[derive(Debug, Clone)]
pub struct RunContext<'a> {
    pub run_id: RunId,
    pub seed: &'a str,
    pub question: &'a str,
    /// Reference text already split into (id, text) pairs, e.g. ("Ref-1", "...").
    pub references: &'a [(String, String)],
    pub seats: &'a [FrozenSeat],
    pub synthesizer_prompt: &'a str,
    /// The shared system prompts frozen for this run.
    pub system: &'a instructions::SystemPrompts,
}

/// The logical call an envelope is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CallSpec {
    Answer { seat: u8 },
    Review { reviewer: u8, target: u8 },
    Improve { seat: u8 },
    Synthesize,
}

impl CallSpec {
    pub fn stage(self) -> Stage {
        match self {
            CallSpec::Answer { .. } => Stage::Answer,
            CallSpec::Review { .. } => Stage::Review,
            CallSpec::Improve { .. } => Stage::Improve,
            CallSpec::Synthesize => Stage::Synthesize,
        }
    }
    pub fn role(self) -> Role {
        match self {
            CallSpec::Answer { .. } => Role::Answer,
            CallSpec::Review { .. } => Role::Review,
            CallSpec::Improve { .. } => Role::Improve,
            CallSpec::Synthesize => Role::Synthesize,
        }
    }
    /// Which seat's prompt and settings apply. Synthesis uses the synthesizer prompt.
    pub fn seat(self) -> Option<u8> {
        match self {
            CallSpec::Answer { seat } | CallSpec::Improve { seat } => Some(seat),
            CallSpec::Review { reviewer, .. } => Some(reviewer),
            CallSpec::Synthesize => None,
        }
    }
}

/// Papers available to the builder. The builder selects from these by role; it never
/// trusts the caller to pass "the right ones".
#[derive(Debug, Default, Clone)]
pub struct Papers {
    pub answers: HashMap<u8, Paper>,
    /// key: (reviewer, target)
    pub reviews: HashMap<(u8, u8), Paper>,
    pub improved: HashMap<u8, Paper>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// An anonymous view produced for this envelope. The ledger stores it (artifact_views).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub id: ViewId,
    pub paper_id: PaperId,
    pub alias: String,
    pub transform_version: &'static str,
    pub text: String,
    pub digest: String,
    pub possibly_identifiable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub call: CallSpec,
    pub stage: Stage,
    pub instructions: String,
    pub messages: Vec<Message>,
    pub schema_id: String,
    pub allowed_view_ids: Vec<ViewId>,
    /// alias -> paper id, plus the order in which views were presented.
    pub alias_map: BTreeMap<String, PaperId>,
    pub presentation_order: Vec<String>,
    pub digest: String,
    #[serde(skip)]
    pub views: Vec<View>,
    #[serde(skip)]
    pub validation: EnvelopeContext,
}

pub const TRANSFORM_VERSION: &str = "anon.v1";

/// Deterministic alias: prefix + 8 base36 chars from SHA-256(seed, call, paper).
pub fn alias(prefix: &str, seed: &str, call: &CallSpec, paper: PaperId) -> String {
    let material = format!("{seed}|{}|{paper}", serde_json::to_string(call).unwrap());
    let h = digest(material.as_bytes());
    let mut out = String::with_capacity(10);
    out.push_str(prefix);
    out.push('-');
    // map hex pairs to base36 alphanumerics
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    for i in 0..8 {
        let byte = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap();
        out.push(ALPHA[(byte as usize) % ALPHA.len()] as char);
    }
    out
}

fn rng_for(seed: &str, call: &CallSpec) -> rand_chacha::ChaCha20Rng {
    let material = format!("{seed}|shuffle|{}", serde_json::to_string(call).unwrap());
    let h = digest(material.as_bytes());
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap();
    }
    rand_chacha::ChaCha20Rng::from_seed(key)
}

fn check_paper(p: &Paper, run: RunId, kind: PaperKind) -> Result<(), EnvelopeError> {
    if p.run_id != run {
        return Err(EnvelopeError::ForeignRun(p.id));
    }
    if p.kind != kind {
        return Err(EnvelopeError::WrongKind(p.id));
    }
    Ok(())
}

fn claim_ids(p: &Paper) -> Result<HashSet<String>, EnvelopeError> {
    p.body["claims"]
        .as_array()
        .ok_or_else(|| EnvelopeError::Malformed(p.id, "claims missing".into()))?
        .iter()
        .map(|c| c["id"].as_str().map(str::to_string).ok_or_else(|| EnvelopeError::Malformed(p.id, "claim id missing".into())))
        .collect()
}

fn issue_ids(p: &Paper) -> Result<Vec<String>, EnvelopeError> {
    p.body["issues"]
        .as_array()
        .ok_or_else(|| EnvelopeError::Malformed(p.id, "issues missing".into()))?
        .iter()
        .map(|i| i["id"].as_str().map(str::to_string).ok_or_else(|| EnvelopeError::Malformed(p.id, "issue id missing".into())))
        .collect()
}

fn task_block(ctx: &RunContext<'_>) -> String {
    let mut s = String::new();
    s.push_str("### TASK\n");
    s.push_str(ctx.question.trim());
    s.push('\n');
    if !ctx.references.is_empty() {
        s.push_str("\n### REFERENCE MATERIAL (supplied by the user; cite by id)\n");
        for (id, text) in ctx.references {
            s.push_str(&format!("[{id}]\n{}\n\n", text.trim()));
        }
    } else {
        s.push_str("\n(No reference material was supplied. Leave every references list empty; do not invent Ref- ids.)\n");
    }
    s
}

fn make_view(ctx: &RunContext<'_>, call: &CallSpec, prefix: &str, paper: &Paper, body_text: String) -> View {
    let a = alias(prefix, ctx.seed, call, paper.id);
    let (text, flagged) = anonymize::suppress_identity(&body_text, ctx.seats);
    let d = digest(text.as_bytes());
    View { id: ViewId::new(), paper_id: paper.id, alias: a, transform_version: TRANSFORM_VERSION, text, digest: d, possibly_identifiable: flagged }
}

/// Build the envelope for one logical call. Pure and deterministic apart from fresh ViewIds.
pub fn build(ctx: &RunContext<'_>, call: CallSpec, papers: &Papers) -> Result<Envelope, EnvelopeError> {
    let n = ctx.seats.len() as u8;
    let seat_ok = |s: u8| if s < n { Ok(()) } else { Err(EnvelopeError::SeatOutOfRange(s)) };
    let stage = call.stage();
    let role = call.role();
    let ref_ids: HashSet<String> = ctx.references.iter().map(|(id, _)| id.clone()).collect();

    let (role_prompt, settings_seat) = match call.seat() {
        Some(s) => {
            seat_ok(s)?;
            (ctx.seats[s as usize].prompt_for(role).to_string(), Some(s))
        }
        None => (ctx.synthesizer_prompt.to_string(), None),
    };
    let _ = settings_seat;

    let instructions = instructions::compose(ctx.system, stage, &role_prompt);

    let mut messages = vec![Message { role: "user".into(), content: task_block(ctx) }];
    let mut views: Vec<View> = Vec::new();
    let mut validation = EnvelopeContext { reference_ids: ref_ids, ..Default::default() };
    let mut presentation_order = Vec::new();

    match call {
        CallSpec::Answer { .. } => {}
        CallSpec::Review { reviewer, target } => {
            seat_ok(target)?;
            if reviewer == target {
                return Err(EnvelopeError::SelfReview);
            }
            let p = papers.answers.get(&target).ok_or(EnvelopeError::MissingPaper { kind: "answer", seat: target })?;
            check_paper(p, ctx.run_id, PaperKind::Answer)?;
            let text = anonymize::render_answer(&p.body);
            let v = make_view(ctx, &call, "A", p, text);
            messages.push(Message { role: "user".into(), content: format!("### ANSWER TO REVIEW [{}]\n{}", v.alias, v.text) });
            validation.allowed_aliases.insert(v.alias.clone());
            validation.target_claim_ids = claim_ids(p)?;
            presentation_order.push(v.alias.clone());
            views.push(v);
        }
        CallSpec::Improve { seat } => {
            let own = papers.answers.get(&seat).ok_or(EnvelopeError::MissingPaper { kind: "answer", seat })?;
            check_paper(own, ctx.run_id, PaperKind::Answer)?;
            messages.push(Message { role: "user".into(), content: format!("### YOUR ORIGINAL ANSWER\n{}", anonymize::render_answer(&own.body)) });
            // reviews about this seat, one per other seat, shuffled
            let mut incoming: Vec<&Paper> = Vec::new();
            for r in 0..n {
                if r == seat {
                    continue;
                }
                let p = papers.reviews.get(&(r, seat)).ok_or(EnvelopeError::MissingReview { reviewer: r, target: seat })?;
                check_paper(p, ctx.run_id, PaperKind::Review)?;
                if p.author_seat != r || p.target_seat != Some(seat) {
                    return Err(EnvelopeError::WrongKind(p.id));
                }
                incoming.push(p);
            }
            incoming.shuffle(&mut rng_for(ctx.seed, &call));
            for p in incoming {
                let v = make_view(ctx, &call, "R", p, anonymize::render_review(&p.body));
                for issue in issue_ids(p)? {
                    validation.supplied_issues.insert((v.alias.clone(), issue));
                }
                validation.allowed_aliases.insert(v.alias.clone());
                messages.push(Message { role: "user".into(), content: format!("### REVIEW OF YOUR ANSWER [{}]\n{}", v.alias, v.text) });
                presentation_order.push(v.alias.clone());
                views.push(v);
            }
        }
        CallSpec::Synthesize => {
            let mut candidates: Vec<&Paper> = Vec::new();
            for s in 0..n {
                let p = papers.improved.get(&s).ok_or(EnvelopeError::MissingPaper { kind: "improved", seat: s })?;
                check_paper(p, ctx.run_id, PaperKind::Improved)?;
                candidates.push(p);
            }
            candidates.shuffle(&mut rng_for(ctx.seed, &call));
            for p in candidates {
                let v = make_view(ctx, &call, "C", p, anonymize::render_improved_for_synthesis(&p.body));
                validation.candidate_claims.insert(v.alias.clone(), claim_ids(p)?);
                validation.allowed_aliases.insert(v.alias.clone());
                messages.push(Message { role: "user".into(), content: format!("### CANDIDATE ANSWER [{}]\n{}", v.alias, v.text) });
                presentation_order.push(v.alias.clone());
                views.push(v);
            }
        }
    }

    let schema_id = match stage {
        Stage::Answer => "answer.v1",
        Stage::Review => "review.v1",
        Stage::Improve => "improved.v1",
        Stage::Synthesize => "final.v1",
    }
    .to_string();

    let alias_map: BTreeMap<String, PaperId> = views.iter().map(|v| (v.alias.clone(), v.paper_id)).collect();
    let digest_input = serde_json::json!({
        "call": call,
        "instructions": instructions,
        "messages": messages,
        "schema_id": schema_id,
        "views": views.iter().map(|v| &v.digest).collect::<Vec<_>>(),
    });
    let d = digest(canonical_json(&digest_input).as_bytes());

    Ok(Envelope {
        call,
        stage,
        instructions,
        messages,
        schema_id,
        allowed_view_ids: views.iter().map(|v| v.id).collect(),
        alias_map,
        presentation_order,
        digest: d,
        views,
        validation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rt_core::{CredentialRef, MicroUsd, RoleSettings, SeatLimits};
    use serde_json::json;

    fn seat(i: u8, label: &str, provider: &str, model: &str) -> FrozenSeat {
        let rs = RoleSettings { temperature: Some(0.5), max_output_tokens: 1000, reasoning_effort: None, web_search: false };
        FrozenSeat {
            seat_index: i,
            label: label.into(),
            provider: provider.into(),
            credential_ref: CredentialRef::new(),
            model_id: model.into(),
            model_is_alias: false,
            answer: rs.clone(),
            review: rs.clone(),
            improve: rs,
            prompt_answer: format!("answer prompt {i}"),
            prompt_review: format!("review prompt {i}"),
            prompt_improve: format!("improve prompt {i}"),
            limits: SeatLimits { cost_cap: MicroUsd(400_000) },
        }
    }

    pub(crate) fn seats(n: u8) -> Vec<FrozenSeat> {
        let names = [("Claude", "Anthropic", "claude-x"), ("GPT", "OpenAI", "gpt-x"), ("Gemini", "Google", "gemini-x"), ("Grok", "xAI", "grok-x"), ("DeepSeek", "DeepSeek", "deepseek-x"), ("Mistral", "Mistral", "mistral-x"), ("Llama", "Groq", "llama-x"), ("Qwen", "OpenRouter", "qwen-x")];
        (0..n).map(|i| seat(i, names[i as usize].0, names[i as usize].1, names[i as usize].2)).collect()
    }

    /// Canary: every paper body carries a unique token naming its author and kind.
    pub(crate) fn papers(run: RunId, n: u8) -> Papers {
        let mut p = Papers::default();
        for s in 0..n {
            p.answers.insert(s, Paper { id: PaperId::new(), run_id: run, kind: PaperKind::Answer, author_seat: s, target_seat: None, body: json!({"schema_version":"answer.v1","answer":format!("CANARY_ANSWER_{s}"),"assumptions":[],"claims":[{"id":"C1","text":format!("claim CANARY_ANSWER_{s}")}],"limitations":[]}) });
            p.improved.insert(s, Paper { id: PaperId::new(), run_id: run, kind: PaperKind::Improved, author_seat: s, target_seat: None, body: json!({"schema_version":"improved.v1","decisions":[{"review_alias":"R-secret01","issue_id":"I1","decision":"accepted","reason":format!("CANARY_DECISION_{s}")}],"answer":format!("CANARY_IMPROVED_{s}"),"claims":[{"id":"C1","text":"c"}],"unresolved":[format!("open CANARY_UNRESOLVED_{s}")]}) });
            for r in 0..n {
                if r != s {
                    p.reviews.insert((r, s), Paper { id: PaperId::new(), run_id: run, kind: PaperKind::Review, author_seat: r, target_seat: Some(s), body: json!({"schema_version":"review.v1","target_alias":"A-whatever","correct_claim_ids":[],"issues":[{"id":"I1","target_claim_id":"C1","explanation":format!("CANARY_REVIEW_{r}_OF_{s}")}],"missing":[],"needs_external_verification":false}) });
                }
            }
        }
        p
    }

    fn all_text(e: &Envelope) -> String {
        let mut s = e.instructions.clone();
        for m in &e.messages {
            s.push('\n');
            s.push_str(&m.content);
        }
        s
    }

    #[test]
    fn review_sees_exactly_one_foreign_answer_and_nothing_else() {
        for n in 2..=8u8 {
            let run = RunId::new();
            let seats = seats(n);
            let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &[], seats: &seats, synthesizer_prompt: "synth", system: &instructions::SystemPrompts::default() };
            let papers = papers(run, n);
            for reviewer in 0..n {
                for target in 0..n {
                    if reviewer == target {
                        assert_eq!(build(&ctx, CallSpec::Review { reviewer, target }, &papers).unwrap_err(), EnvelopeError::SelfReview);
                        continue;
                    }
                    let e = build(&ctx, CallSpec::Review { reviewer, target }, &papers).unwrap();
                    let text = all_text(&e);
                    assert!(text.contains(&format!("CANARY_ANSWER_{target}")));
                    for other in 0..n {
                        if other != target {
                            assert!(!text.contains(&format!("CANARY_ANSWER_{other}")), "n={n} reviewer {reviewer} saw answer {other}");
                        }
                    }
                    assert!(!text.contains("CANARY_REVIEW_"), "reviews never leak into reviews");
                    assert!(!text.contains("CANARY_IMPROVED_"));
                    assert_eq!(e.views.len(), 1);
                    assert_eq!(e.validation.allowed_aliases.len(), 1);
                    assert!(text.contains(&format!("review prompt {reviewer}")));
                    assert!(!text.contains(&format!("answer prompt {reviewer}")));
                }
            }
        }
    }

    #[test]
    fn improve_sees_own_answer_and_only_reviews_about_it() {
        for n in 2..=8u8 {
            let run = RunId::new();
            let seats = seats(n);
            let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &[], seats: &seats, synthesizer_prompt: "synth", system: &instructions::SystemPrompts::default() };
            let papers = papers(run, n);
            for seat in 0..n {
                let e = build(&ctx, CallSpec::Improve { seat }, &papers).unwrap();
                let text = all_text(&e);
                assert!(text.contains(&format!("CANARY_ANSWER_{seat}")));
                for other in 0..n {
                    if other != seat {
                        assert!(!text.contains(&format!("CANARY_ANSWER_{other}")), "other answers leak");
                        assert!(text.contains(&format!("CANARY_REVIEW_{other}_OF_{seat}")), "missing incoming review");
                        for t in 0..n {
                            if t != seat {
                                assert!(!text.contains(&format!("CANARY_REVIEW_{seat}_OF_{t}")), "reviews the seat wrote leaked");
                                assert!(!text.contains(&format!("CANARY_REVIEW_{other}_OF_{t}")), "reviews about others leaked");
                            }
                        }
                    }
                }
                assert_eq!(e.views.len(), (n - 1) as usize);
                assert_eq!(e.validation.supplied_issues.len(), (n - 1) as usize);
            }
        }
    }

    #[test]
    fn synthesis_sees_improved_answers_only_without_decision_audit() {
        for n in 2..=8u8 {
            let run = RunId::new();
            let seats = seats(n);
            let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &[], seats: &seats, synthesizer_prompt: "SYNTH PROMPT", system: &instructions::SystemPrompts::default() };
            let papers = papers(run, n);
            let e = build(&ctx, CallSpec::Synthesize, &papers).unwrap();
            let text = all_text(&e);
            for s in 0..n {
                assert!(text.contains(&format!("CANARY_IMPROVED_{s}")));
                assert!(text.contains(&format!("CANARY_UNRESOLVED_{s}")), "unresolved issues are included");
                assert!(!text.contains(&format!("CANARY_ANSWER_{s}")), "original answers leak");
                assert!(!text.contains(&format!("CANARY_DECISION_{s}")), "decision audit leaks");
            }
            assert!(!text.contains("CANARY_REVIEW_"));
            assert!(text.contains("SYNTH PROMPT"));
            assert_eq!(e.validation.candidate_claims.len(), n as usize);
        }
    }

    #[test]
    fn identities_are_suppressed_in_views() {
        let run = RunId::new();
        let seats = seats(4);
        let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &[], seats: &seats, synthesizer_prompt: "s", system: &instructions::SystemPrompts::default() };
        let mut papers = papers(run, 4);
        papers.answers.get_mut(&1).unwrap().body["answer"] = json!("As GPT from OpenAI, I think gpt-x is right. Claude disagrees.");
        let e = build(&ctx, CallSpec::Review { reviewer: 0, target: 1 }, &papers).unwrap();
        let text = all_text(&e);
        for word in ["GPT", "OpenAI", "gpt-x", "Claude"] {
            assert!(!text.contains(word), "{word} leaked");
        }
        assert!(text.contains("another model"));
    }

    #[test]
    fn digest_is_stable_across_rebuilds_and_changes_with_content() {
        let run = RunId::new();
        let seats = seats(3);
        let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &[], seats: &seats, synthesizer_prompt: "s", system: &instructions::SystemPrompts::default() };
        let papers = papers(run, 3);
        let a = build(&ctx, CallSpec::Improve { seat: 1 }, &papers).unwrap();
        let b = build(&ctx, CallSpec::Improve { seat: 1 }, &papers).unwrap();
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.presentation_order, b.presentation_order, "shuffle is seeded");
        let ctx2 = RunContext { seed: "other", ..ctx };
        let c = build(&ctx2, CallSpec::Improve { seat: 1 }, &papers).unwrap();
        assert_ne!(a.digest, c.digest);
    }

    #[test]
    fn foreign_run_and_missing_papers_are_rejected() {
        let run = RunId::new();
        let seats = seats(2);
        let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &[], seats: &seats, synthesizer_prompt: "s", system: &instructions::SystemPrompts::default() };
        let mut papers = papers(run, 2);
        let foreign = papers.answers.get_mut(&1).unwrap();
        foreign.run_id = RunId::new();
        let fid = foreign.id;
        assert_eq!(build(&ctx, CallSpec::Review { reviewer: 0, target: 1 }, &papers).unwrap_err(), EnvelopeError::ForeignRun(fid));
        let empty = Papers::default();
        assert_eq!(build(&ctx, CallSpec::Improve { seat: 0 }, &empty).unwrap_err(), EnvelopeError::MissingPaper { kind: "answer", seat: 0 });
        assert_eq!(build(&ctx, CallSpec::Review { reviewer: 0, target: 5 }, &papers).unwrap_err(), EnvelopeError::SeatOutOfRange(5));
    }

    #[test]
    fn references_are_passed_through_and_validated() {
        let run = RunId::new();
        let seats = seats(2);
        let refs = vec![("Ref-1".to_string(), "Some source".to_string())];
        let ctx = RunContext { run_id: run, seed: "seed", question: "Q?", references: &refs, seats: &seats, synthesizer_prompt: "s", system: &instructions::SystemPrompts::default() };
        let e = build(&ctx, CallSpec::Answer { seat: 0 }, &Papers::default()).unwrap();
        assert!(e.messages[0].content.contains("[Ref-1]"));
        assert!(e.validation.reference_ids.contains("Ref-1"));
        assert_eq!(e.schema_id, "answer.v1");
    }

    #[test]
    fn aliases_match_schema_patterns() {
        let a = alias("A", "seed", &CallSpec::Review { reviewer: 0, target: 1 }, PaperId::new());
        assert!(a.starts_with("A-") && a.len() == 10 && a[2..].chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
