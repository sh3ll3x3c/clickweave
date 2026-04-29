//! Procedural-skills layer (Spec 3).
//!
//! On-disk markdown skill files with YAML frontmatter, a per-project +
//! opt-in global directory tier, an in-memory `SkillIndex` rebuilt per
//! run, and an extractor + replay engine wired into the Spec 1 agent
//! loop.
//!
//! Phase 1 lands the pure-logic modules (types, signatures, frontmatter
//! parser, provenance tracer, loop folding, substitution, outcome
//! predicate, render block). Filesystem I/O, the file watcher, the
//! extractor, the retrieval scorer, and the replay engine arrive in
//! later phases. Everything in this module is `#[allow(dead_code)]`
//! until those phases wire it into `runner.rs`.

#![allow(dead_code)]

pub mod extractor;
pub mod frontmatter;
pub mod index;
pub mod loop_folding;
pub mod outcome;
pub mod provenance;
pub mod render;
pub mod replay;
pub mod retrieval;
pub mod signature;
pub mod store;
pub mod substitution;
pub mod types;
pub mod walkthrough;
pub mod watcher;
pub mod watcher_consumer;

pub use index::SkillIndex;
pub use replay::SkillFrame;
pub use store::{MoveReport, SkillStore, filename_for, move_skills_to_project, slugify};
pub use types::{
    ActionSketchStep, ApplicabilityHints, ApplicabilitySignature, BindingCorrection, BindingRef,
    CaptureClause, CaptureSource, ExpectedWorldModelDelta, LoopPredicate, MaybeExtracted,
    OutcomePredicate, OutputDeclaration, ParameterSlot, ProvenanceEntry, RecordedStep,
    RetrievedSkill, Skill, SkillContext, SkillError, SkillRefinementProposal, SkillScope,
    SkillState, SkillStats, SubgoalSignature,
};
