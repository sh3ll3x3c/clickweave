use super::mapping::step_to_node_type;
use super::parse::{extract_json, id_str_short, step_rejected_reason};
use super::prompt::patcher_system_prompt;
use super::repair::chat_with_repair;
use super::{PatchNodeUpdate, PatchResult, PatcherOutput, PlanStep};
use crate::{ChatBackend, LlmClient, LlmConfig, Message};
use anyhow::{Context, Result};
use clickweave_core::{Edge, Node, NodeType, Position, Workflow, tool_mapping};
use serde_json::Value;
use tracing::info;
use uuid::Uuid;

/// Resolve a `PlanStep` from a `PatchNodeUpdate`.
///
/// Returns `Ok(Some(step))` when the update specifies a node_type change,
/// `Ok(None)` when it doesn't, and `Err(msg)` when parsing/inference fails.
fn resolve_update_step(
    update: &PatchNodeUpdate,
    existing_node_type: &NodeType,
) -> std::result::Result<Option<PlanStep>, String> {
    // Nested format: explicit `node_type` with step_type tag.
    if let Some(nt_value) = &update.node_type {
        return serde_json::from_value::<PlanStep>(nt_value.clone())
            .map(Some)
            .map_err(|e| format!("failed to parse node_type: {}", e));
    }

    // Flat format: `tool_name` and/or `arguments` (LLMs often echo the summary).
    if update.tool_name.is_none() && update.arguments.is_none() {
        return Ok(None);
    }

    let tool_name = update.tool_name.clone().or_else(|| {
        tool_mapping::node_type_to_tool_invocation(existing_node_type)
            .ok()
            .map(|inv| inv.name)
    });

    let tool_name =
        tool_name.ok_or("arguments provided but cannot determine tool_name".to_string())?;

    Ok(Some(PlanStep::Tool {
        tool_name,
        arguments: update
            .arguments
            .clone()
            .unwrap_or(Value::Object(Default::default())),
        name: update.name.clone(),
    }))
}

/// Patch an existing workflow using the planner LLM.
pub async fn patch_workflow(
    workflow: &Workflow,
    user_prompt: &str,
    planner_config: LlmConfig,
    mcp_tools_openai: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<PatchResult> {
    let planner = LlmClient::new(planner_config);
    patch_workflow_with_backend(
        &planner,
        workflow,
        user_prompt,
        mcp_tools_openai,
        allow_ai_transforms,
        allow_agent_steps,
    )
    .await
}

/// Patch a workflow using a given ChatBackend (for testability).
pub async fn patch_workflow_with_backend(
    backend: &impl ChatBackend,
    workflow: &Workflow,
    user_prompt: &str,
    mcp_tools_openai: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<PatchResult> {
    let mut warnings = Vec::new();

    let system = patcher_system_prompt(
        workflow,
        mcp_tools_openai,
        allow_ai_transforms,
        allow_agent_steps,
    );
    let user_msg = format!("Modify the workflow: {}", user_prompt);

    info!("Patching workflow for prompt: {}", user_prompt);

    let messages = vec![Message::system(&system), Message::user(&user_msg)];

    let patcher_output = chat_with_repair(backend, "Patcher", messages, |content| {
        let json_str = extract_json(content);
        serde_json::from_str::<PatcherOutput>(json_str)
            .context("Failed to parse patcher output as JSON")
    })
    .await?;

    // Process added nodes
    let mut added_nodes = Vec::new();
    let last_y = workflow
        .nodes
        .iter()
        .map(|n| n.position.y)
        .fold(0.0_f32, f32::max);

    for (i, step) in patcher_output.add.iter().enumerate() {
        if let Some(reason) = step_rejected_reason(step, allow_ai_transforms, allow_agent_steps) {
            warnings.push(format!("Added step {} removed: {}", i, reason));
            continue;
        }
        match step_to_node_type(step, mcp_tools_openai) {
            Ok((node_type, display_name)) => {
                let position = Position {
                    x: 300.0,
                    y: last_y + 120.0 + (i as f32) * 120.0,
                };
                added_nodes.push(Node::new(node_type, position, display_name));
            }
            Err(e) => {
                warnings.push(format!("Added step {} skipped: {}", i, e));
            }
        }
    }

    // Process removed nodes
    let mut removed_node_ids = Vec::new();
    for id_str in &patcher_output.remove_node_ids {
        let id = match id_str.parse::<Uuid>() {
            Ok(id) => id,
            Err(_) => {
                warnings.push(format!("Remove: invalid node ID: {}", id_str));
                continue;
            }
        };
        if workflow.nodes.iter().any(|n| n.id == id) {
            removed_node_ids.push(id);
        } else {
            warnings.push(format!("Remove: node {} not found in workflow", id_str));
        }
    }

    // Process updated nodes
    let mut updated_nodes = Vec::new();
    for update in &patcher_output.update {
        let id = match update.node_id.parse::<Uuid>() {
            Ok(id) => id,
            Err(_) => {
                warnings.push(format!("Update: invalid node ID: {}", update.node_id));
                continue;
            }
        };
        let Some(existing) = workflow.nodes.iter().find(|n| n.id == id) else {
            warnings.push(format!("Update: node {} not found", update.node_id));
            continue;
        };
        let mut node = existing.clone();
        if let Some(name) = &update.name {
            node.name = name.clone();
        }
        let short_id = id_str_short(&id);
        match resolve_update_step(update, &existing.node_type) {
            Ok(Some(step)) => {
                if let Some(reason) =
                    step_rejected_reason(&step, allow_ai_transforms, allow_agent_steps)
                {
                    warnings.push(format!("Update {}: {}", short_id, reason));
                } else {
                    match step_to_node_type(&step, mcp_tools_openai) {
                        Ok((node_type, _)) => node.node_type = node_type,
                        Err(e) => warnings.push(format!("Update {}: {}", short_id, e)),
                    }
                }
            }
            Ok(None) => {} // name-only update, no node_type change
            Err(msg) => warnings.push(format!("Update {}: {}", short_id, msg)),
        }
        updated_nodes.push(node);
    }

    // Build added edges: connect last existing node to first added node,
    // then chain added nodes
    let mut added_edges = Vec::new();
    let mut removed_edges = Vec::new();

    if !added_nodes.is_empty() {
        // Find the last node that isn't removed
        let last_existing = workflow
            .nodes
            .iter()
            .rev()
            .find(|n| !removed_node_ids.contains(&n.id));

        if let Some(last) = last_existing {
            added_edges.push(Edge {
                from: last.id,
                to: added_nodes[0].id,
            });
        }

        // Chain added nodes
        for pair in added_nodes.windows(2) {
            added_edges.push(Edge {
                from: pair[0].id,
                to: pair[1].id,
            });
        }
    }

    // Remove edges to/from removed nodes
    for edge in &workflow.edges {
        if removed_node_ids.contains(&edge.from) || removed_node_ids.contains(&edge.to) {
            removed_edges.push(edge.clone());
        }
    }

    info!(
        "Patch: +{} nodes, -{} nodes, ~{} nodes, +{} edges, -{} edges, {} warnings",
        added_nodes.len(),
        removed_node_ids.len(),
        updated_nodes.len(),
        added_edges.len(),
        removed_edges.len(),
        warnings.len(),
    );

    Ok(PatchResult {
        added_nodes,
        removed_node_ids,
        updated_nodes,
        added_edges,
        removed_edges,
        warnings,
    })
}
