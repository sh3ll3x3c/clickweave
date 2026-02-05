use crate::Workflow;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Workflow has no nodes")]
    NoNodes,

    #[error("No entry point found (all nodes have incoming edges)")]
    NoEntryPoint,

    #[error("Node {0} has multiple outgoing edges (only single path allowed)")]
    MultipleOutgoingEdges(String),

    #[error("Cycle detected in workflow")]
    CycleDetected,
}

pub fn validate_workflow(workflow: &Workflow) -> Result<(), ValidationError> {
    if workflow.nodes.is_empty() {
        return Err(ValidationError::NoNodes);
    }

    // Check for entry points (nodes with no incoming edges)
    let targets: std::collections::HashSet<_> = workflow.edges.iter().map(|e| e.to).collect();
    let entry_count = workflow
        .nodes
        .iter()
        .filter(|n| !targets.contains(&n.id))
        .count();
    if entry_count == 0 {
        return Err(ValidationError::NoEntryPoint);
    }

    // Check single outgoing edge per node
    for node in &workflow.nodes {
        let outgoing = workflow.edges.iter().filter(|e| e.from == node.id).count();
        if outgoing > 1 {
            return Err(ValidationError::MultipleOutgoingEdges(node.name.clone()));
        }
    }

    // Check for cycles using visited set
    let mut visited = std::collections::HashSet::new();
    for node in &workflow.nodes {
        if visited.contains(&node.id) {
            continue;
        }
        let mut current = node.id;
        let mut path = std::collections::HashSet::new();
        loop {
            if !path.insert(current) {
                return Err(ValidationError::CycleDetected);
            }
            visited.insert(current);
            let next = workflow.edges.iter().find(|e| e.from == current);
            match next {
                Some(edge) => current = edge.to,
                None => break,
            }
        }
    }

    Ok(())
}
