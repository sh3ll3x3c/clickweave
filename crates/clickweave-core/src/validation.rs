use thiserror::Error;

use crate::Workflow;

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
    let has_entry_point = workflow.nodes.iter().any(|n| !targets.contains(&n.id));
    if !has_entry_point {
        return Err(ValidationError::NoEntryPoint);
    }

    // Check single outgoing edge per node
    for node in &workflow.nodes {
        let outgoing = workflow.edges.iter().filter(|e| e.from == node.id).count();
        if outgoing > 1 {
            return Err(ValidationError::MultipleOutgoingEdges(node.name.clone()));
        }
    }

    // Check for cycles by walking each unvisited chain
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
            match workflow.edges.iter().find(|e| e.from == current) {
                Some(edge) => current = edge.to,
                None => break,
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClickParams, NodeType, Position, TypeTextParams};

    #[test]
    fn test_validate_empty_workflow() {
        let wf = Workflow::default();
        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::NoNodes));
    }

    #[test]
    fn test_validate_no_entry_point() {
        let mut wf = Workflow::default();
        let a = wf.add_node(
            NodeType::Click(ClickParams::default()),
            Position { x: 0.0, y: 0.0 },
        );
        let b = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            Position { x: 100.0, y: 0.0 },
        );
        // Create edges so every node has an incoming edge
        wf.add_edge(a, b);
        wf.add_edge(b, a);

        let err = validate_workflow(&wf).unwrap_err();
        // This could be NoEntryPoint or CycleDetected depending on check order
        assert!(
            matches!(err, ValidationError::NoEntryPoint)
                || matches!(err, ValidationError::CycleDetected)
        );
    }

    #[test]
    fn test_validate_multiple_outgoing_edges() {
        let mut wf = Workflow::default();
        let a = wf.add_node(
            NodeType::Click(ClickParams::default()),
            Position { x: 0.0, y: 0.0 },
        );
        let b = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            Position { x: 100.0, y: 0.0 },
        );
        let c = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            Position { x: 200.0, y: 0.0 },
        );
        wf.add_edge(a, b);
        wf.add_edge(a, c); // a has 2 outgoing

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::MultipleOutgoingEdges(_)));
    }

    #[test]
    fn test_validate_valid_linear_workflow() {
        let mut wf = Workflow::default();
        let a = wf.add_node(
            NodeType::Click(ClickParams::default()),
            Position { x: 0.0, y: 0.0 },
        );
        let b = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            Position { x: 100.0, y: 0.0 },
        );
        wf.add_edge(a, b);

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_single_node() {
        let mut wf = Workflow::default();
        wf.add_node(
            NodeType::Click(ClickParams::default()),
            Position { x: 0.0, y: 0.0 },
        );

        assert!(validate_workflow(&wf).is_ok());
    }
}
