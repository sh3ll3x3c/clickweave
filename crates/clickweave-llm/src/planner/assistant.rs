use super::conversation::{ChatRole, ConversationSession};
use super::mapping::step_to_node_type;
use super::parse::{extract_json, id_str_short, layout_nodes, step_rejected_reason};
use super::prompt::assistant_system_prompt;
use super::summarize::summarize_overflow;
use super::{PatchNodeUpdate, PatchResult, PatcherOutput, PlanStep, PlannerOutput};
use crate::{ChatBackend, LlmClient, LlmConfig, Message};
use anyhow::Result;
use clickweave_core::{Edge, Node, NodeType, Position, Workflow, tool_mapping};
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

/// Result of an assistant chat turn.
pub struct AssistantResult {
    /// Natural language response.
    pub message: String,
    /// Workflow changes, if any.
    pub patch: Option<PatchResult>,
    /// Updated summary if summarization was triggered.
    pub new_summary: Option<String>,
    /// Warnings from step processing.
    pub warnings: Vec<String>,
}

/// Chat with the assistant, creating an LlmClient from config.
#[allow(clippy::too_many_arguments)]
pub async fn assistant_chat(
    workflow: &Workflow,
    user_message: &str,
    session: &ConversationSession,
    run_context_text: Option<&str>,
    config: LlmConfig,
    mcp_tools: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<AssistantResult> {
    let client = LlmClient::new(config);
    assistant_chat_with_backend(
        &client,
        workflow,
        user_message,
        session,
        run_context_text,
        mcp_tools,
        allow_ai_transforms,
        allow_agent_steps,
    )
    .await
}

/// Chat with the assistant using a given ChatBackend (for testability).
#[allow(clippy::too_many_arguments)]
pub async fn assistant_chat_with_backend(
    backend: &impl ChatBackend,
    workflow: &Workflow,
    user_message: &str,
    session: &ConversationSession,
    run_context_text: Option<&str>,
    mcp_tools: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<AssistantResult> {
    // 1. Optionally summarize overflow (non-fatal on error)
    let new_summary = if session.needs_summarization(None) {
        match summarize_overflow(backend, session, None).await {
            Ok(summary) if !summary.is_empty() => {
                info!("Summarized conversation overflow");
                Some(summary)
            }
            Ok(_) => None,
            Err(e) => {
                warn!("Summarization failed (non-fatal): {}", e);
                None
            }
        }
    } else {
        None
    };

    // 2. Build system prompt
    let system = assistant_system_prompt(
        workflow,
        mcp_tools,
        allow_ai_transforms,
        allow_agent_steps,
        run_context_text,
    );

    // 3. Assemble messages: system + optional summary context + recent window + new user message
    let mut messages = vec![Message::system(&system)];

    // Inject summary context if available (prefer new summary, fall back to existing)
    let effective_summary = new_summary.as_deref().or(session.summary.as_deref());
    if let Some(summary) = effective_summary {
        messages.push(Message::user(format!(
            "Conversation context (summary of earlier discussion): {}",
            summary
        )));
        messages.push(Message::assistant(
            "Understood, I have the context from our earlier discussion.".to_string(),
        ));
    }

    // Add recent conversation window
    for entry in session.recent_window(None) {
        let msg = match entry.role {
            ChatRole::User => Message::user(&entry.content),
            ChatRole::Assistant => Message::assistant(&entry.content),
        };
        messages.push(msg);
    }

    // Add the new user message
    messages.push(Message::user(user_message));

    // 4. Call the LLM (single call, no repair)
    let response = backend.chat(messages, None).await?;

    let content = response
        .choices
        .first()
        .and_then(|c| c.message.content_text())
        .unwrap_or("")
        .to_string();

    // 5. Try to parse the response
    let (message, patch, warnings) = parse_assistant_response(
        &content,
        workflow,
        mcp_tools,
        allow_ai_transforms,
        allow_agent_steps,
    );

    info!(
        has_patch = patch.is_some(),
        warnings = warnings.len(),
        "Assistant response processed"
    );

    Ok(AssistantResult {
        message,
        patch,
        new_summary,
        warnings,
    })
}

/// Try to parse the LLM response as a patch, plan, or conversational text.
///
/// For existing workflows (non-empty), tries PatcherOutput first.
/// For empty workflows, tries PlannerOutput first.
/// Falls back to treating the whole response as conversational.
fn parse_assistant_response(
    content: &str,
    workflow: &Workflow,
    mcp_tools: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> (String, Option<PatchResult>, Vec<String>) {
    let json_str = extract_json(content);

    if !workflow.nodes.is_empty() {
        // Try parsing as PatcherOutput first
        if let Ok(output) = serde_json::from_str::<PatcherOutput>(json_str) {
            // If all arrays are empty, treat as conversational
            if output.add.is_empty()
                && output.remove_node_ids.is_empty()
                && output.update.is_empty()
            {
                return (content.to_string(), None, Vec::new());
            }
            let prose = extract_prose(content);
            let (patch, warnings) = build_patch_result(
                &output,
                workflow,
                mcp_tools,
                allow_ai_transforms,
                allow_agent_steps,
            );
            let message = prose.unwrap_or_else(|| describe_patch(&patch));
            return (message, Some(patch), warnings);
        }
    }

    // Try parsing as PlannerOutput (for empty workflows or if patch parsing fails)
    if let Ok(output) = serde_json::from_str::<PlannerOutput>(json_str)
        && !output.steps.is_empty()
    {
        let prose = extract_prose(content);
        let (patch, warnings) = build_plan_as_patch(
            &output.steps,
            mcp_tools,
            allow_ai_transforms,
            allow_agent_steps,
        );
        let message = prose.unwrap_or_else(|| describe_patch(&patch));
        return (message, Some(patch), warnings);
    }

    // Conversational response
    (content.to_string(), None, Vec::new())
}

/// Extract prose text before a JSON block, if any.
fn extract_prose(content: &str) -> Option<String> {
    // Check if there's text before a JSON object or code fence
    let trimmed = content.trim();

    // Look for start of JSON block
    let json_start = trimmed.find("```").or_else(|| {
        // Find the first `{` that starts a JSON object
        trimmed.find('{').filter(|&pos| pos > 0)
    });

    if let Some(pos) = json_start {
        let prose = trimmed[..pos].trim();
        if !prose.is_empty() {
            return Some(prose.to_string());
        }
    }

    None
}

/// Generate a default description of what a patch does.
fn describe_patch(patch: &PatchResult) -> String {
    let mut parts = Vec::new();
    if !patch.added_nodes.is_empty() {
        parts.push(format!(
            "Added {} node{}",
            patch.added_nodes.len(),
            if patch.added_nodes.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if !patch.removed_node_ids.is_empty() {
        parts.push(format!(
            "Removed {} node{}",
            patch.removed_node_ids.len(),
            if patch.removed_node_ids.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if !patch.updated_nodes.is_empty() {
        parts.push(format!(
            "Updated {} node{}",
            patch.updated_nodes.len(),
            if patch.updated_nodes.len() == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    if parts.is_empty() {
        "No workflow changes.".to_string()
    } else {
        format!("{}.", parts.join(", "))
    }
}

/// Process PatcherOutput into PatchResult (duplicates logic from patch.rs).
fn build_patch_result(
    output: &PatcherOutput,
    workflow: &Workflow,
    mcp_tools: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> (PatchResult, Vec<String>) {
    let mut warnings = Vec::new();

    // Process added nodes
    let mut added_nodes = Vec::new();
    let last_y = workflow
        .nodes
        .iter()
        .map(|n| n.position.y)
        .fold(0.0_f32, f32::max);

    for (i, step) in output.add.iter().enumerate() {
        if let Some(reason) = step_rejected_reason(step, allow_ai_transforms, allow_agent_steps) {
            warnings.push(format!("Added step {} removed: {}", i, reason));
            continue;
        }
        match step_to_node_type(step, mcp_tools) {
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
    for id_str in &output.remove_node_ids {
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
    for update in &output.update {
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
                    match step_to_node_type(&step, mcp_tools) {
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

    (
        PatchResult {
            added_nodes,
            removed_node_ids,
            updated_nodes,
            added_edges,
            removed_edges,
            warnings: warnings.clone(),
        },
        warnings,
    )
}

/// Convert planner steps into a PatchResult (all adds).
fn build_plan_as_patch(
    steps: &[PlanStep],
    mcp_tools: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> (PatchResult, Vec<String>) {
    let mut warnings = Vec::new();
    let mut valid_steps = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        if let Some(reason) = step_rejected_reason(step, allow_ai_transforms, allow_agent_steps) {
            warnings.push(format!("Step {} removed: {}", i, reason));
            continue;
        }
        valid_steps.push(step);
    }

    let positions = layout_nodes(valid_steps.len());
    let mut added_nodes = Vec::new();

    for (i, step) in valid_steps.iter().enumerate() {
        match step_to_node_type(step, mcp_tools) {
            Ok((node_type, display_name)) => {
                added_nodes.push(Node::new(node_type, positions[i], display_name));
            }
            Err(e) => {
                warnings.push(format!("Step {} skipped: {}", i, e));
            }
        }
    }

    // Build linear edges between added nodes
    let added_edges: Vec<Edge> = added_nodes
        .windows(2)
        .map(|pair| Edge {
            from: pair[0].id,
            to: pair[1].id,
        })
        .collect();

    (
        PatchResult {
            added_nodes,
            removed_node_ids: Vec::new(),
            updated_nodes: Vec::new(),
            added_edges,
            removed_edges: Vec::new(),
            warnings: warnings.clone(),
        },
        warnings,
    )
}

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
