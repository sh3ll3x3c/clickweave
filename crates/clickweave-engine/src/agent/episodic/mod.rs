//! Episodic memory layer for the Spec 2 agent.
//!
//! See the agent-episodic-memory design doc for the full rationale.
//! High-level shape:
//!
//! - Primary use case: recovery reuse (D20).
//! - Storage: SQLite per scope (D26), two-tier workflow-local + global (D21).
//! - Retrieval: hybrid — structured `PreStateSignature` primary + text
//!   similarity secondary (D22), fires at run-start and `Recovering` entry (D24).
//! - Writes: async, piggyback on Spec 1's `RecoverySucceeded` StepRecord (D30).
//! - Failure isolation: never fail the agent run (D32).

pub mod embedder;
pub mod promotion;
pub mod render;
pub mod retrieval;
pub mod signature;
pub mod store;
pub mod types;
pub mod write;

pub use embedder::{Embedder, HashedShingleEmbedder};
pub use store::{EpisodicStore, EpisodicStoreConfig, SqliteEpisodicStore};
pub use types::{
    CompactAction, EpisodeRecord, EpisodeScope, EpisodicContext, EpisodicError, FailureSignature,
    InsertOutcome, PreStateSignature, PromotionTerminalKind, RecoveringEntrySnapshot,
    RecoveryActionsHash, RetrievalQuery, RetrievalTrigger, RetrievedEpisode, ScoreBreakdown,
    TriggeringError, WriteRequest,
};
pub use write::EpisodicWriter;

#[cfg(test)]
mod end_to_end_tests;
