//! Public types for the procedural-skills layer.
//!
//! Phase 1 introduces the type surface only — filesystem I/O, the file
//! watcher, the extractor, and the replay engine are wired up in
//! subsequent phases. Module-level `#[allow(dead_code)]` mirrors the
//! Spec 2 episodic types pattern.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::step_record::WorldModelSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    ProjectLocal,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    Draft,
    Confirmed,
    Promoted,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SubgoalSignature(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ApplicabilitySignature(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ParameterSlot {
    pub name: String,
    pub type_tag: String,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BindingRef {
    Captured { name: String },
    Params { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct OutputDeclaration {
    pub name: String,
    pub type_tag: String,
    pub from: BindingRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AxDescriptorMatch {
    pub role: String,
    pub name: String,
    pub parent_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum CaptureSource {
    AxDescriptor { descriptor: AxDescriptorMatch },
    ToolResult { jsonpath: String },
    Literal { value: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CaptureClause {
    pub name: String,
    pub source: CaptureSource,
}

/// Skills-layer mirror of `agent::types::WorldModelDiff` (same
/// `changed_fields: Vec<String>` shape). Owned by this module so the
/// `Skill` value round-trips through YAML / JSON without forcing
/// `Deserialize` onto the runtime diff type. The extractor (Phase 3)
/// converts from `WorldModelDiff` at the boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExpectedWorldModelDelta {
    pub changed_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ActionSketchStep {
    ToolCall {
        tool: String,
        args: serde_json::Value,
        captures_pre: Vec<CaptureClause>,
        captures: Vec<CaptureClause>,
        expected_world_model_delta: ExpectedWorldModelDelta,
    },
    SubSkill {
        skill_id: String,
        version: u32,
        parameters: serde_json::Value,
        bind_outputs_as: HashMap<String, String>,
    },
    Loop {
        until: LoopPredicate,
        body: Vec<ActionSketchStep>,
        max_iterations: u32,
        iteration_delay_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum LoopPredicate {
    WorldModelDelta { expr: String },
    StepCountReached { count: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum OutcomePredicate {
    SubgoalCompleted {
        post_state_world_model_signature: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ApplicabilityHints {
    pub apps: Vec<String>,
    pub hosts: Vec<String>,
    pub signature: ApplicabilitySignature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProvenanceEntry {
    pub run_id: String,
    pub step_index: usize,
    pub completed_at: DateTime<Utc>,
    pub workflow_hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SkillStats {
    pub occurrence_count: u32,
    pub success_rate: f32,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub last_invoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Skill {
    pub id: String,
    pub version: u32,
    pub state: SkillState,
    pub scope: SkillScope,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub subgoal_text: String,
    pub subgoal_signature: SubgoalSignature,
    pub applicability: ApplicabilityHints,
    pub parameter_schema: Vec<ParameterSlot>,
    pub action_sketch: Vec<ActionSketchStep>,
    pub outputs: Vec<OutputDeclaration>,
    pub outcome_predicate: OutcomePredicate,
    pub provenance: Vec<ProvenanceEntry>,
    pub stats: SkillStats,
    pub edited_by_user: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub produced_node_ids: Vec<Uuid>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct SkillContext {
    pub enabled: bool,
    pub project_skills_dir: PathBuf,
    pub global_skills_dir: Option<PathBuf>,
    pub project_id: String,
}

impl SkillContext {
    /// Construct a context that disables every skill hook on the
    /// runner. Mirrors `EpisodicContext::disabled()` — used by tests
    /// and internal callers that don't construct skill paths.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            project_skills_dir: PathBuf::new(),
            global_skills_dir: None,
            project_id: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecordedStep {
    pub tool_name: String,
    pub arguments: serde_json::Value,
    pub result_text: String,
    pub world_model_pre: WorldModelSnapshot,
    pub world_model_post: WorldModelSnapshot,
}

#[derive(Debug, Clone)]
pub struct RetrievedSkill {
    pub skill: Arc<Skill>,
    pub score: f32,
}

#[derive(Debug)]
pub enum MaybeExtracted {
    Inserted {
        skill_id: String,
        version: u32,
    },
    Merged {
        skill_id: String,
        version: u32,
        occurrence_count: u32,
    },
    Skipped {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BindingCorrection {
    pub step_index: usize,
    pub capture_name: String,
    pub keep: bool,
    pub correction: Option<CaptureClause>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SkillRefinementProposal {
    pub parameter_schema: Vec<ParameterSlot>,
    pub binding_corrections: Vec<BindingCorrection>,
    pub description: String,
    pub name_suggestion: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("invalid frontmatter: {0}")]
    InvalidFrontmatter(String),
    #[error("skill not found: {0}@v{1}")]
    NotFound(String, u32),
    #[error("skill in draft state cannot be invoked: {0}@v{1}")]
    DraftCannotInvoke(String, u32),
    #[error("invalid parameters: {0}")]
    InvalidParameters(String),
    #[error("substitution error: {0}")]
    Substitution(String),
    #[error("outcome predicate failed: {0}")]
    OutcomeFailed(String),
}
