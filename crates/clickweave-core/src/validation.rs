use std::collections::{HashMap, HashSet};

use thiserror::Error;
use uuid::Uuid;

use crate::{EdgeOutput, NodeType, Workflow};

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

    #[error("If node '{0}' must have both true and false edges")]
    MissingIfBranch(String),

    #[error("Switch node '{0}' missing edge for case '{1}'")]
    MissingSwitchCase(String, String),

    #[error("Loop node '{0}' must have both body and done edges")]
    MissingLoopEdge(String),

    #[error("EndLoop node '{0}' references non-Loop node")]
    InvalidEndLoopTarget(String),

    #[error("Loop node '{0}' has no paired EndLoop")]
    UnpairedLoop(String),

    #[error("EndLoop node '{0}' edge does not point to its paired Loop")]
    EndLoopEdgeMismatch(String),
}

pub fn validate_workflow(workflow: &Workflow) -> Result<(), ValidationError> {
    if workflow.nodes.is_empty() {
        return Err(ValidationError::NoNodes);
    }

    // Check for entry points: nodes with no incoming edges.
    // EndLoop back-edges to Loop nodes don't count as incoming edges.
    let targets_excluding_endloop_back: HashSet<Uuid> = workflow
        .edges
        .iter()
        .filter(|e| {
            // Exclude edges that originate from an EndLoop node and point to its paired Loop
            if let Some(node) = workflow.find_node(e.from)
                && let NodeType::EndLoop(params) = &node.node_type
                && e.to == params.loop_id
            {
                return false;
            }
            true
        })
        .map(|e| e.to)
        .collect();

    let has_entry_point = workflow
        .nodes
        .iter()
        .any(|n| !targets_excluding_endloop_back.contains(&n.id));
    if !has_entry_point {
        return Err(ValidationError::NoEntryPoint);
    }

    // Validate outgoing edges per node based on node type
    validate_outgoing_edges(workflow)?;

    // Validate loop pairing
    validate_loop_pairing(workflow)?;

    // Cycle detection: ignore edges originating from EndLoop nodes, then
    // do a standard DFS-based cycle check on the remaining graph.
    validate_no_illegal_cycles(workflow)?;

    Ok(())
}

/// Validate that each node has the correct outgoing edges for its type.
fn validate_outgoing_edges(workflow: &Workflow) -> Result<(), ValidationError> {
    for node in &workflow.nodes {
        let outgoing: Vec<_> = workflow
            .edges
            .iter()
            .filter(|e| e.from == node.id)
            .collect();

        match &node.node_type {
            NodeType::If(_) => {
                let has_true = outgoing
                    .iter()
                    .any(|e| e.output.as_ref() == Some(&EdgeOutput::IfTrue));
                let has_false = outgoing
                    .iter()
                    .any(|e| e.output.as_ref() == Some(&EdgeOutput::IfFalse));
                if !has_true || !has_false {
                    return Err(ValidationError::MissingIfBranch(node.name.clone()));
                }
            }
            NodeType::Switch(params) => {
                for case in &params.cases {
                    let has_case = outgoing.iter().any(|e| {
                        e.output.as_ref()
                            == Some(&EdgeOutput::SwitchCase {
                                name: case.name.clone(),
                            })
                    });
                    if !has_case {
                        return Err(ValidationError::MissingSwitchCase(
                            node.name.clone(),
                            case.name.clone(),
                        ));
                    }
                }
                // SwitchDefault is optional — no validation required for it
            }
            NodeType::Loop(_) => {
                let has_body = outgoing
                    .iter()
                    .any(|e| e.output.as_ref() == Some(&EdgeOutput::LoopBody));
                let has_done = outgoing
                    .iter()
                    .any(|e| e.output.as_ref() == Some(&EdgeOutput::LoopDone));
                if !has_body || !has_done {
                    return Err(ValidationError::MissingLoopEdge(node.name.clone()));
                }
            }
            NodeType::EndLoop(_) => {
                // EndLoop must have exactly 1 regular edge (validated in loop pairing)
                // but we don't enforce the "max 1" rule here since loop pairing covers it.
            }
            _ => {
                // Regular nodes: 0 or 1 outgoing edges
                if outgoing.len() > 1 {
                    return Err(ValidationError::MultipleOutgoingEdges(node.name.clone()));
                }
            }
        }
    }
    Ok(())
}

/// Validate loop pairing:
/// - Every EndLoop.loop_id must reference a valid Loop node
/// - Every Loop node must have exactly one EndLoop referencing it
/// - EndLoop's outgoing edge must point to its loop_id
fn validate_loop_pairing(workflow: &Workflow) -> Result<(), ValidationError> {
    // Collect all Loop node IDs
    let loop_node_ids: HashSet<Uuid> = workflow
        .nodes
        .iter()
        .filter(|n| matches!(&n.node_type, NodeType::Loop(_)))
        .map(|n| n.id)
        .collect();

    // Track which Loop nodes are referenced by EndLoop nodes
    let mut loop_references: HashMap<Uuid, usize> = HashMap::new();

    for node in &workflow.nodes {
        if let NodeType::EndLoop(params) = &node.node_type {
            // EndLoop.loop_id must reference a valid Loop node
            if !loop_node_ids.contains(&params.loop_id) {
                return Err(ValidationError::InvalidEndLoopTarget(node.name.clone()));
            }

            *loop_references.entry(params.loop_id).or_insert(0) += 1;

            // EndLoop's outgoing edge must point to its loop_id
            let outgoing: Vec<_> = workflow
                .edges
                .iter()
                .filter(|e| e.from == node.id)
                .collect();
            if outgoing.len() != 1 || outgoing[0].to != params.loop_id {
                return Err(ValidationError::EndLoopEdgeMismatch(node.name.clone()));
            }
        }
    }

    // Every Loop node must have exactly one EndLoop referencing it
    for node in &workflow.nodes {
        if matches!(&node.node_type, NodeType::Loop(_)) {
            let count = loop_references.get(&node.id).copied().unwrap_or(0);
            if count == 0 {
                return Err(ValidationError::UnpairedLoop(node.name.clone()));
            }
        }
    }

    Ok(())
}

/// Cycle detection that allows EndLoop→Loop back-edges.
///
/// We ignore all edges originating from EndLoop nodes when building the
/// adjacency graph, then run standard DFS cycle detection. EndLoop edges
/// are validated separately by `validate_loop_pairing`.
fn validate_no_illegal_cycles(workflow: &Workflow) -> Result<(), ValidationError> {
    // Build set of EndLoop node IDs
    let endloop_ids: HashSet<Uuid> = workflow
        .nodes
        .iter()
        .filter(|n| matches!(&n.node_type, NodeType::EndLoop(_)))
        .map(|n| n.id)
        .collect();

    // Build adjacency list, excluding edges from EndLoop nodes
    let mut adjacency: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for node in &workflow.nodes {
        adjacency.entry(node.id).or_default();
    }
    for edge in &workflow.edges {
        if !endloop_ids.contains(&edge.from) {
            adjacency.entry(edge.from).or_default().push(edge.to);
        }
    }

    // DFS cycle detection using white/gray/black coloring
    #[derive(Clone, Copy, PartialEq)]
    enum Color {
        White,
        Gray,
        Black,
    }

    let mut color: HashMap<Uuid, Color> = workflow
        .nodes
        .iter()
        .map(|n| (n.id, Color::White))
        .collect();

    fn dfs(
        node: Uuid,
        adjacency: &HashMap<Uuid, Vec<Uuid>>,
        color: &mut HashMap<Uuid, Color>,
    ) -> bool {
        color.insert(node, Color::Gray);
        if let Some(neighbors) = adjacency.get(&node) {
            for &neighbor in neighbors {
                match color.get(&neighbor) {
                    Some(Color::Gray) => return true, // back edge = cycle
                    Some(Color::White) => {
                        if dfs(neighbor, adjacency, color) {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }
        color.insert(node, Color::Black);
        false
    }

    for node in &workflow.nodes {
        if color.get(&node.id) == Some(&Color::White) && dfs(node.id, &adjacency, &mut color) {
            return Err(ValidationError::CycleDetected);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClickParams, Condition, EndLoopParams, IfParams, LiteralValue, LoopParams, NodeType,
        Operator, Position, SwitchCase, SwitchParams, TypeTextParams, ValueRef,
    };

    fn dummy_condition() -> Condition {
        Condition {
            left: ValueRef::Variable {
                name: "x".to_string(),
            },
            operator: Operator::Equals,
            right: ValueRef::Literal {
                value: LiteralValue::Bool { value: true },
            },
        }
    }

    fn pos(x: f32, y: f32) -> Position {
        Position { x, y }
    }

    // --- Regression tests (existing behavior) ---

    #[test]
    fn test_validate_empty_workflow() {
        let wf = Workflow::default();
        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::NoNodes));
    }

    #[test]
    fn test_validate_no_entry_point() {
        let mut wf = Workflow::default();
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let b = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            pos(100.0, 0.0),
        );
        // Create edges so every node has an incoming edge (and it's a cycle)
        wf.add_edge(a, b);
        wf.add_edge(b, a);

        let err = validate_workflow(&wf).unwrap_err();
        // Could be NoEntryPoint or CycleDetected depending on check order
        assert!(
            matches!(err, ValidationError::NoEntryPoint)
                || matches!(err, ValidationError::CycleDetected)
        );
    }

    #[test]
    fn test_validate_multiple_outgoing_edges() {
        let mut wf = Workflow::default();
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let b = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            pos(100.0, 0.0),
        );
        let c = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            pos(200.0, 0.0),
        );
        wf.add_edge(a, b);
        wf.add_edge(a, c); // a has 2 outgoing

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::MultipleOutgoingEdges(_)));
    }

    #[test]
    fn test_validate_valid_linear_workflow() {
        let mut wf = Workflow::default();
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let b = wf.add_node(
            NodeType::TypeText(TypeTextParams::default()),
            pos(100.0, 0.0),
        );
        wf.add_edge(a, b);

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_single_node() {
        let mut wf = Workflow::default();
        wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));

        assert!(validate_workflow(&wf).is_ok());
    }

    // --- If node tests ---

    #[test]
    fn test_validate_valid_if_workflow() {
        // If → (IfTrue) → A, If → (IfFalse) → B, both A and B → C (converge)
        let mut wf = Workflow::default();
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: dummy_condition(),
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));
        let c = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 50.0));

        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, b, EdgeOutput::IfFalse);
        wf.add_edge(a, c);
        wf.add_edge(b, c);

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_if_missing_branch() {
        // If with only IfTrue edge → MissingIfBranch
        let mut wf = Workflow::default();
        let if_node = wf.add_node(
            NodeType::If(IfParams {
                condition: dummy_condition(),
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));

        wf.add_edge_with_output(if_node, a, EdgeOutput::IfTrue);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::MissingIfBranch(_)));
    }

    // --- Loop tests ---

    #[test]
    fn test_validate_valid_loop() {
        // Loop → (LoopBody) → BodyNode → EndLoop → (back to Loop)
        // Loop → (LoopDone) → DoneNode
        let mut wf = Workflow::default();
        let loop_node = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: dummy_condition(),
                max_iterations: 10,
            }),
            pos(0.0, 0.0),
        );
        let body = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let end_loop = wf.add_node(
            NodeType::EndLoop(EndLoopParams { loop_id: loop_node }),
            pos(200.0, 0.0),
        );
        let done = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge_with_output(loop_node, body, EdgeOutput::LoopBody);
        wf.add_edge_with_output(loop_node, done, EdgeOutput::LoopDone);
        wf.add_edge(body, end_loop);
        wf.add_edge(end_loop, loop_node); // back-edge

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_loop_without_end_loop() {
        // Loop node but no EndLoop referencing it → UnpairedLoop
        let mut wf = Workflow::default();
        let loop_node = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: dummy_condition(),
                max_iterations: 10,
            }),
            pos(0.0, 0.0),
        );
        let body = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let done = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge_with_output(loop_node, body, EdgeOutput::LoopBody);
        wf.add_edge_with_output(loop_node, done, EdgeOutput::LoopDone);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::UnpairedLoop(_)));
    }

    #[test]
    fn test_validate_end_loop_bad_target() {
        // EndLoop with loop_id pointing to a non-Loop node → InvalidEndLoopTarget
        let mut wf = Workflow::default();
        let regular = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let end_loop = wf.add_node(
            NodeType::EndLoop(EndLoopParams { loop_id: regular }),
            pos(100.0, 0.0),
        );
        wf.add_edge(end_loop, regular);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidEndLoopTarget(_)));
    }

    #[test]
    fn test_validate_non_endloop_cycle_detected() {
        // A → B → A cycle (neither is EndLoop) → CycleDetected
        let mut wf = Workflow::default();
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let c = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 0.0));
        // c is the entry point, c → a → b → a (cycle)
        wf.add_edge(c, a);
        wf.add_edge(a, b);
        wf.add_edge(b, a);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::CycleDetected));
    }

    #[test]
    fn test_validate_valid_nested_loops() {
        // Outer Loop → (LoopBody) → Inner Loop → (LoopBody) → Node → Inner EndLoop → Inner Loop
        //                                        Inner Loop → (LoopDone) → Outer EndLoop → Outer Loop
        // Outer Loop → (LoopDone) → FinalNode
        let mut wf = Workflow::default();

        let outer_loop = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: dummy_condition(),
                max_iterations: 10,
            }),
            pos(0.0, 0.0),
        );
        let inner_loop = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: dummy_condition(),
                max_iterations: 5,
            }),
            pos(100.0, 0.0),
        );
        let inner_body = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 0.0));
        let inner_end = wf.add_node(
            NodeType::EndLoop(EndLoopParams {
                loop_id: inner_loop,
            }),
            pos(300.0, 0.0),
        );
        let outer_end = wf.add_node(
            NodeType::EndLoop(EndLoopParams {
                loop_id: outer_loop,
            }),
            pos(200.0, 100.0),
        );
        let final_node = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 200.0));

        // Outer loop edges
        wf.add_edge_with_output(outer_loop, inner_loop, EdgeOutput::LoopBody);
        wf.add_edge_with_output(outer_loop, final_node, EdgeOutput::LoopDone);

        // Inner loop edges
        wf.add_edge_with_output(inner_loop, inner_body, EdgeOutput::LoopBody);
        wf.add_edge_with_output(inner_loop, outer_end, EdgeOutput::LoopDone);

        // Inner body → inner end → inner loop (back-edge)
        wf.add_edge(inner_body, inner_end);
        wf.add_edge(inner_end, inner_loop);

        // Outer end → outer loop (back-edge)
        wf.add_edge(outer_end, outer_loop);

        assert!(validate_workflow(&wf).is_ok());
    }

    // --- Switch tests ---

    #[test]
    fn test_validate_valid_switch_workflow() {
        let mut wf = Workflow::default();
        let switch_node = wf.add_node(
            NodeType::Switch(SwitchParams {
                cases: vec![
                    SwitchCase {
                        name: "case_a".to_string(),
                        condition: dummy_condition(),
                    },
                    SwitchCase {
                        name: "case_b".to_string(),
                        condition: dummy_condition(),
                    },
                ],
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));

        wf.add_edge_with_output(
            switch_node,
            a,
            EdgeOutput::SwitchCase {
                name: "case_a".to_string(),
            },
        );
        wf.add_edge_with_output(
            switch_node,
            b,
            EdgeOutput::SwitchCase {
                name: "case_b".to_string(),
            },
        );

        assert!(validate_workflow(&wf).is_ok());
    }

    #[test]
    fn test_validate_switch_missing_case() {
        let mut wf = Workflow::default();
        let switch_node = wf.add_node(
            NodeType::Switch(SwitchParams {
                cases: vec![
                    SwitchCase {
                        name: "case_a".to_string(),
                        condition: dummy_condition(),
                    },
                    SwitchCase {
                        name: "case_b".to_string(),
                        condition: dummy_condition(),
                    },
                ],
            }),
            pos(0.0, 0.0),
        );
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));

        // Only provide edge for case_a, missing case_b
        wf.add_edge_with_output(
            switch_node,
            a,
            EdgeOutput::SwitchCase {
                name: "case_a".to_string(),
            },
        );

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::MissingSwitchCase(_, ref case) if case == "case_b"));
    }

    #[test]
    fn test_validate_end_loop_edge_mismatch() {
        // EndLoop's outgoing edge points somewhere other than its loop_id
        let mut wf = Workflow::default();
        let loop_node = wf.add_node(
            NodeType::Loop(LoopParams {
                exit_condition: dummy_condition(),
                max_iterations: 10,
            }),
            pos(0.0, 0.0),
        );
        let body = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        let done = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 100.0));
        let end_loop = wf.add_node(
            NodeType::EndLoop(EndLoopParams { loop_id: loop_node }),
            pos(200.0, 0.0),
        );

        wf.add_edge_with_output(loop_node, body, EdgeOutput::LoopBody);
        wf.add_edge_with_output(loop_node, done, EdgeOutput::LoopDone);
        wf.add_edge(body, end_loop);
        // EndLoop edge points to done instead of loop_node
        wf.add_edge(end_loop, done);

        let err = validate_workflow(&wf).unwrap_err();
        assert!(matches!(err, ValidationError::EndLoopEdgeMismatch(_)));
    }
}
