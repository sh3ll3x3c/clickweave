use std::collections::HashMap;

use uuid::Uuid;

use crate::output_schema::NodeContext;
use crate::{Edge, Node, NodeType, Workflow};

/// Validation warning (not error) for CDP nodes without a CDP scope.
#[derive(Debug, Clone)]
pub struct CdpScopeWarning {
    pub node_name: String,
    pub node_id: Uuid,
    pub message: String,
}

/// Check that each CDP node has an upstream FocusWindow targeting a CDP-capable
/// app in its execution path. Returns warnings, not errors.
///
/// Uses path-aware DFS: when walking backwards through an If or Switch node,
/// only the branch that actually leads to the CDP node is considered, so a
/// FocusWindow in an unrelated branch is not mistaken for coverage.
pub(crate) fn validate_cdp_scope(workflow: &Workflow) -> Vec<CdpScopeWarning> {
    let mut warnings = Vec::new();

    let node_map: HashMap<Uuid, &Node> = workflow.nodes.iter().map(|n| (n.id, n)).collect();

    // Build reverse adjacency: child_id -> [(parent_id, &Edge)]
    let mut reverse_edges: HashMap<Uuid, Vec<(Uuid, &Edge)>> = HashMap::new();
    for edge in &workflow.edges {
        reverse_edges
            .entry(edge.to)
            .or_default()
            .push((edge.from, edge));
    }

    // Shared memo across all CDP nodes — avoids re-traversing common ancestor paths
    let mut memo: HashMap<Uuid, bool> = HashMap::new();

    for node in &workflow.nodes {
        if node.node_type.node_context() != NodeContext::Cdp {
            continue;
        }

        let found = all_paths_have_cdp_scope(node.id, &node_map, &reverse_edges, &mut memo);

        if !found {
            warnings.push(CdpScopeWarning {
                node_name: node.name.clone(),
                node_id: node.id,
                message: format!(
                    "{} may execute without a CDP app focused. \
                     Add a FocusWindow targeting Chrome or an Electron app before it.",
                    node.node_type.display_name()
                ),
            });
        }
    }

    warnings
}

/// Returns true if ALL execution paths reaching `node_id` pass through a
/// CDP-capable FocusWindow (without an intervening non-CDP FocusWindow or
/// QuitApp that would break the scope).
///
/// For branching nodes (If/Switch), when we reach one from a child, we only
/// consider the branch output that connects to that child. The branching
/// node's own predecessors are then checked normally (these are shared by all
/// branches and represent the common path before the branch point).
fn all_paths_have_cdp_scope(
    node_id: Uuid,
    node_map: &HashMap<Uuid, &Node>,
    reverse_edges: &HashMap<Uuid, Vec<(Uuid, &Edge)>>,
    memo: &mut HashMap<Uuid, bool>,
) -> bool {
    if let Some(&cached) = memo.get(&node_id) {
        return cached;
    }

    // Temporarily mark as true to break cycles (loop back-edges).
    // If we re-encounter this node during recursion, we assume the loop
    // body doesn't need an additional FocusWindow (the scope from before
    // the loop still holds).
    memo.insert(node_id, true);

    let result = check_node_cdp_scope(node_id, node_map, reverse_edges, memo);
    memo.insert(node_id, result);
    result
}

fn check_node_cdp_scope(
    node_id: Uuid,
    node_map: &HashMap<Uuid, &Node>,
    reverse_edges: &HashMap<Uuid, Vec<(Uuid, &Edge)>>,
    memo: &mut HashMap<Uuid, bool>,
) -> bool {
    let Some(node) = node_map.get(&node_id) else {
        return false;
    };

    // FocusWindow with a CDP-capable app_kind establishes scope.
    // LaunchApp is transparent — the executor detects app kind at runtime,
    // so at validation time we can't know if it targets a CDP app. Skip it
    // and continue walking predecessors.
    match &node.node_type {
        NodeType::FocusWindow(p) if p.app_kind.uses_cdp() => return true,
        NodeType::FocusWindow(_) => return false, // Non-CDP focus breaks scope
        NodeType::QuitApp(_) => return false,
        _ => {}
    }

    let preds = match reverse_edges.get(&node_id) {
        Some(preds) if !preds.is_empty() => preds,
        _ => {
            // Entry point with no predecessors -- no FocusWindow found
            return false;
        }
    };

    // ALL predecessor paths must have CDP scope, since any could be the
    // actual execution path leading to this node.
    check_predecessors_branch_aware(preds, node_map, reverse_edges, memo)
}

/// Check predecessors with branch awareness.
///
/// When a node has multiple predecessors (e.g., after an If/Switch merge),
/// ALL paths must have CDP scope since any of them could be the actual
/// execution path.
///
/// When a predecessor is an If/Switch node connected via a branch output
/// edge, we skip into the If/Switch's own predecessors (the common path
/// before the branch point) instead of evaluating other branches.
fn check_predecessors_branch_aware(
    preds: &[(Uuid, &Edge)],
    node_map: &HashMap<Uuid, &Node>,
    reverse_edges: &HashMap<Uuid, Vec<(Uuid, &Edge)>>,
    memo: &mut HashMap<Uuid, bool>,
) -> bool {
    for &(pred_id, edge) in preds {
        let Some(pred_node) = node_map.get(&pred_id) else {
            return false;
        };

        let pred_has_scope = match &pred_node.node_type {
            NodeType::If(_) | NodeType::Switch(_) if edge.output.is_some() => {
                // Arrived via a branch output edge — only the branching node's
                // own predecessors matter (skip into the common path).
                all_paths_have_cdp_scope(pred_id, node_map, reverse_edges, memo)
            }
            _ => all_paths_have_cdp_scope(pred_id, node_map, reverse_edges, memo),
        };

        if !pred_has_scope {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::super::ValidationWarning;
    use super::super::test_helpers::pos;
    use super::super::validate_workflow;
    use crate::{
        AppKind, CdpClickParams, ClickParams, EdgeOutput, FocusMethod, FocusWindowParams, IfParams,
        NodeType, QuitAppParams, Workflow,
    };

    #[test]
    fn cdp_node_with_upstream_chrome_focus_no_warning() {
        let mut wf = Workflow::default();
        let focus = wf.add_node(
            NodeType::FocusWindow(FocusWindowParams {
                method: FocusMethod::AppName,
                value: Some("Google Chrome".to_string()),
                bring_to_front: true,
                app_kind: AppKind::ChromeBrowser,
                ..Default::default()
            }),
            pos(0.0, 0.0),
        );
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(100.0, 0.0),
        );
        wf.add_edge(focus, cdp);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert!(result.warnings.is_empty(), "expected no warnings");
    }

    #[test]
    fn cdp_node_without_upstream_focus_warns() {
        let mut wf = Workflow::default();
        wf.add_node(NodeType::CdpClick(CdpClickParams::default()), pos(0.0, 0.0));

        let result = validate_workflow(&wf).expect("should pass validation");
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].message().contains("CDP app focused"));
    }

    #[test]
    fn cdp_node_after_native_focus_warns() {
        let mut wf = Workflow::default();
        let focus = wf.add_node(
            NodeType::FocusWindow(FocusWindowParams {
                method: FocusMethod::AppName,
                value: Some("Calculator".to_string()),
                bring_to_front: true,
                app_kind: AppKind::Native,
                ..Default::default()
            }),
            pos(0.0, 0.0),
        );
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(100.0, 0.0),
        );
        wf.add_edge(focus, cdp);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn cdp_scope_broken_by_quit_app() {
        let mut wf = Workflow::default();
        let focus = wf.add_node(
            NodeType::FocusWindow(FocusWindowParams {
                method: FocusMethod::AppName,
                value: Some("Google Chrome".to_string()),
                bring_to_front: true,
                app_kind: AppKind::ChromeBrowser,
                ..Default::default()
            }),
            pos(0.0, 0.0),
        );
        let quit = wf.add_node(
            NodeType::QuitApp(QuitAppParams {
                app_name: "Google Chrome".to_string(),
                ..Default::default()
            }),
            pos(100.0, 0.0),
        );
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(200.0, 0.0),
        );
        wf.add_edge(focus, quit);
        wf.add_edge(quit, cdp);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert_eq!(result.warnings.len(), 1);
    }

    #[test]
    fn native_only_workflow_no_warnings() {
        let mut wf = Workflow::default();
        let a = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let b = wf.add_node(NodeType::Click(ClickParams::default()), pos(100.0, 0.0));
        wf.add_edge(a, b);

        let result = validate_workflow(&wf).expect("should pass validation");
        assert!(result.warnings.is_empty());
    }

    fn chrome_focus() -> NodeType {
        NodeType::FocusWindow(FocusWindowParams {
            method: FocusMethod::AppName,
            value: Some("Google Chrome".to_string()),
            bring_to_front: true,
            app_kind: AppKind::ChromeBrowser,
            ..Default::default()
        })
    }

    fn dummy_if_condition() -> IfParams {
        use crate::output_schema::{ConditionValue, OutputRef};
        use crate::{LiteralValue, Operator};

        IfParams {
            condition: crate::Condition {
                left: OutputRef {
                    node: "click_1".to_string(),
                    field: "result".to_string(),
                },
                operator: Operator::Equals,
                right: ConditionValue::Literal {
                    value: LiteralValue::Bool { value: true },
                },
            },
        }
    }

    fn cdp_warnings(result: &super::super::ValidationResult) -> Vec<&ValidationWarning> {
        result
            .warnings
            .iter()
            .filter(|w| matches!(w, ValidationWarning::Cdp(_)))
            .collect()
    }

    #[test]
    fn cdp_in_if_true_with_focus_only_in_if_false_warns() {
        // FocusWindow(Chrome) is only in IfFalse -- CdpClick in IfTrue should warn
        //
        //   Click -> If
        //            +-- IfTrue  -> CdpClick       (no focus!)
        //            +-- IfFalse -> FocusWindow(Chrome) -> Click
        let mut wf = Workflow::default();
        let click = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let if_node = wf.add_node(NodeType::If(dummy_if_condition()), pos(100.0, 0.0));
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(200.0, 0.0),
        );
        let focus = wf.add_node(chrome_focus(), pos(200.0, 100.0));
        let end = wf.add_node(NodeType::Click(ClickParams::default()), pos(300.0, 100.0));

        wf.add_edge(click, if_node);
        wf.add_edge_with_output(if_node, cdp, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, focus, EdgeOutput::IfFalse);
        wf.add_edge(focus, end);

        let result = validate_workflow(&wf).expect("should pass validation");
        let cdp_warns = cdp_warnings(&result);
        assert_eq!(
            cdp_warns.len(),
            1,
            "CdpClick in IfTrue branch without focus should warn"
        );
    }

    #[test]
    fn cdp_in_if_true_with_upstream_focus_no_warning() {
        // FocusWindow(Chrome) BEFORE the If -- both branches have scope
        //
        //   FocusWindow(Chrome) -> If
        //                          +-- IfTrue  -> CdpClick
        //                          +-- IfFalse -> Click
        let mut wf = Workflow::default();
        let focus = wf.add_node(chrome_focus(), pos(0.0, 0.0));
        let if_node = wf.add_node(NodeType::If(dummy_if_condition()), pos(100.0, 0.0));
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(200.0, 0.0),
        );
        let click = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 100.0));

        wf.add_edge(focus, if_node);
        wf.add_edge_with_output(if_node, cdp, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, click, EdgeOutput::IfFalse);

        let result = validate_workflow(&wf).expect("should pass validation");
        let cdp_warns = cdp_warnings(&result);
        assert!(
            cdp_warns.is_empty(),
            "CdpClick with upstream focus before If should not warn"
        );
    }

    #[test]
    fn cdp_after_if_merge_warns_when_one_branch_lacks_focus() {
        // After If merge, one branch has focus and one does not -- should warn
        //
        //   Click -> If
        //            +-- IfTrue  -> Click --------+
        //            +-- IfFalse -> FocusWindow ---+-> CdpClick
        let mut wf = Workflow::default();
        let entry = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let if_node = wf.add_node(NodeType::If(dummy_if_condition()), pos(100.0, 0.0));
        let true_click = wf.add_node(NodeType::Click(ClickParams::default()), pos(200.0, 0.0));
        let false_focus = wf.add_node(chrome_focus(), pos(200.0, 100.0));
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(300.0, 50.0),
        );

        wf.add_edge(entry, if_node);
        wf.add_edge_with_output(if_node, true_click, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, false_focus, EdgeOutput::IfFalse);
        wf.add_edge(true_click, cdp);
        wf.add_edge(false_focus, cdp);

        let result = validate_workflow(&wf).expect("should pass validation");
        let cdp_warns = cdp_warnings(&result);
        assert_eq!(
            cdp_warns.len(),
            1,
            "CdpClick after merge should warn when IfTrue branch lacks focus"
        );
    }

    #[test]
    fn cdp_after_if_merge_no_warning_when_both_branches_have_focus() {
        // After If merge, both branches have focus -- no warning
        //
        //   Click -> If
        //            +-- IfTrue  -> FocusWindow ---+
        //            +-- IfFalse -> FocusWindow ---+-> CdpClick
        let mut wf = Workflow::default();
        let entry = wf.add_node(NodeType::Click(ClickParams::default()), pos(0.0, 0.0));
        let if_node = wf.add_node(NodeType::If(dummy_if_condition()), pos(100.0, 0.0));
        let true_focus = wf.add_node(chrome_focus(), pos(200.0, 0.0));
        let false_focus = wf.add_node(chrome_focus(), pos(200.0, 100.0));
        let cdp = wf.add_node(
            NodeType::CdpClick(CdpClickParams::default()),
            pos(300.0, 50.0),
        );

        wf.add_edge(entry, if_node);
        wf.add_edge_with_output(if_node, true_focus, EdgeOutput::IfTrue);
        wf.add_edge_with_output(if_node, false_focus, EdgeOutput::IfFalse);
        wf.add_edge(true_focus, cdp);
        wf.add_edge(false_focus, cdp);

        let result = validate_workflow(&wf).expect("should pass validation");
        let cdp_warns = cdp_warnings(&result);
        assert!(
            cdp_warns.is_empty(),
            "CdpClick after merge should not warn when both branches have focus"
        );
    }
}
