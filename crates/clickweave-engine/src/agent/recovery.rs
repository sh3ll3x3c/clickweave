/// Action the agent should take after encountering an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Retry the same action (e.g., transient network error).
    Retry,
    /// Re-observe the page and let the LLM choose a different action.
    ReObserve,
    /// Abort the agent run — too many consecutive errors.
    Abort,
}

/// Determine the recovery action based on consecutive error count and
/// the maximum allowed consecutive errors.
///
/// Strategy:
/// - 1st error: retry the same action (might be transient)
/// - 2nd error: re-observe the page (the page state may have changed)
/// - 3rd+ error: abort (the agent is stuck)
pub fn recovery_strategy(
    consecutive_errors: usize,
    max_consecutive_errors: usize,
) -> RecoveryAction {
    if consecutive_errors == 0 {
        // No errors yet — shouldn't be called, but default to retry
        return RecoveryAction::Retry;
    }

    if consecutive_errors >= max_consecutive_errors {
        return RecoveryAction::Abort;
    }

    if consecutive_errors == 1 {
        RecoveryAction::Retry
    } else {
        RecoveryAction::ReObserve
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_error_retries() {
        assert_eq!(recovery_strategy(1, 3), RecoveryAction::Retry);
    }

    #[test]
    fn second_error_reobserves() {
        assert_eq!(recovery_strategy(2, 3), RecoveryAction::ReObserve);
    }

    #[test]
    fn max_errors_aborts() {
        assert_eq!(recovery_strategy(3, 3), RecoveryAction::Abort);
    }

    #[test]
    fn over_max_errors_aborts() {
        assert_eq!(recovery_strategy(5, 3), RecoveryAction::Abort);
    }

    #[test]
    fn zero_errors_retries() {
        assert_eq!(recovery_strategy(0, 3), RecoveryAction::Retry);
    }

    #[test]
    fn single_max_aborts_on_first() {
        // With max_consecutive_errors = 1, the first error triggers abort
        assert_eq!(recovery_strategy(1, 1), RecoveryAction::Abort);
    }
}
