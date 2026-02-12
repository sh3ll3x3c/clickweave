use super::mapping::step_to_node_type;
use super::parse::{extract_json, layout_nodes, truncate_intent};
use super::prompt::planner_system_prompt;
use super::*;
use crate::{ChatBackend, ChatResponse, Choice, Message};
use clickweave_core::{ClickParams, MouseButton, NodeType, ScreenshotMode, TakeScreenshotParams};
use std::sync::Mutex;

// ── Test helpers ────────────────────────────────────────────────

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
        _tools: Option<Vec<serde_json::Value>>,
    ) -> anyhow::Result<ChatResponse> {
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

fn sample_tools() -> Vec<serde_json::Value> {
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

// ── Unit tests ──────────────────────────────────────────────────

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

// ── Planning integration tests ──────────────────────────────────

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
        plan_workflow_with_backend(&mock, "Click somewhere", &sample_tools(), false, false).await;

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

// ── Patching integration tests ──────────────────────────────────

#[tokio::test]
async fn test_patch_adds_node() {
    let workflow = Workflow {
        id: uuid::Uuid::new_v4(),
        name: "Test".to_string(),
        nodes: vec![Node::new(
            NodeType::TakeScreenshot(TakeScreenshotParams {
                mode: ScreenshotMode::Window,
                target: None,
                include_ocr: true,
            }),
            clickweave_core::Position { x: 300.0, y: 100.0 },
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
        clickweave_core::Position { x: 300.0, y: 100.0 },
        "Click",
    );
    let node_id = node.id;
    let workflow = Workflow {
        id: uuid::Uuid::new_v4(),
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
        id: uuid::Uuid::new_v4(),
        name: "Test".to_string(),
        nodes: vec![Node::new(
            NodeType::TakeScreenshot(TakeScreenshotParams {
                mode: ScreenshotMode::Window,
                target: None,
                include_ocr: true,
            }),
            clickweave_core::Position { x: 300.0, y: 100.0 },
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
        clickweave_core::Position { x: 300.0, y: 100.0 },
        "Click",
    );
    let node_id = node.id;
    let workflow = Workflow {
        id: uuid::Uuid::new_v4(),
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
        id: uuid::Uuid::new_v4(),
        name: "Test".to_string(),
        nodes: vec![Node::new(
            NodeType::TakeScreenshot(TakeScreenshotParams {
                mode: ScreenshotMode::Window,
                target: None,
                include_ocr: true,
            }),
            clickweave_core::Position { x: 300.0, y: 100.0 },
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
        id: uuid::Uuid::new_v4(),
        name: "Test".to_string(),
        nodes: vec![Node::new(
            NodeType::TakeScreenshot(TakeScreenshotParams {
                mode: ScreenshotMode::Window,
                target: None,
                include_ocr: true,
            }),
            clickweave_core::Position { x: 300.0, y: 100.0 },
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
