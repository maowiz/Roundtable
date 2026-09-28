//! Run and logical-call state machines (PRD 8.1, plan 3.2 and 3.3).
//! Transitions are total functions: an illegal transition is an error, never a silent no-op.

use crate::Stage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Draft,
    Validating,
    Answering,
    Reviewing,
    Improving,
    Synthesizing,
    Completed,
    Blocked,
    Interrupted,
    Cancelling,
    Cancelled,
    Failed,
}

impl RunState {
    pub fn is_terminal(self) -> bool {
        matches!(self, RunState::Completed | RunState::Cancelled | RunState::Failed)
    }

    pub fn is_active_stage(self) -> bool {
        matches!(self, RunState::Answering | RunState::Reviewing | RunState::Improving | RunState::Synthesizing)
    }

    pub fn for_stage(stage: Stage) -> RunState {
        match stage {
            Stage::Answer => RunState::Answering,
            Stage::Review => RunState::Reviewing,
            Stage::Improve => RunState::Improving,
            Stage::Synthesize => RunState::Synthesizing,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Draft => "draft",
            RunState::Validating => "validating",
            RunState::Answering => "answering",
            RunState::Reviewing => "reviewing",
            RunState::Improving => "improving",
            RunState::Synthesizing => "synthesizing",
            RunState::Completed => "completed",
            RunState::Blocked => "blocked",
            RunState::Interrupted => "interrupted",
            RunState::Cancelling => "cancelling",
            RunState::Cancelled => "cancelled",
            RunState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    Validate,
    ValidationFailed,
    ValidationOk,
    /// All required calls of the current stage accepted; move to the next stage.
    BarrierPassed,
    /// Final paper accepted.
    Accept,
    /// Budget, deadline, exhausted attempts, persistence failure, context overflow.
    Block,
    /// User resumes a blocked or interrupted run (after amending limits, if needed).
    Resume,
    Cancel,
    /// All in-flight work has settled after a cancel.
    CancelSettled,
    /// Set at process launch for any non-terminal run.
    Interrupt,
    /// Unrecoverable (for example, final schema invalid after the attempt cap).
    Fail,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("illegal transition: {from:?} on {event:?}")]
pub struct TransitionError {
    pub from: RunState,
    pub event: RunEventKind,
}

impl RunState {
    /// Apply an event. `stage` is the run's current stage, needed for BarrierPassed and Resume.
    pub fn apply(self, event: RunEventKind, stage: Stage) -> Result<RunState, TransitionError> {
        use RunEventKind as E;
        use RunState as S;
        let next = match (self, event) {
            (S::Draft, E::Validate) => S::Validating,
            (S::Validating, E::ValidationFailed) => S::Draft,
            (S::Validating, E::ValidationOk) => S::Answering,
            (s, E::BarrierPassed) if s.is_active_stage() => match stage.next() {
                Some(n) => S::for_stage(n),
                None => return Err(TransitionError { from: self, event }),
            },
            (S::Synthesizing, E::Accept) => S::Completed,
            (s, E::Block) if s.is_active_stage() => S::Blocked,
            (S::Blocked | S::Interrupted, E::Resume) => S::for_stage(stage),
            (s, E::Cancel) if s.is_active_stage() || matches!(s, S::Blocked | S::Interrupted | S::Validating) => S::Cancelling,
            (S::Cancelling, E::CancelSettled) => S::Cancelled,
            (s, E::Interrupt) if !s.is_terminal() && s != S::Draft => S::Interrupted,
            (s, E::Fail) if !s.is_terminal() => S::Failed,
            _ => return Err(TransitionError { from: self, event }),
        };
        Ok(next)
    }
}

/// Logical call states (plan 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallState {
    Planned,
    Ready,
    InFlight,
    RetryWait,
    Accepted,
    Uncertain,
    Refused,
    Invalid,
    Failed,
}

impl CallState {
    pub fn is_terminal(self) -> bool {
        matches!(self, CallState::Accepted | CallState::Refused | CallState::Failed)
    }
    /// States from which the engine may dispatch another automatic attempt.
    pub fn can_auto_dispatch(self) -> bool {
        matches!(self, CallState::Ready | CallState::RetryWait | CallState::Invalid)
    }
    /// States that require an explicit user action before another attempt.
    pub fn needs_user(self) -> bool {
        matches!(self, CallState::Uncertain)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            CallState::Planned => "planned",
            CallState::Ready => "ready",
            CallState::InFlight => "in_flight",
            CallState::RetryWait => "retry_wait",
            CallState::Accepted => "accepted",
            CallState::Uncertain => "uncertain",
            CallState::Refused => "refused",
            CallState::Invalid => "invalid",
            CallState::Failed => "failed",
        }
    }
}

/// Outcome of one attempt, classified by the adapter (plan section 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AttemptOutcome {
    Accepted,
    /// Provider says wait; `retry_after_ms` may be None when unspecified.
    RateLimited { retry_after_ms: Option<u64> },
    /// Failed for certain before any request bytes were sent.
    TransportBeforeSend,
    /// Documented transient provider failure (5xx, overloaded).
    TransientServer,
    /// Timeout, stream loss or crash after the request may have been accepted.
    Uncertain,
    /// Valid transport, output failed schema or semantic validation.
    InvalidOutput,
    Refused,
    Truncated,
    /// Auth, quota, invalid request, unsupported model or setting.
    NonRetryable,
}

/// Decide the next call state after an attempt (PRD 8.3).
/// `attempts_used` counts the attempt just finished. `schema_retries_used` counts schema retries so far.
pub fn next_call_state(outcome: &AttemptOutcome, attempts_used: u8, schema_retries_used: u8) -> CallState {
    use crate::bounds::{MAX_ATTEMPTS_PER_CALL, MAX_SCHEMA_RETRIES_PER_CALL};
    let attempts_left = attempts_used < MAX_ATTEMPTS_PER_CALL;
    match outcome {
        AttemptOutcome::Accepted => CallState::Accepted,
        AttemptOutcome::Refused => CallState::Refused,
        AttemptOutcome::NonRetryable => CallState::Failed,
        AttemptOutcome::Truncated => {
            if attempts_left {
                CallState::Invalid
            } else {
                CallState::Failed
            }
        }
        // The reply was lost after sending (timeout, broken connection). The possible charge stays on the
        // books as unresolved; while attempts remain the call is retried rather than stopping the whole run.
        AttemptOutcome::Uncertain => {
            if attempts_left {
                CallState::RetryWait
            } else {
                CallState::Uncertain
            }
        }
        AttemptOutcome::InvalidOutput => {
            if attempts_left && schema_retries_used < MAX_SCHEMA_RETRIES_PER_CALL {
                CallState::Invalid
            } else {
                CallState::Failed
            }
        }
        AttemptOutcome::RateLimited { .. } | AttemptOutcome::TransportBeforeSend | AttemptOutcome::TransientServer => {
            if attempts_left {
                CallState::RetryWait
            } else {
                CallState::Failed
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_through_all_stages() {
        let mut s = RunState::Draft;
        s = s.apply(RunEventKind::Validate, Stage::Answer).unwrap();
        s = s.apply(RunEventKind::ValidationOk, Stage::Answer).unwrap();
        assert_eq!(s, RunState::Answering);
        s = s.apply(RunEventKind::BarrierPassed, Stage::Answer).unwrap();
        assert_eq!(s, RunState::Reviewing);
        s = s.apply(RunEventKind::BarrierPassed, Stage::Review).unwrap();
        assert_eq!(s, RunState::Improving);
        s = s.apply(RunEventKind::BarrierPassed, Stage::Improve).unwrap();
        assert_eq!(s, RunState::Synthesizing);
        s = s.apply(RunEventKind::Accept, Stage::Synthesize).unwrap();
        assert_eq!(s, RunState::Completed);
        assert!(s.is_terminal());
    }

    #[test]
    fn terminal_states_reject_everything() {
        for s in [RunState::Completed, RunState::Cancelled, RunState::Failed] {
            for e in [RunEventKind::Cancel, RunEventKind::Interrupt, RunEventKind::Resume, RunEventKind::Block, RunEventKind::Fail] {
                assert!(s.apply(e, Stage::Answer).is_err(), "{s:?} accepted {e:?}");
            }
        }
    }

    #[test]
    fn block_resume_and_cancel() {
        let s = RunState::Reviewing.apply(RunEventKind::Block, Stage::Review).unwrap();
        assert_eq!(s, RunState::Blocked);
        assert_eq!(s.apply(RunEventKind::Resume, Stage::Review).unwrap(), RunState::Reviewing);
        let c = s.apply(RunEventKind::Cancel, Stage::Review).unwrap();
        assert_eq!(c, RunState::Cancelling);
        assert_eq!(c.apply(RunEventKind::CancelSettled, Stage::Review).unwrap(), RunState::Cancelled);
    }

    #[test]
    fn interrupt_marks_non_terminal_runs_only() {
        assert_eq!(RunState::Improving.apply(RunEventKind::Interrupt, Stage::Improve).unwrap(), RunState::Interrupted);
        assert!(RunState::Draft.apply(RunEventKind::Interrupt, Stage::Answer).is_err());
        assert_eq!(RunState::Interrupted.apply(RunEventKind::Resume, Stage::Improve).unwrap(), RunState::Improving);
    }

    #[test]
    fn synthesizing_cannot_pass_a_barrier() {
        assert!(RunState::Synthesizing.apply(RunEventKind::BarrierPassed, Stage::Synthesize).is_err());
    }

    #[test]
    fn retry_policy_respects_caps() {
        use AttemptOutcome as O;
        assert_eq!(next_call_state(&O::Accepted, 1, 0), CallState::Accepted);
        assert_eq!(next_call_state(&O::RateLimited { retry_after_ms: Some(1000) }, 1, 0), CallState::RetryWait);
        assert_eq!(next_call_state(&O::RateLimited { retry_after_ms: None }, 3, 0), CallState::Failed);
        assert_eq!(next_call_state(&O::InvalidOutput, 1, 0), CallState::Invalid);
        assert_eq!(next_call_state(&O::InvalidOutput, 2, 1), CallState::Failed, "only one schema retry");
        assert_eq!(next_call_state(&O::Uncertain, 1, 0), CallState::RetryWait, "a lost reply is retried while attempts remain");
        assert_eq!(next_call_state(&O::Uncertain, 3, 0), CallState::Uncertain, "then it waits for the user");
        assert_eq!(next_call_state(&O::Refused, 1, 0), CallState::Refused);
        assert_eq!(next_call_state(&O::NonRetryable, 1, 0), CallState::Failed);
        assert!(CallState::Uncertain.needs_user());
        assert!(!CallState::Uncertain.can_auto_dispatch());
    }
}
