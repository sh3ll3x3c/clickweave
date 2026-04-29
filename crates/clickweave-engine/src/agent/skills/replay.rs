//! Inline replay engine for `InvokeSkill` actions. Phase 4 lands the
//! real implementation (per-step expansion, divergence detection,
//! `<skill_in_progress>` block construction).

#![allow(dead_code)]

/// Placeholder for the in-flight skill frame the runner suspends while
/// awaiting an LLM-fallback turn. Phase 4 replaces this unit alias with
/// the real frame struct (current step pointer, captures, sub-skill
/// stack). Phase 3 only needs the type to exist so `StateRunner`'s
/// `suspended_skill_frame: Option<SkillFrame>` field type-checks.
pub type SkillFrame = ();
