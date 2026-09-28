//! Frozen seat configuration inside a run snapshot (PRD 7.2, plan 3.1).

use crate::{CredentialRef, MicroUsd};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Answer,
    Review,
    Improve,
    Synthesize,
}

impl Role {
    pub fn default_temperature(self) -> f32 {
        match self {
            Role::Answer => 0.7,
            Role::Review => 0.2,
            Role::Improve => 0.4,
            Role::Synthesize => 0.3,
        }
    }
}

/// Per-role generation settings. Only settings the model supports are kept (PAN-01);
/// `temperature` is None for models that reject it (reasoning models).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleSettings {
    pub temperature: Option<f32>,
    pub max_output_tokens: u32,
    pub reasoning_effort: Option<String>,
    /// Provider-side web search for this role's calls (OpenRouter `plugins: web`). Off by default:
    /// results are external data entering the model; they never cross seats and never bypass the envelope.
    #[serde(default)]
    pub web_search: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatLimits {
    pub cost_cap: MicroUsd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenSeat {
    pub seat_index: u8,
    pub label: String,
    pub provider: String,
    pub credential_ref: CredentialRef,
    pub model_id: String,
    /// True when the provider documents the ID as a moving alias (PAN-03).
    pub model_is_alias: bool,
    pub answer: RoleSettings,
    pub review: RoleSettings,
    pub improve: RoleSettings,
    pub prompt_answer: String,
    pub prompt_review: String,
    pub prompt_improve: String,
    pub limits: SeatLimits,
}

impl FrozenSeat {
    pub fn settings_for(&self, role: Role) -> &RoleSettings {
        match role {
            Role::Answer => &self.answer,
            Role::Review => &self.review,
            Role::Improve | Role::Synthesize => &self.improve,
        }
    }

    pub fn prompt_for(&self, role: Role) -> &str {
        match role {
            Role::Answer => &self.prompt_answer,
            Role::Review => &self.prompt_review,
            Role::Improve | Role::Synthesize => &self.prompt_improve,
        }
    }

    /// Words shown in the UI become a conservative token bound (plan section 9).
    pub fn words_to_tokens(words: u32) -> u32 {
        ((words as f64) * 1.4).ceil() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_to_tokens_is_conservative() {
        assert_eq!(FrozenSeat::words_to_tokens(1200), 1680);
        assert_eq!(FrozenSeat::words_to_tokens(1), 2);
    }
}
