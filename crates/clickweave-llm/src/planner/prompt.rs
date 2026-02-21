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

    step_types.push_str(r#"

4. **Loop** — repeat a body of steps until an exit condition is met (do-while: body runs at least once). Define the body steps ONCE — the runtime repeats them each iteration, just like a `while` loop in code:
   ```json
   {"id": "<id>", "step_type": "Loop", "exit_condition": <Condition>, "max_iterations": 20, "name": "optional label"}
   ```

5. **EndLoop** — marks the end of a loop body (execution jumps back to the paired Loop node):
   ```json
   {"id": "<id>", "step_type": "EndLoop", "loop_id": "<loop_node_id>", "name": "optional label"}
   ```

6. **If** — conditional branch with exactly 2 outgoing edges (IfTrue, IfFalse):
   ```json
   {"id": "<id>", "step_type": "If", "condition": <Condition>, "name": "optional label"}
   ```

**Condition** objects compare a runtime variable to a value:
```json
{
  "left": {"type": "Variable", "name": "<node_name>.<field>"},
  "operator": "<op>",
  "right": {"type": "Literal", "value": {"type": "Bool", "value": true}}
}
```
Operators: Equals, NotEquals, GreaterThan, LessThan, GreaterThanOrEqual, LessThanOrEqual, Contains, NotContains, IsEmpty, IsNotEmpty.

Literal types: `{"type": "String", "value": "text"}`, `{"type": "Number", "value": 42}`, `{"type": "Bool", "value": true}`.

**Variable names** follow `<sanitized_node_name>.<field>` (lowercase, spaces→underscores). Fields per tool:
- find_text: `.found` (bool), `.text`, `.x`, `.y`, `.count`, `.matches`
- find_image: `.found` (bool), `.x`, `.y`, `.score`, `.count`, `.matches`
- list_windows: `.found` (bool), `.count`, `.windows`
- click, type_text, press_key, scroll, focus_window: `.success` (bool)
- take_screenshot: `.result`
- Any tool: `.result` (raw JSON response)"#);

    format!(
        r#"You are a workflow planner for UI automation. Given a user's intent, produce a sequence of steps that accomplish the goal.

You have access to these MCP tools:

{tool_list}

{step_types}

Rules:
- For **simple linear workflows** (no loops or branches), output: {{"steps": [...]}}
- For **workflows with control flow** (loops, branches), output a graph: {{"nodes": [...], "edges": [...]}}
  - Each node must have an `"id"` field (e.g. "n1", "n2").
  - Each edge has `"from"`, `"to"`, and optional `"output"` ({{"type": "LoopBody"}}, {{"type": "LoopDone"}}, {{"type": "IfTrue"}}, {{"type": "IfFalse"}}).
  - Regular edges (no control flow) omit `"output"`.
  - **EndLoop** must have exactly 1 outgoing edge pointing back to its paired Loop node (regular edge, no `"output"`).
  - Loop nodes must have exactly 2 outgoing edges: LoopBody (into the body) and LoopDone (exit after the loop).
  - If nodes must have exactly 2 outgoing edges: IfTrue and IfFalse.
- Use Loop/EndLoop when the user's intent involves repetition ("until", "while", "keep", "repeat", "N times"). Prefer loops over unrolling steps.
- **Loop edge wiring** — the cycle goes: Loop →(LoopBody)→ body steps → EndLoop → Loop. The exit goes: Loop →(LoopDone)→ after steps. Example edges for a 3-step body:
  ```
  Loop→A  (LoopBody)   // enter body
  A→B                   // body chain
  B→C                   // body chain
  C→EndLoop             // last body step flows into EndLoop
  EndLoop→Loop          // EndLoop loops BACK to Loop (regular edge)
  Loop→After (LoopDone) // exit when condition met
  ```
  WRONG: body→Loop or EndLoop→After. EndLoop ALWAYS points back to Loop, never forward.
- **Loop structure — think like code.** A loop has three parts:
  1. **Before the loop** (setup, runs once): e.g. launch app, type initial value
  2. **Loop body** (between Loop→LoopBody and EndLoop): ONLY the steps that repeat each iteration
  3. **After the loop** (via LoopDone edge, runs once): e.g. verify final result, take screenshot
  Example: "multiply by 2 until > 128" → setup: click "2" | body: click "×", click "2", click "=" | after: verify result. The body has 3 steps, NOT 10. Do NOT put setup or verification inside the loop body.
- Each Tool step must use exactly one tool from the list above with schema-valid arguments.
- Steps execute in sequence (output of one step is available to the next).
- Be precise: use find_text to locate UI elements before clicking them.
- For clicking on text elements: use click with a `target` argument (the text to find on screen) instead of explicit coordinates. The runtime will find the text and click it. Only use find_text separately when you need to verify text is present without clicking.
- Always focus the target window before interacting with it.
- If the user's intent implies opening or using an app that may not already be running, emit a launch_app step before focus_window. For example, "open Calculator and calculate 5×6" should start with launch_app(app_name="Calculator").
- Prefer deterministic Tool steps over AiStep whenever possible.
- Do not add unnecessary steps. Be efficient.
- Use ONLY the step types listed above. Workflows end implicitly after the last node — do not add "End" or "Start" nodes."#,
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
    step_types.push_str(" For control flow nodes (Loop, EndLoop, If), use \"add_nodes\" + \"add_edges\" instead of \"add\".");

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
  "add_nodes": [<nodes with "id" fields, for control flow>],
  "add_edges": [{{"from": "<id>", "to": "<id>", "output": {{"type": "LoopBody"}}}}],
  "remove_node_ids": ["<id1>", "<id2>"],
  "update": [{{"node_id": "<id>", "name": "new name", "node_type": <step as Tool/AiStep/AiTransform>}}]
}}

Rules:
- Only include fields that have changes (omit empty arrays).
- For "add", use the same step format as planning (step_type: Tool/AiTransform/AiStep).
- For "remove_node_ids", use the exact node IDs from the current workflow.
- For "update", include "node_type" whenever tool arguments need to change (e.g. different search text, click target, key). Changing only the "name" does NOT change what the node actually does at runtime.
- New nodes from "add" will be appended after the last existing node.
- For "add_nodes" + "add_edges", use short IDs (e.g. "n1", "n2") for new nodes. You can reference existing workflow node UUIDs in "add_edges" to connect new nodes to existing ones.
- Keep the workflow functional — don't remove nodes that break the flow without replacement.
- **Loop structure — think like code.** Setup steps go BEFORE the loop. Only repeating steps go in the body. Verification/cleanup goes AFTER (LoopDone). Example: "multiply by 2 until > 128" → setup: click "2" | body: click "×", click "2", click "=" | after: verify result."#,
    )
}

/// Build the unified assistant system prompt.
///
/// Handles both planning (empty workflow) and patching (existing workflow).
pub(crate) fn assistant_system_prompt(
    workflow: &Workflow,
    tools_json: &[Value],
    allow_ai_transforms: bool,
    allow_agent_steps: bool,
    run_context: Option<&str>,
) -> String {
    if workflow.nodes.is_empty() {
        let base = planner_system_prompt(tools_json, allow_ai_transforms, allow_agent_steps);
        let mut prompt = format!(
            "You are a conversational workflow assistant for UI automation. \
             You help users create and modify workflows through natural dialogue.\n\n\
             The workflow is currently empty. When the user describes what they want to automate, \
             generate a workflow plan.\n\n{base}"
        );
        if let Some(ctx) = run_context {
            prompt.push_str(&format!("\n\nLatest execution results:\n{ctx}"));
        }
        prompt
    } else {
        let base =
            patcher_system_prompt(workflow, tools_json, allow_ai_transforms, allow_agent_steps);
        let mut prompt = format!(
            "You are a conversational workflow assistant for UI automation. \
             You help users modify their existing workflow through natural dialogue.\n\n\
             When the user asks to modify the workflow, output the JSON patch as specified below. \
             When the user asks a question or makes a comment that doesn't require workflow changes, \
             respond conversationally WITHOUT any JSON output.\n\n{base}"
        );
        if let Some(ctx) = run_context {
            prompt.push_str(&format!("\n\nLatest execution results:\n{ctx}"));
        }
        prompt
    }
}
