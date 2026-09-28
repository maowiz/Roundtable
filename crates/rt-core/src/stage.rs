use serde::{Deserialize, Serialize};
use std::fmt;

/// The four dependency barriers of a run (PRD section 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Answer,
    Review,
    Improve,
    Synthesize,
}

impl Stage {
    pub const ALL: [Stage; 4] = [Stage::Answer, Stage::Review, Stage::Improve, Stage::Synthesize];

    pub fn index(self) -> usize {
        match self {
            Stage::Answer => 0,
            Stage::Review => 1,
            Stage::Improve => 2,
            Stage::Synthesize => 3,
        }
    }

    pub fn next(self) -> Option<Stage> {
        match self {
            Stage::Answer => Some(Stage::Review),
            Stage::Review => Some(Stage::Improve),
            Stage::Improve => Some(Stage::Synthesize),
            Stage::Synthesize => None,
        }
    }

    /// Number of logical calls this stage needs for `n` seats.
    pub fn call_count(self, n: usize) -> usize {
        match self {
            Stage::Answer | Stage::Improve => n,
            Stage::Review => n * (n - 1),
            Stage::Synthesize => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Answer => "answer",
            Stage::Review => "review",
            Stage::Improve => "improve",
            Stage::Synthesize => "synthesize",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Base logical calls for a run: N² + N + 1.
pub fn base_calls(n: usize) -> usize {
    Stage::ALL.iter().map(|s| s.call_count(n)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_counts_match_prd_table() {
        assert_eq!(base_calls(2), 7);
        assert_eq!(base_calls(4), 21);
        assert_eq!(base_calls(8), 73);
        assert_eq!(Stage::Review.call_count(4), 12);
    }

    #[test]
    fn stages_are_ordered() {
        assert_eq!(Stage::Answer.next(), Some(Stage::Review));
        assert_eq!(Stage::Synthesize.next(), None);
        assert!(Stage::Answer < Stage::Synthesize);
    }
}
