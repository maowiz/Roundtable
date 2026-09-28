//! Paper schemas and the native validator (OUT-01…05).
//!
//! Structure is checked with JSON Schema; semantics (aliases, claim IDs, decisions,
//! references) are checked natively. Valid JSON alone is never success.

use rt_core::bounds::{MAX_CLAIMS, MAX_RESPONSE_BYTES};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub const ANSWER_V1: &str = include_str!("../../../contracts/papers/answer.v1.schema.json");
pub const REVIEW_V1: &str = include_str!("../../../contracts/papers/review.v1.schema.json");
pub const IMPROVED_V1: &str = include_str!("../../../contracts/papers/improved.v1.schema.json");
pub const FINAL_V1: &str = include_str!("../../../contracts/papers/final.v1.schema.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaperKind {
    Answer,
    Review,
    Improved,
    Final,
}

impl PaperKind {
    pub fn schema_id(self) -> &'static str {
        match self {
            PaperKind::Answer => "answer.v1",
            PaperKind::Review => "review.v1",
            PaperKind::Improved => "improved.v1",
            PaperKind::Final => "final.v1",
        }
    }
    pub fn schema_json(self) -> &'static str {
        match self {
            PaperKind::Answer => ANSWER_V1,
            PaperKind::Review => REVIEW_V1,
            PaperKind::Improved => IMPROVED_V1,
            PaperKind::Final => FINAL_V1,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("response exceeds {0} bytes")]
    TooLarge(usize),
    #[error("not JSON: {0}")]
    NotJson(String),
    #[error("schema violation: {0}")]
    Schema(String),
    #[error("semantic violation: {0}")]
    Semantic(String),
}

/// What the envelope allowed. The validator rejects anything outside it (OUT-02).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvelopeContext {
    /// Review: the single target alias. Improve: allowed review aliases. Final: candidate aliases.
    pub allowed_aliases: HashSet<String>,
    /// Review: claim IDs present in the target answer view.
    pub target_claim_ids: HashSet<String>,
    /// Improve: (review_alias, issue_id) pairs the seat must decide on.
    pub supplied_issues: HashSet<(String, String)>,
    /// Final: claim IDs per candidate alias.
    pub candidate_claims: HashMap<String, HashSet<String>>,
    /// Reference IDs supplied to the model (Ref-1, Ref-2, ...).
    pub reference_ids: HashSet<String>,
}

fn compiled(kind: PaperKind) -> &'static jsonschema::Validator {
    static CACHE: OnceLock<HashMap<&'static str, jsonschema::Validator>> = OnceLock::new();
    let map = CACHE.get_or_init(|| {
        let mut m = HashMap::new();
        for k in [PaperKind::Answer, PaperKind::Review, PaperKind::Improved, PaperKind::Final] {
            let schema: Value = serde_json::from_str(k.schema_json()).expect("bundled schema parses");
            let v = jsonschema::validator_for(&schema).expect("bundled schema compiles");
            m.insert(k.schema_id(), v);
        }
        m
    });
    &map[kind.schema_id()]
}

/// Parse and validate raw model output for a paper kind against its envelope context.
pub fn validate(kind: PaperKind, raw: &str, ctx: &EnvelopeContext) -> Result<Value, ValidationError> {
    if raw.len() > MAX_RESPONSE_BYTES {
        return Err(ValidationError::TooLarge(MAX_RESPONSE_BYTES));
    }
    let mut value: Value = serde_json::from_str(raw.trim()).map_err(|e| ValidationError::NotJson(e.to_string()))?;
    // Models sometimes echo the schema's own metadata keys; they carry no paper content.
    if let Some(obj) = value.as_object_mut() {
        for k in ["$schema", "$id", "title", "$comment"] {
            obj.remove(k);
        }
        // review.v1 once carried a per-issue severity; it was dropped from the contract (reviews are
        // unranked). A model trained on older prompts may still emit it, so it is discarded, not rejected.
        if kind == PaperKind::Review {
            if let Some(issues) = obj.get_mut("issues").and_then(Value::as_array_mut) {
                for i in issues.iter_mut() {
                    if let Some(o) = i.as_object_mut() {
                        o.remove("severity");
                    }
                }
            }
        }
    }
    // Models sometimes add a decision for a review point that was never shown to them, or repeat one.
    // Those entries refer to nothing, so they are dropped; every real review point must still be decided.
    if kind == PaperKind::Improved && !ctx.supplied_issues.is_empty() {
        if let Some(decisions) = value.get_mut("decisions").and_then(Value::as_array_mut) {
            let mut seen = HashSet::new();
            decisions.retain(|d| {
                let key = (d["review_alias"].as_str().unwrap_or_default().to_string(), d["issue_id"].as_str().unwrap_or_default().to_string());
                ctx.supplied_issues.contains(&key) && seen.insert(key)
            });
        }
    }
    validate_value(kind, &value, ctx)?;
    Ok(value)
}

/// The schema as models should see it in instructions: without metadata keys they might echo back.
pub fn schema_for_prompt(kind: PaperKind) -> Value {
    let mut v: Value = serde_json::from_str(kind.schema_json()).expect("bundled schema parses");
    if let Some(obj) = v.as_object_mut() {
        for k in ["$schema", "$id", "title"] {
            obj.remove(k);
        }
    }
    v
}

pub fn validate_value(kind: PaperKind, value: &Value, ctx: &EnvelopeContext) -> Result<(), ValidationError> {
    let v = compiled(kind);
    if let Err(err) = v.validate(value) {
        return Err(ValidationError::Schema(format!("{} at {}", err, err.instance_path)));
    }
    match kind {
        PaperKind::Answer => check_answer(value, ctx),
        PaperKind::Review => check_review(value, ctx),
        PaperKind::Improved => check_improved(value, ctx),
        PaperKind::Final => check_final(value, ctx),
    }
}

fn str_items<'a>(v: &'a Value, path: &str) -> Vec<&'a str> {
    v.get(path).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default()
}

fn unique_ids(items: &[&Value], field: &str, what: &str) -> Result<HashSet<String>, ValidationError> {
    let mut seen = HashSet::new();
    for it in items {
        let id = it.get(field).and_then(Value::as_str).unwrap_or_default().to_string();
        if !seen.insert(id.clone()) {
            return Err(ValidationError::Semantic(format!("duplicate {what} id {id}")));
        }
    }
    if seen.len() > MAX_CLAIMS {
        return Err(ValidationError::Semantic(format!("more than {MAX_CLAIMS} {what}s")));
    }
    Ok(seen)
}

fn check_references(claims: &[&Value], ctx: &EnvelopeContext) -> Result<(), ValidationError> {
    for c in claims {
        for r in str_items(c, "references") {
            if !ctx.reference_ids.contains(r) {
                return Err(ValidationError::Semantic(format!("unknown reference {r}")));
            }
        }
    }
    Ok(())
}

fn check_answer(v: &Value, ctx: &EnvelopeContext) -> Result<(), ValidationError> {
    let claims: Vec<&Value> = v["claims"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    unique_ids(&claims, "id", "claim")?;
    check_references(&claims, ctx)
}

fn check_review(v: &Value, ctx: &EnvelopeContext) -> Result<(), ValidationError> {
    let alias = v["target_alias"].as_str().unwrap_or_default();
    if !ctx.allowed_aliases.contains(alias) {
        return Err(ValidationError::Semantic(format!("target alias {alias} is not the allowed target")));
    }
    for id in str_items(v, "correct_claim_ids") {
        if !ctx.target_claim_ids.contains(id) {
            return Err(ValidationError::Semantic(format!("correct claim {id} does not exist in the target")));
        }
    }
    let issues: Vec<&Value> = v["issues"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    unique_ids(&issues, "id", "issue")?;
    for it in &issues {
        if let Some(t) = it.get("target_claim_id").and_then(Value::as_str) {
            if !ctx.target_claim_ids.contains(t) {
                return Err(ValidationError::Semantic(format!("issue targets unknown claim {t}")));
            }
        }
    }
    Ok(())
}

fn check_improved(v: &Value, ctx: &EnvelopeContext) -> Result<(), ValidationError> {
    let claims: Vec<&Value> = v["claims"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    unique_ids(&claims, "id", "claim")?;
    check_references(&claims, ctx)?;
    let mut decided = HashSet::new();
    for d in v["decisions"].as_array().map(|a| a.iter()).into_iter().flatten() {
        let key = (
            d["review_alias"].as_str().unwrap_or_default().to_string(),
            d["issue_id"].as_str().unwrap_or_default().to_string(),
        );
        if !ctx.supplied_issues.contains(&key) {
            return Err(ValidationError::Semantic(format!("decision for unknown issue {}/{}", key.0, key.1)));
        }
        if !decided.insert(key.clone()) {
            return Err(ValidationError::Semantic(format!("duplicate decision for {}/{}", key.0, key.1)));
        }
    }
    if decided.len() != ctx.supplied_issues.len() {
        let missing: Vec<String> = ctx.supplied_issues.difference(&decided).map(|(a, i)| format!("{a}/{i}")).collect();
        return Err(ValidationError::Semantic(format!("missing decisions for {}", missing.join(", "))));
    }
    Ok(())
}

fn check_final(v: &Value, ctx: &EnvelopeContext) -> Result<(), ValidationError> {
    for kc in v["key_claims"].as_array().map(|a| a.iter()).into_iter().flatten() {
        let support = kc["support"].as_str().unwrap_or_default();
        let supporting: Vec<&Value> = kc.get("supporting").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
        if support == "supported" && supporting.is_empty() {
            return Err(ValidationError::Semantic("a supported claim must cite at least one candidate".into()));
        }
        for s in supporting {
            let alias = s["candidate_alias"].as_str().unwrap_or_default();
            let Some(claim_ids) = ctx.candidate_claims.get(alias) else {
                return Err(ValidationError::Semantic(format!("unknown candidate alias {alias}")));
            };
            if let Some(cid) = s.get("claim_id").and_then(Value::as_str) {
                if !claim_ids.contains(cid) {
                    return Err(ValidationError::Semantic(format!("candidate {alias} has no claim {cid}")));
                }
            }
        }
        for r in str_items(kc, "references") {
            if !ctx.reference_ids.contains(r) {
                return Err(ValidationError::Semantic(format!("unknown reference {r}")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx_review() -> EnvelopeContext {
        EnvelopeContext {
            allowed_aliases: ["A-k3j9x2q1".to_string()].into(),
            target_claim_ids: ["C1".to_string(), "C2".to_string()].into(),
            ..Default::default()
        }
    }

    #[test]
    fn all_bundled_schemas_compile() {
        for k in [PaperKind::Answer, PaperKind::Review, PaperKind::Improved, PaperKind::Final] {
            let _ = compiled(k);
        }
    }

    #[test]
    fn answer_accepts_minimal_valid_paper_and_rejects_duplicates() {
        let ok = json!({"schema_version":"answer.v1","answer":"Start narrow.","assumptions":[],"claims":[{"id":"C1","text":"x"}],"limitations":[]});
        validate_value(PaperKind::Answer, &ok, &EnvelopeContext::default()).unwrap();
        let dup = json!({"schema_version":"answer.v1","answer":"a","assumptions":[],"claims":[{"id":"C1","text":"x"},{"id":"C1","text":"y"}],"limitations":[]});
        assert!(matches!(validate_value(PaperKind::Answer, &dup, &EnvelopeContext::default()), Err(ValidationError::Semantic(_))));
        let extra = json!({"schema_version":"answer.v1","answer":"a","assumptions":[],"claims":[{"id":"C1","text":"x"}],"limitations":[],"surprise":1});
        assert!(matches!(validate_value(PaperKind::Answer, &extra, &EnvelopeContext::default()), Err(ValidationError::Schema(_))));
    }

    #[test]
    fn answer_rejects_unknown_reference_ids() {
        let v = json!({"schema_version":"answer.v1","answer":"a","assumptions":[],"claims":[{"id":"C1","text":"x","references":["Ref-9"]}],"limitations":[]});
        let ctx = EnvelopeContext { reference_ids: ["Ref-1".to_string()].into(), ..Default::default() };
        assert!(matches!(validate_value(PaperKind::Answer, &v, &ctx), Err(ValidationError::Semantic(_))));
    }

    #[test]
    fn review_must_target_the_allowed_alias_and_may_find_nothing() {
        let clean = json!({"schema_version":"review.v1","target_alias":"A-k3j9x2q1","correct_claim_ids":["C1","C2"],"issues":[],"missing":[],"needs_external_verification":false});
        validate_value(PaperKind::Review, &clean, &ctx_review()).unwrap();
        let wrong = json!({"schema_version":"review.v1","target_alias":"A-zzzzzzzz","correct_claim_ids":[],"issues":[],"missing":[],"needs_external_verification":false});
        assert!(matches!(validate_value(PaperKind::Review, &wrong, &ctx_review()), Err(ValidationError::Semantic(_))));
        let bad_claim = json!({"schema_version":"review.v1","target_alias":"A-k3j9x2q1","correct_claim_ids":[],"issues":[{"id":"I1","target_claim_id":"C7","explanation":"x"}],"missing":[],"needs_external_verification":true});
        assert!(matches!(validate_value(PaperKind::Review, &bad_claim, &ctx_review()), Err(ValidationError::Semantic(_))));
    }

    #[test]
    fn improved_drops_decisions_for_points_it_was_never_shown_but_still_needs_every_real_one() {
        let ctx = EnvelopeContext { supplied_issues: [("R-abc123".to_string(), "I1".to_string())].into(), ..Default::default() };
        let raw = r#"{"schema_version":"improved.v1","decisions":[{"review_alias":"R-abc123","issue_id":"I1","decision":"accepted","reason":"ok"},{"review_alias":"R-zzz999","issue_id":"I1","decision":"accepted","reason":"invented"},{"review_alias":"R-abc123","issue_id":"I1","decision":"rejected","reason":"repeat"}],"answer":"a","claims":[{"id":"C1","text":"c"}],"unresolved":[]}"#;
        let v = validate(PaperKind::Improved, raw, &ctx).unwrap();
        assert_eq!(v["decisions"].as_array().unwrap().len(), 1, "invented and repeated decisions are dropped");
        assert_eq!(v["decisions"][0]["decision"], "accepted", "the first real decision is kept");
        let missing = r#"{"schema_version":"improved.v1","decisions":[{"review_alias":"R-zzz999","issue_id":"I1","decision":"accepted","reason":"invented"}],"answer":"a","claims":[{"id":"C1","text":"c"}],"unresolved":[]}"#;
        assert!(matches!(validate(PaperKind::Improved, missing, &ctx), Err(ValidationError::Semantic(_))), "a real point left undecided still fails");
    }

    #[test]
    fn review_drops_a_legacy_severity_rating_instead_of_rejecting_it() {
        let raw = r#"{"schema_version":"review.v1","target_alias":"A-k3j9x2q1","correct_claim_ids":["C1"],"issues":[{"id":"I1","target_claim_id":"C1","severity":"high","explanation":"x"}],"missing":[],"needs_external_verification":false}"#;
        let v = validate(PaperKind::Review, raw, &ctx_review()).unwrap();
        assert!(v["issues"][0].get("severity").is_none(), "issues are unranked; a stray rating never reaches the ledger");
        assert_eq!(v["issues"][0]["explanation"], "x");
        // the prompt schema no longer offers the field at all
        let schema = schema_for_prompt(PaperKind::Review);
        assert!(schema["properties"]["issues"]["items"]["properties"].get("severity").is_none());
    }

    #[test]
    fn improved_must_decide_every_supplied_issue_exactly_once() {
        let ctx = EnvelopeContext {
            supplied_issues: [("R-abc123".to_string(), "I1".to_string()), ("R-def456".to_string(), "I1".to_string())].into(),
            ..Default::default()
        };
        let full = json!({"schema_version":"improved.v1","decisions":[
            {"review_alias":"R-abc123","issue_id":"I1","decision":"accepted","reason":"fair"},
            {"review_alias":"R-def456","issue_id":"I1","decision":"rejected","reason":"not applicable"}],
            "answer":"better","claims":[{"id":"C1","text":"x"}],"unresolved":[]});
        validate_value(PaperKind::Improved, &full, &ctx).unwrap();
        let missing = json!({"schema_version":"improved.v1","decisions":[
            {"review_alias":"R-abc123","issue_id":"I1","decision":"accepted","reason":"fair"}],
            "answer":"better","claims":[{"id":"C1","text":"x"}],"unresolved":[]});
        assert!(matches!(validate_value(PaperKind::Improved, &missing, &ctx), Err(ValidationError::Semantic(_))));
        let foreign = json!({"schema_version":"improved.v1","decisions":[
            {"review_alias":"R-abc123","issue_id":"I1","decision":"accepted","reason":"fair"},
            {"review_alias":"R-def456","issue_id":"I1","decision":"rejected","reason":"n"},
            {"review_alias":"R-ghi789","issue_id":"I1","decision":"rejected","reason":"n"}],
            "answer":"better","claims":[{"id":"C1","text":"x"}],"unresolved":[]});
        assert!(matches!(validate_value(PaperKind::Improved, &foreign, &ctx), Err(ValidationError::Semantic(_))));
    }

    #[test]
    fn final_supported_claims_need_real_candidates() {
        let ctx = EnvelopeContext {
            candidate_claims: [("C-cand01".to_string(), ["C1".to_string()].into())].into(),
            ..Default::default()
        };
        let ok = json!({"schema_version":"final.v1","answer":"a","key_claims":[
            {"text":"t","support":"supported","supporting":[{"candidate_alias":"C-cand01","claim_id":"C1"}]},
            {"text":"u","support":"unsupported"}],
            "unresolved_conflicts":[],"verification_limitations":[]});
        validate_value(PaperKind::Final, &ok, &ctx).unwrap();
        let no_support = json!({"schema_version":"final.v1","answer":"a","key_claims":[{"text":"t","support":"supported"}],"unresolved_conflicts":[],"verification_limitations":[]});
        assert!(matches!(validate_value(PaperKind::Final, &no_support, &ctx), Err(ValidationError::Semantic(_))));
        let bad_alias = json!({"schema_version":"final.v1","answer":"a","key_claims":[{"text":"t","support":"supported","supporting":[{"candidate_alias":"C-nope00"}]}],"unresolved_conflicts":[],"verification_limitations":[]});
        assert!(matches!(validate_value(PaperKind::Final, &bad_alias, &ctx), Err(ValidationError::Semantic(_))));
    }

    #[test]
    fn echoed_schema_metadata_keys_are_ignored() {
        let raw = r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"x","title":"t","schema_version":"answer.v1","answer":"a","assumptions":[],"claims":[{"id":"C1","text":"x"}],"limitations":[]}"#;
        let v = validate(PaperKind::Answer, raw, &EnvelopeContext::default()).unwrap();
        assert!(v.get("$schema").is_none());
        assert!(schema_for_prompt(PaperKind::Review).get("$id").is_none());
    }

    #[test]
    fn oversized_and_non_json_are_rejected_before_parsing() {
        let big = "x".repeat(MAX_RESPONSE_BYTES + 1);
        assert!(matches!(validate(PaperKind::Answer, &big, &EnvelopeContext::default()), Err(ValidationError::TooLarge(_))));
        assert!(matches!(validate(PaperKind::Answer, "not json", &EnvelopeContext::default()), Err(ValidationError::NotJson(_))));
    }
}
