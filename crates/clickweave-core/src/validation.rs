use crate::{NodeKind, Workflow};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Workflow must have exactly one Start node")]
    MissingOrMultipleStart,

    #[error("Workflow must have exactly one End node")]
    MissingOrMultipleEnd,

    #[error("Node {0} has multiple outgoing edges (only single path allowed)")]
    MultipleOutgoingEdges(String),

    #[error("Workflow is not connected: cannot reach End from Start")]
    NotConnected,

    #[error("Cycle detected in workflow")]
    CycleDetected,
}

pub fn validate_workflow(workflow: &Workflow) -> Result<(), ValidationError> {
    // Check exactly one Start node
    let start_count = workflow
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Start)
        .count();
    if start_count != 1 {
        return Err(ValidationError::MissingOrMultipleStart);
    }

    // Check exactly one End node
    let end_count = workflow
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::End)
        .count();
    if end_count != 1 {
        return Err(ValidationError::MissingOrMultipleEnd);
    }

    // Check single outgoing edge per node (except End)
    for node in &workflow.nodes {
        if node.kind == NodeKind::End {
            continue;
        }
        let outgoing = workflow.edges.iter().filter(|e| e.from == node.id).count();
        if outgoing > 1 {
            return Err(ValidationError::MultipleOutgoingEdges(node.name.clone()));
        }
    }

    // Check connectivity: execution order should end at End node
    let order = workflow.execution_order();
    if order.is_empty() {
        return Err(ValidationError::NotConnected);
    }

    let last_id = order.last().unwrap();
    let last_node = workflow.find_node(*last_id);
    if !last_node.is_some_and(|n| n.kind == NodeKind::End) {
        return Err(ValidationError::NotConnected);
    }

    Ok(())
}
