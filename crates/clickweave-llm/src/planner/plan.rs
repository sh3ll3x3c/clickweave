use super::mapping::step_to_node_type;
use super::parse::{extract_json, layout_nodes, step_rejected_reason, truncate_intent};
use super::prompt::planner_system_prompt;
use super::repair::chat_with_repair;
use super::{PlanResult, PlannerOutput};
use crate::{ChatBackend, LlmClient, LlmConfig, Message};
use anyhow::{Context, Result, anyhow};
use clickweave_core::{Edge, Node, Workflow, validate_workflow};
use serde_json::Value;
use tracing::{debug, info};
use uuid::Uuid;

/// Plan a workflow from an intent using the planner LLM.
pub async fn plan_workflow(
    intent: &str,
    planner_config: LlmConfig,
    mcp_tools_openai: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<PlanResult> {
    let planner = LlmClient::new(planner_config);
    plan_workflow_with_backend(
        &planner,
        intent,
        mcp_tools_openai,
        allow_ai_transforms,
        allow_agent_steps,
    )
    .await
}

/// Plan a workflow using a given ChatBackend (for testability).
/// On parse or validation failure, retries once with the error message appended.
pub async fn plan_workflow_with_backend(
    backend: &impl ChatBackend,
    intent: &str,
    mcp_tools_openai: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<PlanResult> {
    let system = planner_system_prompt(mcp_tools_openai, allow_ai_transforms, allow_agent_steps);
    let user_msg = format!("Plan a workflow for: {}", intent);

    info!("Planning workflow for intent: {}", intent);
    debug!("Planner system prompt length: {} chars", system.len());

    let messages = vec![Message::system(&system), Message::user(&user_msg)];

    chat_with_repair(backend, "Planner", messages, |content| {
        parse_and_build_workflow(
            content,
            intent,
            mcp_tools_openai,
            allow_ai_transforms,
            allow_agent_steps,
        )
    })
    .await
}

/// Parse planner output JSON and build a workflow.
fn parse_and_build_workflow(
    content: &str,
    intent: &str,
    mcp_tools_openai: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Result<PlanResult> {
    let mut warnings = Vec::new();

    let json_str = extract_json(content);

    let planner_output: PlannerOutput =
        serde_json::from_str(json_str).context("Failed to parse planner output as JSON")?;

    if planner_output.steps.is_empty() {
        return Err(anyhow!("Planner returned no steps"));
    }

    // Filter out rejected steps and collect warnings in a single pass
    let mut steps = Vec::new();
    for step in &planner_output.steps {
        if let Some(reason) = step_rejected_reason(step, allow_ai_transforms, allow_agent_steps) {
            warnings.push(format!("Planner step removed: {}", reason));
            continue;
        }
        steps.push(step);
    }

    if steps.is_empty() {
        return Err(anyhow!(
            "No valid steps after filtering (all were rejected by feature flags)"
        ));
    }

    // Map steps to nodes
    let positions = layout_nodes(steps.len());
    let mut nodes = Vec::new();

    for (i, step) in steps.iter().enumerate() {
        match step_to_node_type(step, mcp_tools_openai) {
            Ok((node_type, display_name)) => {
                nodes.push(Node::new(node_type, positions[i], display_name));
            }
            Err(e) => {
                warnings.push(format!("Step {} skipped: {}", i, e));
            }
        }
    }

    if nodes.is_empty() {
        return Err(anyhow!("No valid nodes produced from planner output"));
    }

    // Build linear edges
    let edges: Vec<Edge> = nodes
        .windows(2)
        .map(|pair| Edge {
            from: pair[0].id,
            to: pair[1].id,
        })
        .collect();

    let workflow = Workflow {
        id: Uuid::new_v4(),
        name: truncate_intent(intent),
        nodes,
        edges,
    };

    // Validate
    validate_workflow(&workflow).context("Generated workflow failed validation")?;

    info!(
        "Planned workflow: {} nodes, {} edges, {} warnings",
        workflow.nodes.len(),
        workflow.edges.len(),
        warnings.len()
    );

    Ok(PlanResult { workflow, warnings })
}
