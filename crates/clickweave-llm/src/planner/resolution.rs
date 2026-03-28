use clickweave_core::Workflow;

/// Build the system prompt for runtime resolution queries.
///
/// Teaches the LLM the restricted patch grammar (update/insert/remove)
/// and the constraints on what can be changed at runtime.
pub fn resolution_system_prompt(workflow: &Workflow) -> String {
    let workflow_summary = build_workflow_summary(workflow);

    format!(
        r#"You are a workflow assistant helping fix a runtime resolution failure.

A workflow step failed to find its target element on screen. You have the full planning context from earlier in this conversation. Use it to propose a minimal fix.

## Current Workflow
{workflow_summary}

## Response Format

Return a JSON object with optional fields:
```json
{{
  "reasoning": "Brief explanation of the fix",
  "update": [{{"node_id": "<uuid>", ...changed_fields}}],
  "add_nodes": [{{"name": "...", "node_type": "...", "arguments": {{}}, "insert_before": "<failing_node_uuid>"}}],
  "remove_node_ids": ["<uuid>", ...]
}}
```

### Update (field corrections on the failing node)
- `target` — the element to interact with
- `name` — node display name
- `text` — text to type
- `expected_outcome` — verification criteria
- Tool-specific arguments (`key`, `direction`, `app_name`)
- The node type (tool) MUST stay the same

### Insert (new steps before the failing node)
- Allowed types: Click, PressKey, TypeText, Hover, FocusWindow, Scroll, FindText
- Use `insert_before` with the failing node's ID — do NOT emit edges
- NOT allowed: Loop, EndLoop, If, Switch, AiStep

### Remove (redundant steps ahead of the failing node)
- Sequential action nodes that are now redundant
- NOT: control-flow nodes, the failing node, already-completed nodes

## Constraints
- Minimal patch — only changes needed to unblock the current step
- Do NOT add/remove loops, conditionals, or switch branches
- Do NOT restructure control-flow edges
- Do NOT modify already-completed nodes
- Prefer changing the target over changing the tool type
"#
    )
}

fn build_workflow_summary(workflow: &Workflow) -> String {
    let mut lines = Vec::new();
    for node in &workflow.nodes {
        lines.push(format!(
            "- [{}] {} ({})",
            node.id,
            node.name,
            node.node_type.display_name()
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_prompt_includes_workflow_nodes() {
        let wf = Workflow::default();
        let prompt = resolution_system_prompt(&wf);
        assert!(prompt.contains("Current Workflow"));
        assert!(prompt.contains("insert_before"));
        assert!(prompt.contains("Minimal patch"));
    }
}
