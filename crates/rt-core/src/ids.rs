//! ULID-based identifiers. Each entity gets its own newtype so a paper ID cannot be
//! passed where a run ID is expected (ISO-02, SEC-10).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid identifier: {0}")]
pub struct InvalidId(pub String);

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(ulid::Ulid);

        impl $name {
            pub fn new() -> Self {
                Self(ulid::Ulid::new())
            }
            pub fn from_ulid(u: ulid::Ulid) -> Self {
                Self(u)
            }
            pub fn as_ulid(&self) -> ulid::Ulid {
                self.0
            }
            pub const PREFIX: &'static str = $prefix;
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}:{}", $prefix, self.0)
            }
        }
        impl FromStr for $name {
            type Err = InvalidId;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                if s.len() != 26 {
                    return Err(InvalidId(s.to_string()));
                }
                ulid::Ulid::from_string(s).map(Self).map_err(|_| InvalidId(s.to_string()))
            }
        }
    };
}

id_type!(RunId, "run");
id_type!(PanelId, "panel");
id_type!(LogicalCallId, "call");
id_type!(AttemptId, "attempt");
id_type!(EnvelopeId, "env");
id_type!(PaperId, "paper");
id_type!(ViewId, "view");
id_type!(BudgetEntryId, "budget");
id_type!(CredentialRef, "cred");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_strings() {
        let id = RunId::new();
        let s = id.to_string();
        assert_eq!(s.len(), 26);
        assert_eq!(RunId::from_str(&s).unwrap(), id);
    }

    #[test]
    fn malformed_ids_are_rejected() {
        assert!(RunId::from_str("not-an-id").is_err());
        assert!(RunId::from_str("").is_err());
        assert!(RunId::from_str("01ARZ3NDEKTSV4RRFFQ69G5FA").is_err()); // 25 chars
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = PaperId::new();
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{}\"", id));
        let back: PaperId = serde_json::from_str(&json).unwrap();
        assert_eq!(back, id);
    }
}
