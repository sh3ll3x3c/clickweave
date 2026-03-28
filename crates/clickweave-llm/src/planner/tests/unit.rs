use super::helpers::*;
use crate::planner::mapping::step_to_node_type;
use crate::planner::parse::{
    extract_json, id_str_short, layout_nodes, step_rejected_reason, truncate_intent,
};
use crate::planner::prompt::planner_system_prompt;
use crate::planner::*;
use clickweave_core::NodeType;

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

#[test]
fn test_planner_prompt_includes_tools() {
    let tools = vec![serde_json::json!({
        "type": "function",
        "function": {
            "name": "click",
            "description": "Click at coordinates",
            "parameters": {}
        }
    })];
    let prompt = planner_system_prompt(&tools, false, false, None, None, false);
    assert!(prompt.contains("click"));
    assert!(prompt.contains("Tool"));
    assert!(!prompt.contains("step_type\": \"AiTransform\""));
    assert!(!prompt.contains("step_type\": \"AiStep\""));
}

#[test]
fn test_planner_system_prompt_with_all_features() {
    let prompt = planner_system_prompt(&[], true, true, None, None, false);
    assert!(prompt.contains("AiTransform"));
    assert!(prompt.contains("AiStep"));
}

#[test]
fn test_planner_prompt_includes_control_flow() {
    let prompt = planner_system_prompt(&[], false, false, None, None, false);
    assert!(
        prompt.contains("Loop"),
        "Prompt should mention Loop step type"
    );
    assert!(
        prompt.contains("EndLoop"),
        "Prompt should mention EndLoop step type"
    );
    assert!(prompt.contains("If"), "Prompt should mention If step type");
    assert!(
        prompt.contains("exit_condition"),
        "Prompt should describe exit_condition"
    );
    assert!(prompt.contains("loop_id"), "Prompt should describe loop_id");
    assert!(
        prompt.contains("\"nodes\""),
        "Prompt should describe graph output format"
    );
    assert!(
        prompt.contains("\"edges\""),
        "Prompt should describe graph output format"
    );
    assert!(
        prompt.contains(".found"),
        "Prompt should include variable examples"
    );
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
fn test_step_to_node_type_loop() {
    let step = PlanStep::Loop {
        name: Some("Repeat".to_string()),
        exit_condition: bool_condition("check.found"),
        max_iterations: Some(20),
    };
    let (nt, name) = step_to_node_type(&step, &[]).unwrap();
    assert_eq!(name, "Repeat");
    assert!(matches!(nt, NodeType::Loop(_)));
    if let NodeType::Loop(p) = nt {
        assert_eq!(p.max_iterations, 20);
    }
}

#[test]
fn test_step_to_node_type_end_loop() {
    let step = PlanStep::EndLoop {
        name: Some("End Loop".to_string()),
        loop_id: "n2".to_string(),
    };
    let (nt, name) = step_to_node_type(&step, &[]).unwrap();
    assert_eq!(name, "End Loop");
    assert!(matches!(nt, NodeType::EndLoop(_)));
}

#[test]
fn test_step_to_node_type_if() {
    let step = PlanStep::If {
        name: Some("Check Result".to_string()),
        condition: bool_condition("find_text.found"),
    };
    let (nt, name) = step_to_node_type(&step, &[]).unwrap();
    assert_eq!(name, "Check Result");
    assert!(matches!(nt, NodeType::If(_)));
}

// ── extract_json additional tests ───────────────────────────────

#[test]
fn test_extract_json_with_surrounding_text() {
    let input = "Here is the output:\n```json\n{\"key\": \"value\"}\n```\nDone!";
    assert_eq!(extract_json(input), r#"{"key": "value"}"#);
}

#[test]
fn test_extract_json_whitespace_only() {
    let input = "   ";
    assert_eq!(extract_json(input), "");
}

#[test]
fn test_extract_json_no_closing_fence() {
    // When there's an opening fence but no closing fence, falls through to trimmed
    let input = "```json\n{\"key\": true}";
    assert_eq!(extract_json(input), input.trim());
}

// ── layout_nodes additional tests ───────────────────────────────

#[test]
fn test_layout_nodes_empty() {
    let positions = layout_nodes(0);
    assert!(positions.is_empty());
}

#[test]
fn test_layout_nodes_single() {
    let positions = layout_nodes(1);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].x, 300.0);
    assert_eq!(positions[0].y, 100.0);
}

#[test]
fn test_layout_nodes_spacing() {
    let positions = layout_nodes(3);
    // All x-coordinates are the same
    assert!(positions.iter().all(|p| p.x == 300.0));
    // Vertical spacing is 120.0
    let dy01 = positions[1].y - positions[0].y;
    let dy12 = positions[2].y - positions[1].y;
    assert!((dy01 - 120.0).abs() < f32::EPSILON);
    assert!((dy12 - 120.0).abs() < f32::EPSILON);
}

// ── truncate_intent additional tests ────────────────────────────

#[test]
fn test_truncate_intent_exactly_50_bytes() {
    let input = "a".repeat(50);
    assert_eq!(truncate_intent(&input), input);
}

#[test]
fn test_truncate_intent_51_bytes() {
    let input = "a".repeat(51);
    let truncated = truncate_intent(&input);
    assert!(truncated.ends_with("..."));
    assert!(truncated.len() <= 50);
}

// ── id_str_short tests ──────────────────────────────────────────

#[test]
fn test_id_str_short_returns_hyphenated_uuid() {
    let id = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
    let short = id_str_short(&id);
    // The function uses {:.8} on Hyphenated, which returns the full hyphenated form
    assert_eq!(short, "550e8400-e29b-41d4-a716-446655440000");
}

#[test]
fn test_id_str_short_is_deterministic() {
    let id = uuid::Uuid::new_v4();
    let a = id_str_short(&id);
    let b = id_str_short(&id);
    assert_eq!(a, b);
    // Should match the standard hyphenated form
    assert_eq!(a, id.as_hyphenated().to_string());
}

// ── step_rejected_reason tests ──────────────────────────────────

#[test]
fn test_step_rejected_reason_tool_always_allowed() {
    let step = PlanStep::Tool {
        tool_name: "click".to_string(),
        arguments: serde_json::json!({}),
        name: None,
    };
    // Tool steps are never rejected regardless of flags
    assert!(step_rejected_reason(&step, false, false).is_none());
    assert!(step_rejected_reason(&step, true, true).is_none());
}

#[test]
fn test_step_rejected_reason_ai_transform_disabled() {
    let step = PlanStep::AiTransform {
        kind: "summarize".to_string(),
        input_ref: "prev.output".to_string(),
        output_schema: None,
        name: None,
    };
    let reason = step_rejected_reason(&step, false, false);
    assert!(reason.is_some());
    assert!(reason.unwrap().contains("AI transforms disabled"));
}

#[test]
fn test_step_rejected_reason_ai_transform_enabled() {
    let step = PlanStep::AiTransform {
        kind: "summarize".to_string(),
        input_ref: "prev.output".to_string(),
        output_schema: None,
        name: None,
    };
    assert!(step_rejected_reason(&step, true, false).is_none());
}

#[test]
fn test_step_rejected_reason_ai_step_disabled() {
    let step = PlanStep::AiStep {
        prompt: "do something".to_string(),
        allowed_tools: None,
        max_tool_calls: None,
        timeout_ms: None,
        name: None,
    };
    let reason = step_rejected_reason(&step, false, false);
    assert!(reason.is_some());
    assert!(reason.unwrap().contains("agent steps disabled"));
}

#[test]
fn test_step_rejected_reason_ai_step_enabled() {
    let step = PlanStep::AiStep {
        prompt: "do something".to_string(),
        allowed_tools: None,
        max_tool_calls: None,
        timeout_ms: None,
        name: None,
    };
    assert!(step_rejected_reason(&step, false, true).is_none());
}

#[test]
fn test_step_rejected_reason_loop_always_allowed() {
    let step = PlanStep::Loop {
        name: None,
        exit_condition: bool_condition("x.found"),
        max_iterations: None,
    };
    assert!(step_rejected_reason(&step, false, false).is_none());
}

#[test]
fn test_step_rejected_reason_if_always_allowed() {
    let step = PlanStep::If {
        name: None,
        condition: bool_condition("x.found"),
    };
    assert!(step_rejected_reason(&step, false, false).is_none());
}

#[test]
fn test_step_rejected_reason_endloop_always_allowed() {
    let step = PlanStep::EndLoop {
        name: None,
        loop_id: "n1".to_string(),
    };
    assert!(step_rejected_reason(&step, false, false).is_none());
}

#[test]
fn planning_only_tools_rejected_from_workflow() {
    for tool_name in &["probe_app", "take_ax_snapshot", "cdp_connect"] {
        let step = PlanStep::Tool {
            tool_name: tool_name.to_string(),
            arguments: serde_json::json!({"app_name": "Signal"}),
            name: Some(format!("Planning {}", tool_name)),
        };
        let reason = step_rejected_reason(&step, true, true);
        assert!(reason.is_some(), "{} should be rejected", tool_name);
        assert!(reason.unwrap().contains("planning-only"));
    }
}

#[test]
fn dual_use_tools_not_rejected() {
    for tool_name in &["launch_app", "quit_app", "select_page", "click"] {
        let step = PlanStep::Tool {
            tool_name: tool_name.to_string(),
            arguments: serde_json::json!({}),
            name: None,
        };
        let reason = step_rejected_reason(&step, true, true);
        assert!(reason.is_none(), "{} should NOT be rejected", tool_name);
    }
}
