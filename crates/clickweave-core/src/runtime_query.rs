use serde::{Deserialize, Serialize};
use uuid::Uuid;

// NOTE: RuntimeQuery is defined in clickweave-engine (it has a tokio oneshot Sender).
// Only the pure-data types live here.

/// Resolution sent back from Tauri layer to executor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeResolution {
    Updated(WorkflowPatchCompact),
    Rewind {
        patch: WorkflowPatchCompact,
        first_node_id: Uuid,
    },
    Removed(WorkflowPatchCompact),
    Rejected,
}

/// Compact patch representation for the resolution channel.
/// Uses the same structure as the Tauri-layer WorkflowPatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPatchCompact {
    pub added_nodes: Vec<crate::Node>,
    pub removed_node_ids: Vec<Uuid>,
    pub updated_nodes: Vec<crate::Node>,
    pub added_edges: Vec<crate::Edge>,
    pub removed_edges: Vec<crate::Edge>,
}
