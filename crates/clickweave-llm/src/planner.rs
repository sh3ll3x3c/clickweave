use crate::{ChatBackend, ChatResponse, LlmClient, LlmConfig, Message};
use anyhow::{Context, Result, anyhow};
use clickweave_core::{
    AiStepParams, ClickParams, Edge, FindImageParams, FindTextParams, FocusMethod,
    FocusWindowParams, ListWindowsParams, McpToolCallParams, MouseButton, Node, NodeType, Position,
    PressKeyParams, ScreenshotMode, ScrollParams, TakeScreenshotParams, TypeTextParams, Workflow,
    validate_workflow,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info};
use uuid::Uuid;

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

/// Build the planner system prompt.
fn planner_system_prompt(
    tools_json: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> String {
    let tool_list = serde_json::to_string_pretty(tools_json).unwrap_or_default();

    let mut step_types = r#"Available step types:

1. **Tool** — calls exactly one MCP tool:
   ```json
   {"step_type": "Tool", "tool_name": "<name>", "arguments": {...}, "name": "optional label"}
   ```
   The arguments must be valid according to the tool's input schema."#
        .to_string();

    if allow_ai_transforms {
        step_types.push_str(
            r#"

2. **AiTransform** — bounded AI operation (summarize, extract, classify) with no tool access:
   ```json
   {"step_type": "AiTransform", "kind": "summarize|extract|classify", "input_ref": "<step_name>", "output_schema": {...}, "name": "optional label"}
   ```"#,
        );
    }

    if allow_agent_steps {
        step_types.push_str(
            r#"

3. **AiStep** — agentic loop with tool access (use sparingly, only when the task genuinely requires dynamic decision-making):
   ```json
   {"step_type": "AiStep", "prompt": "<what to accomplish>", "allowed_tools": ["tool1", "tool2"], "max_tool_calls": 10, "name": "optional label"}
   ```"#,
        );
    }

    format!(
        r#"You are a workflow planner for UI automation. Given a user's intent, produce a sequence of steps that accomplish the goal.

You have access to these MCP tools:

{tool_list}

{step_types}

Rules:
- Output ONLY a JSON object: {{"steps": [...]}}
- Each Tool step must use exactly one tool from the list above with schema-valid arguments.
- Steps execute in sequence (output of one step is available to the next).
- Be precise: use find_text to locate UI elements before clicking them.
- For clicking on text elements: first use find_text to get coordinates, then use click with those coordinates.
- Always focus the target window before interacting with it.
- Prefer deterministic Tool steps over AiStep whenever possible.
- Do not add unnecessary steps. Be efficient."#,
    )
}

/// Map a PlanStep to a NodeType.
fn step_to_node_type(step: &PlanStep, tools: &[Value]) -> Result<(NodeType, String)> {
    match step {
        PlanStep::Tool {
            tool_name,
            arguments,
            name,
        } => {
            let display = name.clone().unwrap_or_else(|| tool_name.replace('_', " "));

            // Try to map to a typed node, fall back to McpToolCall
            let node_type = match tool_name.as_str() {
                "take_screenshot" => {
                    let mode = match arguments.get("mode").and_then(|v| v.as_str()) {
                        Some("screen") => ScreenshotMode::Screen,
                        Some("region") => ScreenshotMode::Region,
                        _ => ScreenshotMode::Window,
                    };
                    NodeType::TakeScreenshot(TakeScreenshotParams {
                        mode,
                        target: arguments
                            .get("app_name")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        include_ocr: arguments
                            .get("include_ocr")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(true),
                    })
                }
                "find_text" => {
                    let text = arguments
                        .get("text")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow!("find_text requires non-empty 'text' argument"))?;
                    NodeType::FindText(FindTextParams {
                        search_text: text.to_string(),
                        ..Default::default()
                    })
                }
                "find_image" => NodeType::FindImage(FindImageParams {
                    template_image: arguments
                        .get("template_image_base64")
                        .or_else(|| arguments.get("template_id"))
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    threshold: arguments
                        .get("threshold")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.75),
                    max_results: arguments
                        .get("max_results")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(3) as u32,
                }),
                "click" => NodeType::Click(ClickParams {
                    x: arguments.get("x").and_then(|v| v.as_f64()),
                    y: arguments.get("y").and_then(|v| v.as_f64()),
                    button: match arguments.get("button").and_then(|v| v.as_str()) {
                        Some("right") => MouseButton::Right,
                        Some("center") => MouseButton::Center,
                        _ => MouseButton::Left,
                    },
                    click_count: arguments
                        .get("click_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1) as u32,
                }),
                "type_text" => {
                    let text = arguments
                        .get("text")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow!("type_text requires non-empty 'text' argument"))?;
                    NodeType::TypeText(TypeTextParams {
                        text: text.to_string(),
                    })
                }
                "press_key" => {
                    let key = arguments
                        .get("key")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| anyhow!("press_key requires non-empty 'key' argument"))?;
                    NodeType::PressKey(PressKeyParams {
                        key: key.to_string(),
                        modifiers: arguments
                            .get("modifiers")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                }
                "scroll" => NodeType::Scroll(ScrollParams {
                    delta_y: arguments
                        .get("delta_y")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32,
                    x: arguments.get("x").and_then(|v| v.as_f64()),
                    y: arguments.get("y").and_then(|v| v.as_f64()),
                }),
                "list_windows" => NodeType::ListWindows(ListWindowsParams {
                    app_name: arguments
                        .get("app_name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                }),
                "focus_window" => {
                    let (method, value) = if let Some(app) =
                        arguments.get("app_name").and_then(|v| v.as_str())
                    {
                        (FocusMethod::AppName, Some(app.to_string()))
                    } else if let Some(wid) = arguments.get("window_id").and_then(|v| v.as_u64()) {
                        (FocusMethod::WindowId, Some(wid.to_string()))
                    } else if let Some(pid) = arguments.get("pid").and_then(|v| v.as_u64()) {
                        (FocusMethod::Pid, Some(pid.to_string()))
                    } else {
                        (FocusMethod::AppName, None)
                    };
                    NodeType::FocusWindow(FocusWindowParams {
                        method,
                        value,
                        bring_to_front: true,
                    })
                }
                // Catch-all: use McpToolCall for unknown tools
                _ => {
                    // Verify the tool actually exists
                    let tool_exists = tools
                        .iter()
                        .any(|t| t["function"]["name"].as_str() == Some(tool_name));
                    if !tool_exists {
                        return Err(anyhow!("Unknown tool: {}", tool_name));
                    }
                    NodeType::McpToolCall(McpToolCallParams {
                        tool_name: tool_name.clone(),
                        arguments: arguments.clone(),
                    })
                }
            };

            Ok((node_type, display))
        }
        PlanStep::AiTransform { name, kind, .. } => {
            let display = name
                .clone()
                .unwrap_or_else(|| format!("AI Transform ({})", kind));
            // Map AI transforms to an AiStep with no tools
            Ok((
                NodeType::AiStep(AiStepParams {
                    prompt: format!("Perform a '{}' transform on the input.", kind),
                    allowed_tools: Some(vec![]),
                    max_tool_calls: Some(0),
                    ..Default::default()
                }),
                display,
            ))
        }
        PlanStep::AiStep {
            prompt,
            allowed_tools,
            max_tool_calls,
            timeout_ms,
            name,
        } => {
            let display = name.clone().unwrap_or_else(|| "AI Step".to_string());
            Ok((
                NodeType::AiStep(AiStepParams {
                    prompt: prompt.clone(),
                    allowed_tools: allowed_tools.clone(),
                    max_tool_calls: *max_tool_calls,
                    timeout_ms: *timeout_ms,
                    ..Default::default()
                }),
                display,
            ))
        }
    }
}

/// Lay out nodes in a vertical chain.
fn layout_nodes(count: usize) -> Vec<Position> {
    (0..count)
        .map(|i| Position {
            x: 300.0,
            y: 100.0 + (i as f32) * 120.0,
        })
        .collect()
}

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

const MAX_REPAIR_ATTEMPTS: usize = 1;

/// Chat with the LLM, retrying once with error feedback on failure.
/// `label` is used for log messages (e.g. "Planner", "Patcher").
/// `process` receives the raw text content and returns Ok(T) or Err to trigger a repair.
async fn chat_with_repair<T>(
    backend: &impl ChatBackend,
    label: &str,
    messages: Vec<Message>,
    mut process: impl FnMut(&str) -> Result<T>,
) -> Result<T> {
    let mut messages = messages;
    let mut last_error: Option<String> = None;

    for attempt in 0..=MAX_REPAIR_ATTEMPTS {
        if let Some(ref err) = last_error {
            info!("Repair attempt {} for {} error: {}", attempt, label, err);
            messages.push(Message::user(format!(
                "Your previous output had an error: {}\n\nPlease fix the JSON and try again. Output ONLY the corrected JSON object.",
                err
            )));
        }

        let response: ChatResponse = backend
            .chat(messages.clone(), None)
            .await
            .context(format!("{} LLM call failed", label))?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No response from {}", label.to_lowercase()))?;

        let content = choice
            .message
            .text_content()
            .ok_or_else(|| anyhow!("{} returned no text content", label))?;

        debug!("{} raw output (attempt {}): {}", label, attempt, content);

        messages.push(Message::assistant(content));

        match process(content) {
            Ok(result) => return Ok(result),
            Err(e) if attempt < MAX_REPAIR_ATTEMPTS => {
                last_error = Some(e.to_string());
            }
            Err(e) => return Err(e),
        }
    }

    Err(anyhow!("{} failed after repair attempts", label))
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

/// Check if a step is rejected by feature flags. Returns Some(reason) if rejected.
fn step_rejected_reason(
    step: &PlanStep,
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> Option<&'static str> {
    if !allow_agent_steps && matches!(step, PlanStep::AiStep { .. }) {
        return Some("AiStep rejected (agent steps disabled)");
    }
    if !allow_ai_transforms && matches!(step, PlanStep::AiTransform { .. }) {
        return Some("AiTransform rejected (AI transforms disabled)");
    }
    None
}

/// Extract JSON from text that may be wrapped in markdown code fences.
fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```json") {
        let after_fence = &trimmed[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }
    if let Some(start) = trimmed.find("```") {
        let after_fence = &trimmed[start + 3..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim();
        }
    }
    trimmed
}

/// Output from the patcher LLM.
#[derive(Debug, Deserialize)]
struct PatcherOutput {
    #[serde(default)]
    add: Vec<PlanStep>,
    #[serde(default)]
    remove_node_ids: Vec<String>,
    #[serde(default)]
    update: Vec<PatchNodeUpdate>,
}

/// A node update from the patcher (only changed fields).
#[derive(Debug, Deserialize)]
struct PatchNodeUpdate {
    node_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    node_type: Option<Value>,
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

/// Build the patcher system prompt.
fn patcher_system_prompt(
    workflow: &Workflow,
    tools_json: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
) -> String {
    let tool_list = serde_json::to_string_pretty(tools_json).unwrap_or_default();

    let nodes_summary: Vec<Value> = workflow
        .nodes
        .iter()
        .map(|n| {
            serde_json::json!({
                "id": n.id.to_string(),
                "name": n.name,
                "type": format!("{:?}", n.node_type).split('(').next().unwrap_or("Unknown"),
            })
        })
        .collect();
    let nodes_json = serde_json::to_string_pretty(&nodes_summary).unwrap_or_default();

    let edges_summary: Vec<Value> = workflow
        .edges
        .iter()
        .map(|e| serde_json::json!({"from": e.from.to_string(), "to": e.to.to_string()}))
        .collect();
    let edges_json = serde_json::to_string_pretty(&edges_summary).unwrap_or_default();

    let mut step_types = String::from("Step types for 'add': same as planning (Tool, ");
    if allow_ai_transforms {
        step_types.push_str("AiTransform, ");
    }
    if allow_agent_steps {
        step_types.push_str("AiStep, ");
    }
    step_types.push_str("see the tool schemas below).");

    format!(
        r#"You are a workflow editor for UI automation. Given an existing workflow and a user's modification request, produce a JSON patch.

Current workflow nodes:
{nodes_json}

Current edges:
{edges_json}

Available MCP tools:
{tool_list}

{step_types}

Output ONLY a JSON object with these optional fields:
{{
  "add": [<steps to add, same format as planning>],
  "remove_node_ids": ["<id1>", "<id2>"],
  "update": [{{"node_id": "<id>", "name": "new name", "node_type": <step as Tool/AiStep/AiTransform>}}]
}}

Rules:
- Only include fields that have changes (omit empty arrays).
- For "add", use the same step format as planning (step_type: Tool/AiTransform/AiStep).
- For "remove_node_ids", use the exact node IDs from the current workflow.
- For "update", only include fields that changed.
- New nodes from "add" will be appended after the last existing node.
- Keep the workflow functional — don't remove nodes that break the flow without replacement."#,
    )
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
        match id_str.parse::<Uuid>() {
            Ok(id) => {
                if workflow.nodes.iter().any(|n| n.id == id) {
                    removed_node_ids.push(id);
                } else {
                    warnings.push(format!("Remove: node {} not found in workflow", id_str));
                }
            }
            Err(_) => {
                warnings.push(format!("Remove: invalid node ID: {}", id_str));
            }
        }
    }

    // Process updated nodes
    let mut updated_nodes = Vec::new();
    for update in &patcher_output.update {
        match update.node_id.parse::<Uuid>() {
            Ok(id) => {
                if let Some(existing) = workflow.nodes.iter().find(|n| n.id == id) {
                    let mut node = existing.clone();
                    if let Some(name) = &update.name {
                        node.name = name.clone();
                    }
                    if let Some(nt_value) = &update.node_type {
                        let short_id = id_str_short(&id);
                        match serde_json::from_value::<PlanStep>(nt_value.clone()) {
                            Ok(step) => {
                                if let Some(reason) = step_rejected_reason(
                                    &step,
                                    allow_ai_transforms,
                                    allow_agent_steps,
                                ) {
                                    warnings.push(format!("Update {}: {}", short_id, reason));
                                } else {
                                    match step_to_node_type(&step, mcp_tools_openai) {
                                        Ok((node_type, _)) => node.node_type = node_type,
                                        Err(e) => {
                                            warnings.push(format!("Update {}: {}", short_id, e))
                                        }
                                    }
                                }
                            }
                            Err(e) => warnings.push(format!(
                                "Update {}: failed to parse node_type: {}",
                                short_id, e
                            )),
                        }
                    }
                    updated_nodes.push(node);
                } else {
                    warnings.push(format!("Update: node {} not found", update.node_id));
                }
            }
            Err(_) => {
                warnings.push(format!("Update: invalid node ID: {}", update.node_id));
            }
        }
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

fn id_str_short(id: &Uuid) -> String {
    id.to_string()[..8].to_string()
}

fn truncate_intent(intent: &str) -> String {
    if intent.len() <= 50 {
        return intent.to_string();
    }
    // Find the last char boundary at or before byte 47, leaving room for "..."
    let mut end = 47;
    while end > 0 && !intent.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &intent[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChatResponse, Choice};
    use std::sync::Mutex;

    /// Mock backend that returns a sequence of responses (for testing repair pass).
    struct MockBackend {
        responses: Mutex<Vec<String>>,
        calls: Mutex<Vec<Vec<Message>>>,
    }

    impl MockBackend {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(String::from).collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn single(response: &str) -> Self {
            Self::new(vec![response])
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl ChatBackend for MockBackend {
        fn model_name(&self) -> &str {
            "mock"
        }

        async fn chat(
            &self,
            messages: Vec<Message>,
            _tools: Option<Vec<Value>>,
        ) -> Result<ChatResponse> {
            self.calls.lock().unwrap().push(messages);
            let mut responses = self.responses.lock().unwrap();
            let text = if responses.is_empty() {
                r#"{"steps": []}"#.to_string()
            } else {
                responses.remove(0)
            };
            Ok(ChatResponse {
                id: "mock".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: Message::assistant(&text),
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }
    }

    fn sample_tools() -> Vec<Value> {
        vec![
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "focus_window",
                    "description": "Focus a window",
                    "parameters": {"type": "object", "properties": {"app_name": {"type": "string"}}}
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "take_screenshot",
                    "description": "Take a screenshot",
                    "parameters": {"type": "object", "properties": {"mode": {"type": "string"}}}
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "click",
                    "description": "Click at coordinates",
                    "parameters": {"type": "object", "properties": {"x": {"type": "number"}, "y": {"type": "number"}}}
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "find_text",
                    "description": "Find text on screen",
                    "parameters": {"type": "object", "properties": {"text": {"type": "string"}}}
                }
            }),
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": "type_text",
                    "description": "Type text",
                    "parameters": {"type": "object", "properties": {"text": {"type": "string"}}}
                }
            }),
        ]
    }

    // --- Unit tests ---

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"steps": []}"#;
        assert_eq!(extract_json(input), input);
    }

    #[test]
    fn test_extract_json_code_fence() {
        let input = "```json\n{\"steps\": []}\n```";
        assert_eq!(extract_json(input), r#"{"steps": []}"#);
    }

    #[test]
    fn test_extract_json_plain_fence() {
        let input = "```\n{\"steps\": []}\n```";
        assert_eq!(extract_json(input), r#"{"steps": []}"#);
    }

    #[test]
    fn test_planner_system_prompt_includes_tools() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "click",
                "description": "Click at coordinates",
                "parameters": {}
            }
        })];
        let prompt = planner_system_prompt(&tools, false, false);
        assert!(prompt.contains("click"));
        assert!(prompt.contains("Tool"));
        assert!(!prompt.contains("step_type\": \"AiTransform\""));
        assert!(!prompt.contains("step_type\": \"AiStep\""));
    }

    #[test]
    fn test_planner_system_prompt_with_all_features() {
        let prompt = planner_system_prompt(&[], true, true);
        assert!(prompt.contains("AiTransform"));
        assert!(prompt.contains("AiStep"));
    }

    #[test]
    fn test_step_to_node_type_click() {
        let step = PlanStep::Tool {
            tool_name: "click".to_string(),
            arguments: serde_json::json!({"x": 100.0, "y": 200.0, "button": "left"}),
            name: Some("Click button".to_string()),
        };
        let (nt, name) = step_to_node_type(&step, &[]).unwrap();
        assert_eq!(name, "Click button");
        assert!(matches!(nt, NodeType::Click(_)));
    }

    #[test]
    fn test_step_to_node_type_unknown_tool_uses_mcp_tool_call() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "custom_tool",
                "description": "A custom tool",
                "parameters": {}
            }
        })];
        let step = PlanStep::Tool {
            tool_name: "custom_tool".to_string(),
            arguments: serde_json::json!({"foo": "bar"}),
            name: None,
        };
        let (nt, _) = step_to_node_type(&step, &tools).unwrap();
        assert!(matches!(nt, NodeType::McpToolCall(_)));
    }

    #[test]
    fn test_step_to_node_type_unknown_tool_fails_if_not_in_schema() {
        let result = step_to_node_type(
            &PlanStep::Tool {
                tool_name: "nonexistent".to_string(),
                arguments: serde_json::json!({}),
                name: None,
            },
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_layout_nodes() {
        let positions = layout_nodes(3);
        assert_eq!(positions.len(), 3);
        assert!(positions[1].y > positions[0].y);
        assert!(positions[2].y > positions[1].y);
    }

    #[test]
    fn test_truncate_intent() {
        assert_eq!(truncate_intent("short"), "short");
        let long = "a".repeat(60);
        let truncated = truncate_intent(&long);
        assert!(truncated.len() <= 50);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_truncate_intent_multibyte_utf8() {
        // Each emoji is 4 bytes; 13 emojis = 52 bytes > 50 limit
        let emojis = "🎉".repeat(13);
        let truncated = truncate_intent(&emojis);
        assert!(truncated.ends_with("..."));
        // Must not panic and must be valid UTF-8

        // Multi-byte char spanning the byte-47 boundary
        // 46 ASCII bytes + "é" (2 bytes) + padding = well over 50
        let mixed = format!("{}é{}", "a".repeat(46), "b".repeat(10));
        let truncated = truncate_intent(&mixed);
        assert!(truncated.ends_with("..."));
        // The "é" at byte 46-47 should be included or excluded cleanly
        assert!(!truncated.contains('\u{FFFD}')); // no replacement chars
    }

    // --- Integration tests with mock backend ---

    #[tokio::test]
    async fn test_plan_focus_screenshot_click() {
        let response = r#"{"steps": [
            {"step_type": "Tool", "tool_name": "focus_window", "arguments": {"app_name": "Safari"}},
            {"step_type": "Tool", "tool_name": "take_screenshot", "arguments": {"mode": "window", "app_name": "Safari", "include_ocr": true}},
            {"step_type": "Tool", "tool_name": "find_text", "arguments": {"text": "Login"}},
            {"step_type": "Tool", "tool_name": "click", "arguments": {"x": 100, "y": 200}}
        ]}"#;
        let mock = MockBackend::single(response);
        let result = plan_workflow_with_backend(
            &mock,
            "Focus Safari and click the Login button",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(result.workflow.nodes.len(), 4);
        assert_eq!(result.workflow.edges.len(), 3);
        assert!(result.warnings.is_empty());
        assert!(matches!(
            result.workflow.nodes[0].node_type,
            NodeType::FocusWindow(_)
        ));
        assert!(matches!(
            result.workflow.nodes[1].node_type,
            NodeType::TakeScreenshot(_)
        ));
        assert!(matches!(
            result.workflow.nodes[2].node_type,
            NodeType::FindText(_)
        ));
        assert!(matches!(
            result.workflow.nodes[3].node_type,
            NodeType::Click(_)
        ));
        assert_eq!(mock.call_count(), 1);
    }

    #[tokio::test]
    async fn test_plan_with_code_fence_wrapping() {
        let response = r#"```json
{"steps": [
    {"step_type": "Tool", "tool_name": "type_text", "arguments": {"text": "hello"}}
]}
```"#;
        let mock = MockBackend::single(response);
        let result = plan_workflow_with_backend(&mock, "Type hello", &sample_tools(), false, false)
            .await
            .unwrap();

        assert_eq!(result.workflow.nodes.len(), 1);
        assert!(matches!(
            result.workflow.nodes[0].node_type,
            NodeType::TypeText(_)
        ));
    }

    #[tokio::test]
    async fn test_plan_agent_steps_filtered_when_disabled() {
        let response = r#"{"steps": [
            {"step_type": "Tool", "tool_name": "take_screenshot", "arguments": {}},
            {"step_type": "AiStep", "prompt": "Decide what to do"}
        ]}"#;
        let mock = MockBackend::single(response);
        let result = plan_workflow_with_backend(
            &mock,
            "Take a screenshot and decide",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(result.workflow.nodes.len(), 1);
        assert!(!result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_plan_agent_steps_kept_when_enabled() {
        let response = r#"{"steps": [
            {"step_type": "Tool", "tool_name": "take_screenshot", "arguments": {}},
            {"step_type": "AiStep", "prompt": "Decide what to do", "allowed_tools": ["click"]}
        ]}"#;
        let mock = MockBackend::single(response);
        let result = plan_workflow_with_backend(
            &mock,
            "Take a screenshot and decide",
            &sample_tools(),
            false,
            true,
        )
        .await
        .unwrap();

        assert_eq!(result.workflow.nodes.len(), 2);
        assert!(matches!(
            result.workflow.nodes[1].node_type,
            NodeType::AiStep(_)
        ));
    }

    #[tokio::test]
    async fn test_repair_pass_fixes_invalid_json() {
        let bad_response = r#"Here is the plan: {"steps": [invalid json}]}"#;
        let good_response = r#"{"steps": [{"step_type": "Tool", "tool_name": "click", "arguments": {"x": 50, "y": 50}}]}"#;
        let mock = MockBackend::new(vec![bad_response, good_response]);

        let result =
            plan_workflow_with_backend(&mock, "Click somewhere", &sample_tools(), false, false)
                .await
                .unwrap();

        assert_eq!(result.workflow.nodes.len(), 1);
        // Should have called the backend twice (initial + repair)
        assert_eq!(mock.call_count(), 2);
    }

    #[tokio::test]
    async fn test_repair_pass_fails_after_max_attempts() {
        let bad = r#"not json at all"#;
        let mock = MockBackend::new(vec![bad, bad]);

        let result =
            plan_workflow_with_backend(&mock, "Click somewhere", &sample_tools(), false, false)
                .await;

        assert!(result.is_err());
        assert_eq!(mock.call_count(), 2);
    }

    #[tokio::test]
    async fn test_plan_empty_steps_returns_error() {
        let response = r#"{"steps": []}"#;
        let mock = MockBackend::single(response);
        let result =
            plan_workflow_with_backend(&mock, "Do nothing", &sample_tools(), false, false).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no steps"));
    }

    #[tokio::test]
    async fn test_patch_adds_node() {
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            nodes: vec![Node::new(
                NodeType::TakeScreenshot(TakeScreenshotParams {
                    mode: ScreenshotMode::Window,
                    target: None,
                    include_ocr: true,
                }),
                Position { x: 300.0, y: 100.0 },
                "Screenshot",
            )],
            edges: vec![],
        };

        let response = r#"{"add": [{"step_type": "Tool", "tool_name": "click", "arguments": {"x": 100, "y": 200}}]}"#;
        let mock = MockBackend::single(response);

        let result = patch_workflow_with_backend(
            &mock,
            &workflow,
            "Add a click after the screenshot",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(result.added_nodes.len(), 1);
        assert!(matches!(
            result.added_nodes[0].node_type,
            NodeType::Click(_)
        ));
        // Should add an edge from existing node to new node
        assert_eq!(result.added_edges.len(), 1);
        assert_eq!(result.added_edges[0].from, workflow.nodes[0].id);
    }

    #[tokio::test]
    async fn test_patch_removes_node() {
        let node = Node::new(
            NodeType::Click(ClickParams {
                x: Some(100.0),
                y: Some(200.0),
                button: MouseButton::Left,
                click_count: 1,
            }),
            Position { x: 300.0, y: 100.0 },
            "Click",
        );
        let node_id = node.id;
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            nodes: vec![node],
            edges: vec![],
        };

        let response = format!(r#"{{"remove_node_ids": ["{}"]}}"#, node_id);
        let mock = MockBackend::single(&response);

        let result = patch_workflow_with_backend(
            &mock,
            &workflow,
            "Remove the click",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(result.removed_node_ids.len(), 1);
        assert_eq!(result.removed_node_ids[0], node_id);
    }

    #[tokio::test]
    async fn test_patch_add_filters_disallowed_step_types() {
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            nodes: vec![Node::new(
                NodeType::TakeScreenshot(TakeScreenshotParams {
                    mode: ScreenshotMode::Window,
                    target: None,
                    include_ocr: true,
                }),
                Position { x: 300.0, y: 100.0 },
                "Screenshot",
            )],
            edges: vec![],
        };

        // Patcher tries to add an AiStep and an AiTransform, but both flags are disabled
        let response = r#"{"add": [
            {"step_type": "AiStep", "prompt": "Decide what to do"},
            {"step_type": "AiTransform", "kind": "summarize", "input_ref": "step1"},
            {"step_type": "Tool", "tool_name": "click", "arguments": {"x": 50, "y": 50}}
        ]}"#;
        let mock = MockBackend::single(response);

        let result = patch_workflow_with_backend(
            &mock,
            &workflow,
            "Add some steps",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        // Only the Tool step should survive
        assert_eq!(result.added_nodes.len(), 1);
        assert!(matches!(
            result.added_nodes[0].node_type,
            NodeType::Click(_)
        ));
        // Two warnings for the filtered steps
        assert!(result.warnings.len() >= 2);
    }

    #[tokio::test]
    async fn test_patch_update_rejects_disallowed_node_type_change() {
        let node = Node::new(
            NodeType::Click(ClickParams {
                x: Some(100.0),
                y: Some(200.0),
                button: MouseButton::Left,
                click_count: 1,
            }),
            Position { x: 300.0, y: 100.0 },
            "Click",
        );
        let node_id = node.id;
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            nodes: vec![node],
            edges: vec![],
        };

        // Try to update the node to an AiStep with agent steps disabled
        let response = format!(
            r#"{{"update": [{{"node_id": "{}", "node_type": {{"step_type": "AiStep", "prompt": "do something"}}}}]}}"#,
            node_id
        );
        let mock = MockBackend::single(&response);

        let result = patch_workflow_with_backend(
            &mock,
            &workflow,
            "Change to AI",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        // Node should still be in updated_nodes (name update still applies) but type unchanged
        assert_eq!(result.updated_nodes.len(), 1);
        assert!(matches!(
            result.updated_nodes[0].node_type,
            NodeType::Click(_)
        ));
        assert!(!result.warnings.is_empty());
    }

    #[tokio::test]
    async fn test_patch_repair_pass_fixes_invalid_json() {
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            nodes: vec![Node::new(
                NodeType::TakeScreenshot(TakeScreenshotParams {
                    mode: ScreenshotMode::Window,
                    target: None,
                    include_ocr: true,
                }),
                Position { x: 300.0, y: 100.0 },
                "Screenshot",
            )],
            edges: vec![],
        };

        let bad_response = r#"Here's the patch: {"add": [invalid}]}"#;
        let good_response = r#"{"add": [{"step_type": "Tool", "tool_name": "click", "arguments": {"x": 50, "y": 50}}]}"#;
        let mock = MockBackend::new(vec![bad_response, good_response]);

        let result = patch_workflow_with_backend(
            &mock,
            &workflow,
            "Add a click",
            &sample_tools(),
            false,
            false,
        )
        .await
        .unwrap();

        assert_eq!(result.added_nodes.len(), 1);
        assert_eq!(mock.call_count(), 2);
    }

    #[tokio::test]
    async fn test_patch_repair_pass_fails_after_max_attempts() {
        let workflow = Workflow {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            nodes: vec![Node::new(
                NodeType::TakeScreenshot(TakeScreenshotParams {
                    mode: ScreenshotMode::Window,
                    target: None,
                    include_ocr: true,
                }),
                Position { x: 300.0, y: 100.0 },
                "Screenshot",
            )],
            edges: vec![],
        };

        let bad = r#"not json at all"#;
        let mock = MockBackend::new(vec![bad, bad]);

        let result = patch_workflow_with_backend(
            &mock,
            &workflow,
            "Add a click",
            &sample_tools(),
            false,
            false,
        )
        .await;

        assert!(result.is_err());
        assert_eq!(mock.call_count(), 2);
    }
}
