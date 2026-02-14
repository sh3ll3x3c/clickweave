use clickweave_core::{NodeType, Workflow, tool_mapping};
use serde_json::Value;

/// Build the planner system prompt.
pub(crate) fn planner_system_prompt(
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
- For clicking on text elements: use click with a `target` argument (the text to find on screen) instead of explicit coordinates. The runtime will find the text and click it. Only use find_text separately when you need to verify text is present without clicking.
- Always focus the target window before interacting with it.
- If the user's intent implies opening or using an app that may not already be running, emit a launch_app step before focus_window. For example, "open Calculator and calculate 5×6" should start with launch_app(app_name="Calculator").
- Prefer deterministic Tool steps over AiStep whenever possible.
- Do not add unnecessary steps. Be efficient."#,
    )
}

/// Build the patcher system prompt.
pub(crate) fn patcher_system_prompt(
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
            let mut summary = serde_json::json!({
                "id": n.id.to_string(),
                "name": n.name,
            });
            match tool_mapping::node_type_to_tool_invocation(&n.node_type) {
                Ok(inv) => {
                    summary["tool_name"] = inv.name.into();
                    let mut args = inv.arguments;
                    // Click `target` is internal (not sent to MCP) but the LLM
                    // needs it to know what text the click resolves against.
                    if let NodeType::Click(p) = &n.node_type
                        && let Some(target) = &p.target
                    {
                        args["target"] = Value::String(target.clone());
                    }
                    summary["arguments"] = args;
                }
                Err(_) => {
                    // AiStep / AppDebugKitOp — show the raw node_type
                    if let Ok(v) = serde_json::to_value(&n.node_type) {
                        summary["node_type"] = v;
                    }
                }
            }
            summary
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
- For "update", include "node_type" whenever tool arguments need to change (e.g. different search text, click target, key). Changing only the "name" does NOT change what the node actually does at runtime.
- New nodes from "add" will be appended after the last existing node.
- Keep the workflow functional — don't remove nodes that break the flow without replacement."#,
    )
}
