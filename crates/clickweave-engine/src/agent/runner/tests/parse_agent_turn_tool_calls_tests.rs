//! Tests for the live `parse_agent_turn(&Message)` parser that
//! consumes OpenAI-shaped `tool_calls`. Distinct from the JSON
//! envelope tests above, which exercise the `serde::Deserialize`
//! path for `AgentTurn`.

use super::*;
use crate::agent::task_state::WatchSlotName;
use clickweave_llm::{CallType, FunctionCall, Message, ToolCall};
use serde_json::json;

fn tc(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        call_type: CallType::Function,
        function: FunctionCall {
            name: name.to_string(),
            arguments: args,
        },
    }
}

#[test]
fn maps_mcp_tool_call_to_tool_call_action_with_no_mutations() {
    let msg = Message::assistant_tool_calls(vec![tc("tc1", "cdp_click", json!({"uid": "d5"}))]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert!(turn.mutations.is_empty());
    match turn.action {
        AgentAction::ToolCall { tool_name, .. } => assert_eq!(tool_name, "cdp_click"),
        _ => panic!("expected tool_call"),
    }
}

#[test]
fn maps_agent_done_pseudo_tool_to_agent_done_action() {
    let msg = Message::assistant_tool_calls(vec![tc(
        "tc1",
        "agent_done",
        json!({"summary": "logged in"}),
    )]);
    let turn = parse_agent_turn(&msg).unwrap();
    match turn.action {
        AgentAction::AgentDone { summary } => assert_eq!(summary, "logged in"),
        _ => panic!("expected agent_done"),
    }
}

#[test]
fn maps_invoke_skill_pseudo_tool_to_invoke_skill_action() {
    let msg = Message::assistant_tool_calls(vec![tc(
        "tc1",
        "invoke_skill",
        json!({
            "skill_id": "open_settings",
            "version": 2,
            "parameters": {"app": "Notes"}
        }),
    )]);
    let turn = parse_agent_turn(&msg).unwrap();
    match turn.action {
        AgentAction::InvokeSkill {
            skill_id,
            version,
            parameters,
        } => {
            assert_eq!(skill_id, "open_settings");
            assert_eq!(version, 2);
            assert_eq!(parameters, json!({"app": "Notes"}));
        }
        other => panic!("expected invoke_skill, got {:?}", other),
    }
}

#[test]
fn maps_get_current_datetime_to_tool_call_action() {
    let msg = Message::assistant_tool_calls(vec![tc("tc1", "get_current_datetime", json!({}))]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert!(turn.mutations.is_empty());
    match turn.action {
        AgentAction::ToolCall {
            tool_name,
            arguments,
            tool_call_id,
        } => {
            assert_eq!(tool_name, "get_current_datetime");
            assert_eq!(arguments, json!({}));
            assert_eq!(tool_call_id, "tc1");
        }
        other => panic!("expected get_current_datetime tool call, got {:?}", other),
    }
}

#[test]
fn invoke_skill_missing_required_fields_replans() {
    // Missing `version` — the parser cannot fabricate a sensible
    // default, so degrades to a replan instead of dispatching a
    // skill that won't resolve.
    let msg = Message::assistant_tool_calls(vec![tc(
        "tc1",
        "invoke_skill",
        json!({"skill_id": "open_settings"}),
    )]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert!(matches!(turn.action, AgentAction::AgentReplan { .. }));
}

#[test]
fn invoke_skill_version_overflow_replans_instead_of_wrapping() {
    let msg = Message::assistant_tool_calls(vec![tc(
        "tc1",
        "invoke_skill",
        json!({
            "skill_id": "open_settings",
            "version": u64::from(u32::MAX) + 1,
            "parameters": {}
        }),
    )]);
    let turn = parse_agent_turn(&msg).unwrap();
    match turn.action {
        AgentAction::AgentReplan { reason } => {
            assert!(reason.contains("out of range"));
        }
        other => panic!("expected replan for overflow, got {:?}", other),
    }
}

#[test]
fn collects_mutations_then_takes_first_action_call() {
    let msg = Message::assistant_tool_calls(vec![
        tc("m1", "push_subgoal", json!({"text": "open login"})),
        tc(
            "m2",
            "record_hypothesis",
            json!({"text": "form has 2 fields"}),
        ),
        tc("a1", "cdp_find_elements", json!({})),
        // Extra action calls after the first action are dropped.
        tc("a2", "cdp_click", json!({"uid": "d2"})),
    ]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert_eq!(turn.mutations.len(), 2);
    assert!(matches!(
        turn.mutations[0],
        TaskStateMutation::PushSubgoal { .. }
    ));
    assert!(matches!(
        turn.mutations[1],
        TaskStateMutation::RecordHypothesis { .. }
    ));
    match turn.action {
        AgentAction::ToolCall { tool_name, .. } => assert_eq!(tool_name, "cdp_find_elements"),
        _ => panic!("expected first action to win"),
    }
}

#[test]
fn mutations_after_action_are_still_collected() {
    // Apply order is `apply_mutations` -> action; tool-call array
    // ordering is irrelevant. A mutation emitted after the action
    // is still picked up so the parser is robust to LLM sloppiness.
    let msg = Message::assistant_tool_calls(vec![
        tc("a1", "agent_done", json!({"summary": "done"})),
        tc("m1", "push_subgoal", json!({"text": "noted"})),
    ]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert_eq!(turn.mutations.len(), 1);
    assert!(matches!(turn.action, AgentAction::AgentDone { .. }));
}

#[test]
fn only_mutations_synthesizes_agent_replan() {
    // The LLM emitted state mutations but no action — surface as a
    // replan so the next turn re-observes instead of aborting.
    let msg =
        Message::assistant_tool_calls(vec![tc("m1", "push_subgoal", json!({"text": "explore"}))]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert_eq!(turn.mutations.len(), 1);
    match turn.action {
        AgentAction::AgentReplan { reason } => {
            assert!(reason.starts_with(NO_ACTION_MUTATION_ONLY_PREFIX));
            assert!(reason.contains("no MCP/environment action ran"));
        }
        other => panic!("expected mutation-only replan, got {:?}", other),
    }
}

#[test]
fn malformed_mutation_is_dropped_without_aborting_turn() {
    // `set_watch_slot` requires both `name` and `note`; a missing
    // field drops just that mutation while letting subsequent
    // mutations and the action through.
    let msg = Message::assistant_tool_calls(vec![
        tc("m_bad", "set_watch_slot", json!({"name": "pending_modal"})),
        tc(
            "m_good",
            "set_watch_slot",
            json!({"name": "pending_auth", "note": "captcha shown"}),
        ),
        tc("a1", "agent_replan", json!({"reason": "auth required"})),
    ]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert_eq!(turn.mutations.len(), 1);
    match &turn.mutations[0] {
        TaskStateMutation::SetWatchSlot { name, .. } => {
            assert_eq!(*name, WatchSlotName::PendingAuth)
        }
        _ => panic!("expected set_watch_slot for pending_auth"),
    }
    assert!(matches!(turn.action, AgentAction::AgentReplan { .. }));
}

#[test]
fn refute_hypothesis_parses_index() {
    let msg = Message::assistant_tool_calls(vec![
        tc("m1", "refute_hypothesis", json!({"index": 3})),
        tc("a1", "agent_replan", json!({"reason": "wrong"})),
    ]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert!(matches!(
        turn.mutations[0],
        TaskStateMutation::RefuteHypothesis { index: 3 }
    ));
}

#[test]
fn unknown_watch_slot_name_drops_mutation() {
    let msg = Message::assistant_tool_calls(vec![
        tc(
            "m1",
            "set_watch_slot",
            json!({"name": "made_up_slot", "note": "x"}),
        ),
        tc("a1", "agent_replan", json!({"reason": "ok"})),
    ]);
    let turn = parse_agent_turn(&msg).unwrap();
    assert!(turn.mutations.is_empty());
}

#[test]
fn empty_tool_calls_array_falls_back_to_text_replan() {
    // `assistant_tool_calls(vec![])` with no content emits a replan
    // with the no-call sentinel reason, mirroring text-only output.
    let msg = Message::assistant_tool_calls(vec![]);
    let turn = parse_agent_turn(&msg).unwrap();
    match turn.action {
        AgentAction::AgentReplan { reason } => {
            assert!(reason.contains("no tool call") || reason.is_empty());
        }
        _ => panic!("expected agent_replan fallback"),
    }
}
