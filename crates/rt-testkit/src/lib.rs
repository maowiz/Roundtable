//! Test helpers shared by engine, adapter and envelope tests: canaries, a controllable clock,
//! and sample paper bodies. Fake providers arrive with the adapters crate.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// A unique marker planted in a paper so leak tests can search serialized requests for it.
pub fn canary(kind: &str, seat: u8, extra: &str) -> String {
    format!("CANARY_{}_{}{}", kind.to_uppercase(), seat, if extra.is_empty() { String::new() } else { format!("_{extra}") })
}

/// Assert that `haystack` contains every allowed canary and none of the forbidden ones.
pub fn assert_canaries(haystack: &str, allowed: &[String], forbidden: &[String]) {
    for a in allowed {
        assert!(haystack.contains(a.as_str()), "expected canary {a} is missing");
    }
    for f in forbidden {
        assert!(!haystack.contains(f.as_str()), "forbidden canary {f} leaked");
    }
}

/// Monotonic fake clock in milliseconds, shared across tasks.
#[derive(Clone, Default)]
pub struct FakeClock(Arc<AtomicU64>);

impl FakeClock {
    pub fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
    pub fn advance_ms(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

/// Minimal valid paper bodies for tests. Text fields carry the given canary.
pub mod bodies {
    use serde_json::{json, Value};

    pub fn answer(canary: &str) -> Value {
        json!({"schema_version":"answer.v1","answer":canary,"assumptions":[],"claims":[{"id":"C1","text":format!("claim {canary}")}],"limitations":[]})
    }
    pub fn review(target_alias: &str, canary: &str) -> Value {
        json!({"schema_version":"review.v1","target_alias":target_alias,"correct_claim_ids":["C1"],"issues":[{"id":"I1","target_claim_id":"C1","explanation":canary}],"missing":[],"needs_external_verification":false})
    }
    pub fn improved(decisions: &[(String, String)], canary: &str) -> Value {
        let d: Vec<Value> = decisions.iter().map(|(alias, issue)| json!({"review_alias":alias,"issue_id":issue,"decision":"accepted","reason":"ok"})).collect();
        json!({"schema_version":"improved.v1","decisions":d,"answer":canary,"claims":[{"id":"C1","text":"c"}],"unresolved":[]})
    }
    pub fn final_paper(candidate_alias: &str, canary: &str) -> Value {
        json!({"schema_version":"final.v1","answer":canary,"key_claims":[{"text":"k","support":"supported","supporting":[{"candidate_alias":candidate_alias,"claim_id":"C1"}]}],"unresolved_conflicts":[],"verification_limitations":[]})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canaries_and_clock() {
        assert_eq!(canary("answer", 2, ""), "CANARY_ANSWER_2");
        assert_eq!(canary("review", 1, "OF_3"), "CANARY_REVIEW_1_OF_3");
        let c = FakeClock::default();
        c.advance_ms(250);
        assert_eq!(c.now_ms(), 250);
        assert_canaries("a CANARY_ANSWER_2 b", &[canary("answer", 2, "")], &[canary("answer", 3, "")]);
    }
}
