//! Prompt construction for the state-spine agent runner.
//!
//! The system prompt is built once per run and never re-rendered, preserving
//! prompt-cache hits on the stable prefix (D6). The per-turn user message
//! composes the harness-rendered state block with the current observation.
//!
//! `truncate_summary` is preserved for VLM completion check paths.
//!
//! Phase 2a: this module is dormant — nothing in the live runner imports it.
//! Wiring lands in Phase 3 (cutover), at which point the old `prompt.rs` is
//! deleted and this file is renamed `prompt.rs`.

#![allow(dead_code)] // Phase 2a: module is dormant; live consumers land in Phase 3 cutover.

use clickweave_mcp::Tool;
use serde_json::{Value, json};

use crate::agent::render::render_step_input;
use crate::agent::task_state::TaskState;
use crate::agent::world_model::WorldModel;

const SYSTEM_PROMPT_HEADER: &str = r#"You are Clickweave, an agent that automates desktop and browser workflows via MCP tools.

You operate on a harness-owned world model and task state. Each turn you receive:
1. A `<world_model>` block describing the environment (apps, windows, pages, elements, snapshots, uncertainty).
2. A `<task_state>` block describing your current goal, subgoal stack, active watch slots, and recorded hypotheses.
3. An optional observation returned by the previous tool.

Each turn you respond with a structured JSON object containing:
- `mutations`: zero or more task-state mutations (`push_subgoal`, `complete_subgoal`, `set_watch_slot`, `clear_watch_slot`, `record_hypothesis`, `refute_hypothesis`).
- `action`: exactly one of:
  - `{ "kind": "tool_call", "tool_name": "...", "arguments": {...}, "tool_call_id": "..." }`
  - `{ "kind": "agent_done", "summary": "..." }`
  - `{ "kind": "agent_replan", "reason": "..." }`

Rules:
- The `phase` field in `<task_state>` is harness-inferred. Do not try to set it yourself.
- Uid prefixes signal dispatch family: `a<N>` -> native AX (use `ax_click`/`ax_set_value`/`ax_select`); `d<N>` -> CDP (use `cdp_click`/`cdp_fill`).
- Prefer `cdp_find_elements` for targeted CDP discovery; use `cdp_take_dom_snapshot` only when you need the full page structure.
- When CDP is unavailable (native apps), use `take_ax_snapshot` and native action tools.
- Observation-only tools do not require approval; destructive tools may require approval from the operator.
"#;

/// Build the stable system prompt for the state-spine runner.
///
/// Stability is critical: this string is the prompt-cache prefix for every
/// turn of every run, so it must not embed run-specific data (goal, variant
/// context, timestamps). Variant context lands in `messages[1]` at the user
/// layer (D18).
pub fn build_system_prompt(tools: &[Tool]) -> String {
    let mut out = String::from(SYSTEM_PROMPT_HEADER);
    out.push_str("\n\nAvailable tools:\n");
    for t in tools {
        out.push_str("- ");
        out.push_str(&t.name);
        if let Some(desc) = &t.description
            && !desc.is_empty()
        {
            out.push_str(": ");
            out.push_str(desc);
        }
        out.push('\n');
    }
    out
}

/// Build the per-turn user message. State block first (above the observation),
/// so the LLM reads world + task state before reacting to the observation.
///
/// `retrieved` is the optional Spec 2 episodic-memory result list. When
/// non-empty, a `<retrieved_recoveries>` sibling block is spliced in
/// after the state block and before the observation so the LLM sees
/// remembered recoveries before reacting to the new observation (D23).
pub fn build_user_turn_message(
    wm: &WorldModel,
    ts: &TaskState,
    current_step: usize,
    observation_text: &str,
    retrieved: &[crate::agent::episodic::RetrievedEpisode],
) -> String {
    let mut out = render_step_input(wm, ts, current_step);

    let recoveries_block =
        crate::agent::episodic::render::render_retrieved_recoveries_block(retrieved);
    if !recoveries_block.is_empty() {
        out.push_str(&recoveries_block);
        out.push('\n');
    }

    if !observation_text.is_empty() {
        out.push_str("\n<observation>\n");
        out.push_str(observation_text);
        out.push_str("\n</observation>\n");
    }
    out
}

// --- ported from prompt.rs ---

/// Truncate text to `max_chars`, snapping to a character boundary.
/// Returns the original text if it fits within the limit.
///
/// Copied verbatim from `prompt.rs` so the VLM completion path can switch
/// over cleanly at Phase 3 cutover. The original copy remains in `prompt.rs`
/// as long as the old runner imports it.
pub fn truncate_summary(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let end = text.floor_char_boundary(max_chars);
    format!("{}...", &text[..end])
}

/// Tool definition for the agent_done pseudo-tool.
///
/// Ported verbatim from the legacy `prompt.rs` — the state-spine runner
/// appends this (and `agent_replan_tool`) to the MCP tool list each turn so
/// the LLM sees the completion / replan actions as callable tools.
pub fn agent_done_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "agent_done",
            "description": "Declare the goal as complete. Call this when you have successfully achieved the objective.",
            "parameters": {
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "A brief summary of what was accomplished."
                    }
                },
                "required": ["summary"]
            }
        }
    })
}

/// Tool definition for the agent_replan pseudo-tool.
///
/// Ported verbatim from the legacy `prompt.rs`.
pub fn agent_replan_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "agent_replan",
            "description": "Request a re-plan when the current approach seems stuck or the goal appears unreachable.",
            "parameters": {
                "type": "object",
                "properties": {
                    "reason": {
                        "type": "string",
                        "description": "Why the current approach is not working."
                    }
                },
                "required": ["reason"]
            }
        }
    })
}

// --- Task-state mutation pseudo-tools ---
//
// These describe the `AgentTurn.mutations` surface to the LLM via the
// OpenAI tool-calling API. They never dispatch to MCP —
// `parse_agent_turn` recognises their names and routes their arguments
// into `TaskStateMutation` values that the harness applies before the
// turn's action runs. The MCP tool list is unchanged by their presence.

pub fn push_subgoal_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "push_subgoal",
            "description": "Push a new subgoal onto the task-state stack. The new subgoal becomes the active focus until you call complete_subgoal. Mutation only — does not dispatch to MCP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Short description of the subgoal."
                    }
                },
                "required": ["text"]
            }
        }
    })
}

pub fn complete_subgoal_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "complete_subgoal",
            "description": "Pop the top of the subgoal stack as completed and record a milestone summary. Mutation only — does not dispatch to MCP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "summary": {
                        "type": "string",
                        "description": "What was accomplished for this subgoal."
                    }
                },
                "required": ["summary"]
            }
        }
    })
}

pub fn set_watch_slot_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "set_watch_slot",
            "description": "Mark a background concern (modal, auth, focus shift) so the harness will not replay cached actions while it is active. Mutation only — does not dispatch to MCP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "enum": ["pending_modal", "pending_auth", "pending_focus_shift"],
                        "description": "Which watch slot to set."
                    },
                    "note": {
                        "type": "string",
                        "description": "Operator-readable note about the concern."
                    }
                },
                "required": ["name", "note"]
            }
        }
    })
}

pub fn clear_watch_slot_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "clear_watch_slot",
            "description": "Clear a previously-set watch slot once the background concern has been resolved. Mutation only — does not dispatch to MCP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "enum": ["pending_modal", "pending_auth", "pending_focus_shift"],
                        "description": "Which watch slot to clear."
                    }
                },
                "required": ["name"]
            }
        }
    })
}

pub fn record_hypothesis_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "record_hypothesis",
            "description": "Record a hypothesis you are about to test (rolling ring buffer, oldest evicted). Mutation only — does not dispatch to MCP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The hypothesis under evaluation."
                    }
                },
                "required": ["text"]
            }
        }
    })
}

pub fn refute_hypothesis_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "refute_hypothesis",
            "description": "Mark a previously-recorded hypothesis as refuted. Index is the position in the current <task_state> hypotheses list. Mutation only — does not dispatch to MCP.",
            "parameters": {
                "type": "object",
                "properties": {
                    "index": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Index of the hypothesis to refute."
                    }
                },
                "required": ["index"]
            }
        }
    })
}

/// All harness-local pseudo-tools that the LLM may emit in a turn.
///
/// Order is intentional: the action pseudo-tools (`agent_done`,
/// `agent_replan`) come last so the LLM-facing tool list ends with the
/// "terminate the loop" choices, while the mutations cluster together at
/// the start of the pseudo-tool block.
pub fn pseudo_tools() -> Vec<Value> {
    vec![
        push_subgoal_tool(),
        complete_subgoal_tool(),
        set_watch_slot_tool(),
        clear_watch_slot_tool(),
        record_hypothesis_tool(),
        refute_hypothesis_tool(),
        agent_done_tool(),
        agent_replan_tool(),
    ]
}

/// Names of the pseudo-tools that map to `TaskStateMutation` rather than
/// `AgentAction`. Used by `parse_agent_turn` to route a tool call into
/// `mutations` instead of `action`. Kept as a small `&'static [&'static str]`
/// so the parser can match on it without rebuilding a HashSet per call.
pub const MUTATION_TOOL_NAMES: &[&str] = &[
    "push_subgoal",
    "complete_subgoal",
    "set_watch_slot",
    "clear_watch_slot",
    "record_hypothesis",
    "refute_hypothesis",
];

pub fn is_mutation_tool_name(name: &str) -> bool {
    MUTATION_TOOL_NAMES.contains(&name)
}

#[cfg(test)]
mod state_spine_prompt_tests {
    use super::*;
    use crate::agent::task_state::TaskState;
    use crate::agent::world_model::WorldModel;

    #[test]
    fn system_prompt_is_stable_across_calls_with_same_tool_list() {
        let tools: Vec<Tool> = vec![];
        let a = build_system_prompt(&tools);
        let b = build_system_prompt(&tools);
        assert_eq!(
            a, b,
            "system prompt must be deterministic for cache stability"
        );
    }

    #[test]
    fn system_prompt_does_not_contain_variant_context() {
        // D18: variant context now lives in messages[1], not messages[0].
        let tools: Vec<Tool> = vec![];
        let s = build_system_prompt(&tools);
        assert!(!s.contains("Variant context"));
    }

    #[test]
    fn system_prompt_lists_tools_with_descriptions() {
        let tools = vec![
            Tool {
                name: "cdp_click".to_string(),
                description: Some("Click a CDP-backed element".to_string()),
                input_schema: serde_json::json!({}),
                annotations: None,
            },
            Tool {
                name: "ax_click".to_string(),
                description: None,
                input_schema: serde_json::json!({}),
                annotations: None,
            },
        ];
        let s = build_system_prompt(&tools);
        assert!(s.contains("- cdp_click: Click a CDP-backed element"));
        assert!(s.contains("- ax_click\n"));
    }

    #[test]
    fn user_turn_contains_state_block_and_observation() {
        let wm = WorldModel::default();
        let ts = TaskState::new("ship it".to_string());
        let out = build_user_turn_message(&wm, &ts, 3, "observation text here", &[]);
        assert!(out.contains("<world_model>"));
        assert!(out.contains("<task_state>"));
        assert!(out.contains("observation text here"));
        // State block must appear before the observation.
        let wm_end = out.find("</world_model>").unwrap();
        let obs_start = out.find("observation text here").unwrap();
        assert!(
            wm_end < obs_start,
            "state block must precede the observation"
        );
    }

    #[test]
    fn user_turn_without_observation_omits_observation_tag() {
        let wm = WorldModel::default();
        let ts = TaskState::new("ship it".to_string());
        let out = build_user_turn_message(&wm, &ts, 0, "", &[]);
        assert!(out.contains("<world_model>"));
        assert!(!out.contains("<observation>"));
    }

    #[test]
    fn truncate_summary_short_text_unchanged() {
        assert_eq!(truncate_summary("hello", 10), "hello");
    }

    #[test]
    fn truncate_summary_long_text_truncated() {
        let long = "a".repeat(200);
        let result = truncate_summary(&long, 50);
        assert!(result.len() < 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_summary_multibyte_snaps_to_boundary() {
        // Multi-byte char must not be split mid-sequence.
        let text = "café!";
        let result = truncate_summary(text, 4);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn agent_done_tool_has_required_fields() {
        let tool = agent_done_tool();
        assert_eq!(tool["function"]["name"], "agent_done");
        let required = tool["function"]["parameters"]["required"]
            .as_array()
            .unwrap();
        assert!(required.iter().any(|r| r == "summary"));
    }

    #[test]
    fn agent_replan_tool_has_required_fields() {
        let tool = agent_replan_tool();
        assert_eq!(tool["function"]["name"], "agent_replan");
        let required = tool["function"]["parameters"]["required"]
            .as_array()
            .unwrap();
        assert!(required.iter().any(|r| r == "reason"));
    }
}
