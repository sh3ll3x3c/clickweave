//! Pure function that decides whether a workflow-local episode should be
//! copied to the global store at run-terminal (D31).

#![allow(dead_code)]

/// Promote if the episode has been seen more than once in this workflow OR
/// a matching signature already exists in the global store. Both conditions
/// are cheap SQL lookups at the call site.
pub fn should_promote(workflow_occurrence_count: u32, global_has_matching_signature: bool) -> bool {
    workflow_occurrence_count >= 2 || global_has_matching_signature
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_occurrence_alone_does_not_promote() {
        assert!(!should_promote(1, false));
    }

    #[test]
    fn second_occurrence_promotes() {
        assert!(should_promote(2, false));
    }

    #[test]
    fn first_occurrence_promotes_if_global_match_exists() {
        assert!(should_promote(1, true));
    }

    #[test]
    fn zero_occurrence_with_global_match_promotes() {
        // Defensive: if workflow count somehow starts at 0, global match still wins
        assert!(should_promote(0, true));
    }

    #[test]
    fn zero_occurrence_no_global_does_not_promote() {
        assert!(!should_promote(0, false));
    }
}
