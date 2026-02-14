mod mapping;
mod parse;
mod patch;
mod plan;
mod prompt;
mod repair;

pub mod conversation;

#[cfg(test)]
mod tests;

use clickweave_core::{Edge, Node, Workflow};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

// ── Public types ────────────────────────────────────────────────

/// A single step in the planner's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "step_type")]
pub enum PlanStep {
    Tool {
        tool_name: String,
        arguments: Value,
        #[serde(default)]
        name: Option<String>,
    },
    AiTransform {
        kind: String,
        input_ref: String,
        #[serde(default)]
        output_schema: Option<Value>,
        #[serde(default)]
        name: Option<String>,
    },
    AiStep {
        prompt: String,
        #[serde(default)]
        allowed_tools: Option<Vec<String>>,
        #[serde(default)]
        max_tool_calls: Option<u32>,
        #[serde(default)]
        timeout_ms: Option<u64>,
        #[serde(default)]
        name: Option<String>,
    },
}

/// The raw planner LLM output.
#[derive(Debug, Deserialize)]
pub struct PlannerOutput {
    pub steps: Vec<PlanStep>,
}

/// Result of planning a workflow.
#[derive(Debug)]
pub struct PlanResult {
    pub workflow: Workflow,
    pub warnings: Vec<String>,
}

// ── Patch types ─────────────────────────────────────────────────

/// Output from the patcher LLM.
#[derive(Debug, Deserialize)]
struct PatcherOutput {
    #[serde(default)]
    pub add: Vec<PlanStep>,
    #[serde(default)]
    pub remove_node_ids: Vec<String>,
    #[serde(default)]
    pub update: Vec<PatchNodeUpdate>,
}

/// A node update from the patcher (only changed fields).
#[derive(Debug, Deserialize)]
struct PatchNodeUpdate {
    pub node_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub node_type: Option<Value>,
    /// Flat alternative: LLMs often echo the node summary format.
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub arguments: Option<Value>,
}

/// Result of patching a workflow.
pub struct PatchResult {
    pub added_nodes: Vec<Node>,
    pub removed_node_ids: Vec<Uuid>,
    pub updated_nodes: Vec<Node>,
    pub added_edges: Vec<Edge>,
    pub removed_edges: Vec<Edge>,
    pub warnings: Vec<String>,
}

// ── Re-exports ──────────────────────────────────────────────────

pub use patch::{patch_workflow, patch_workflow_with_backend};
pub use plan::{plan_workflow, plan_workflow_with_backend};
