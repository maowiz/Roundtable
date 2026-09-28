//! Roundtable core: identifiers, domain types and state machines.
//!
//! This crate has no IO. Everything here is deterministic and unit-tested so the
//! engine, ledger and envelope crates can rely on it without mocks.

pub mod ids;
pub mod money;
pub mod plan;
pub mod run;
pub mod seat;
pub mod stage;

pub use ids::*;
pub use money::MicroUsd;
pub use plan::{review_plan, ReviewPair};
pub use run::{next_call_state, AttemptOutcome, CallState, RunEventKind, RunState, TransitionError};
pub use seat::{FrozenSeat, Role, RoleSettings, SeatLimits};
pub use stage::Stage;

/// Bounds shared by preflight, envelope building and validation (PRD section 6).
pub mod bounds {
    pub const MIN_SEATS: usize = 2;
    pub const MAX_SEATS: usize = 8;
    pub const MAX_QUESTION_CHARS: usize = 20_000;
    pub const MAX_REFERENCE_CHARS: usize = 40_000;
    pub const MAX_PROMPT_CHARS: usize = 12_000;
    pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
    pub const MAX_CLAIMS: usize = 50;
    pub const MAX_ATTEMPTS_PER_CALL: u8 = 3;
    pub const MAX_SCHEMA_RETRIES_PER_CALL: u8 = 1;
    pub const DEFAULT_ATTEMPT_DEADLINE_SECS: u64 = 90;
    pub const DEFAULT_RUN_DEADLINE_SECS: u64 = 300;
    pub const DEFAULT_GLOBAL_IN_FLIGHT: usize = 8;
    pub const DEFAULT_PER_PROVIDER_IN_FLIGHT: usize = 2;
    pub const HARD_MAX_IN_FLIGHT: usize = 16;
}

/// SHA-256 digest of canonical bytes, hex encoded. Used for envelopes, papers and request bodies.
pub fn digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

/// Canonical JSON: serde_json with sorted keys is not available on `Value` directly,
/// so we re-serialize through a BTreeMap-ordered walk.
pub fn canonical_json(v: &serde_json::Value) -> String {
    fn walk(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push('{');
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(k).unwrap());
                    out.push(':');
                    walk(&m[*k], out);
                }
                out.push('}');
            }
            serde_json::Value::Array(a) => {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    walk(x, out);
                }
                out.push(']');
            }
            other => out.push_str(&other.to_string()),
        }
    }
    let mut s = String::new();
    walk(v, &mut s);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let v: serde_json::Value = serde_json::from_str(r#"{"b":1,"a":{"z":[3,{"y":2,"x":1}],"m":null}}"#).unwrap();
        assert_eq!(canonical_json(&v), r#"{"a":{"m":null,"z":[3,{"x":1,"y":2}]},"b":1}"#);
    }

    #[test]
    fn digest_is_stable() {
        assert_eq!(digest(b"roundtable"), digest(b"roundtable"));
        assert_ne!(digest(b"a"), digest(b"b"));
        assert_eq!(digest(b"").len(), 64);
    }
}
