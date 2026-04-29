//! Outcome predicate evaluation.
//!
//! After a skill replay finishes its action sketch, the outcome
//! predicate decides whether the run hit a clean match (signature
//! matched), an adapted match (subgoal completed but post-state
//! signature drifted), or a mismatch (subgoal not completed).

#![allow(dead_code)]

use super::signature::compute_post_state_signature;
use super::types::OutcomePredicate;
use crate::agent::world_model::WorldModel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeResult {
    CleanMatch,
    AdaptedMatch,
    Mismatch,
}

pub fn evaluate(
    predicate: &OutcomePredicate,
    subgoal_completed: bool,
    post_state: &WorldModel,
) -> OutcomeResult {
    match predicate {
        OutcomePredicate::SubgoalCompleted {
            post_state_world_model_signature,
        } => {
            if !subgoal_completed {
                return OutcomeResult::Mismatch;
            }
            match post_state_world_model_signature {
                Some(expected) => {
                    let actual = compute_post_state_signature(post_state);
                    if &actual == expected {
                        OutcomeResult::CleanMatch
                    } else {
                        OutcomeResult::AdaptedMatch
                    }
                }
                None => OutcomeResult::CleanMatch,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wm() -> WorldModel {
        WorldModel::default()
    }

    #[test]
    fn no_subgoal_completion_is_mismatch() {
        let p = OutcomePredicate::SubgoalCompleted {
            post_state_world_model_signature: None,
        };
        assert_eq!(evaluate(&p, false, &wm()), OutcomeResult::Mismatch);
    }

    #[test]
    fn subgoal_completion_without_post_signature_is_clean() {
        let p = OutcomePredicate::SubgoalCompleted {
            post_state_world_model_signature: None,
        };
        assert_eq!(evaluate(&p, true, &wm()), OutcomeResult::CleanMatch);
    }

    #[test]
    fn signature_mismatch_is_adapted() {
        let p = OutcomePredicate::SubgoalCompleted {
            post_state_world_model_signature: Some("deadbeefdeadbeef".into()),
        };
        assert_eq!(evaluate(&p, true, &wm()), OutcomeResult::AdaptedMatch);
    }

    #[test]
    fn matching_signature_is_clean() {
        let actual = compute_post_state_signature(&wm());
        let p = OutcomePredicate::SubgoalCompleted {
            post_state_world_model_signature: Some(actual),
        };
        assert_eq!(evaluate(&p, true, &wm()), OutcomeResult::CleanMatch);
    }
}
