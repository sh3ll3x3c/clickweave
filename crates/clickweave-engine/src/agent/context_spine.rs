//! Transcript compaction for the state-spine runner.
//!
//! Rules (D12):
//! - `messages[0]` (system prompt) — never compacted.
//! - `messages[1]` (goal, with prior_turns + variant context inlined) — never compacted.
//! - Last `recent_n` assistant/tool pairs — preserved verbatim.
//! - Beyond `recent_n` — collapsed to a brief harness-authored line.
//! - Snapshot tool-result messages older than the current step are dropped.
//!
//! Continuity data lives in `WorldModel`; the transcript no longer carries it.
//!
//! Phase 2b: this module is dormant — nothing in the live runner imports it.
//! Wiring lands in Phase 3 (cutover), at which point the old `context.rs` is
//! deleted and this file is renamed `context.rs`.

#![allow(dead_code)] // Phase 2b: module is dormant; live consumers land in Phase 3 cutover.

use clickweave_llm::{Content, Message, Role};

/// Tool names whose results are snapshot-family. Bodies older than the
/// current step get dropped entirely from the transcript.
const SNAPSHOT_TOOL_NAMES: &[&str] = &[
    "take_ax_snapshot",
    "take_screenshot",
    "cdp_take_ax_snapshot",
    "cdp_take_dom_snapshot",
    "cdp_take_snapshot",
    "cdp_find_elements",
    "cdp_wait_for",
    "wait_for",
];

#[derive(Debug, Clone)]
pub struct CompactBudget {
    pub max_tokens: usize,
    pub recent_n: usize,
}

impl Default for CompactBudget {
    fn default() -> Self {
        Self {
            max_tokens: 100_000,
            recent_n: 6,
        }
    }
}

/// Compact a chat-history vector under the state-spine rules.
///
/// Invariants:
/// - `messages[0]` (system) and `messages[1]` (goal) are never modified.
/// - The last `budget.recent_n` assistant/tool pairs are preserved verbatim,
///   except that snapshot-family tool-result bodies older than the current
///   step are replaced by a short placeholder (body dropped).
/// - All pairs older than the recent-N window are collapsed to a single
///   brief assistant-authored summary line.
/// - If the total token estimate still exceeds `budget.max_tokens`, the
///   largest surviving body is truncated in-place until the estimate fits
///   or nothing is left to collapse.
pub fn compact(messages: Vec<Message>, budget: &CompactBudget) -> Vec<Message> {
    if messages.len() <= 2 {
        return messages;
    }

    let mut out = Vec::with_capacity(messages.len());
    out.push(messages[0].clone());
    out.push(messages[1].clone());

    // Pair up assistant + tool-result messages after messages[1].
    // Each "pair" is one assistant message followed by its tool-result(s).
    let tail = &messages[2..];
    let pairs = group_into_pairs(tail);

    let total_pairs = pairs.len();
    let recent_start = total_pairs.saturating_sub(budget.recent_n);
    // The "current step" is the last pair. Any snapshot-family tool-result
    // in an earlier pair is stale — the state block carries the fresh view
    // and the body should be dropped outright, independent of the recent-N
    // window and the token budget.
    let current_step_idx = total_pairs.saturating_sub(1);

    for (i, pair) in pairs.iter().enumerate() {
        let is_current_step = i == current_step_idx;
        if i >= recent_start {
            // Recent-N window: keep verbatim, but drop stale snapshot bodies.
            for m in pair {
                if !is_current_step && is_snapshot_tool_result(m) {
                    out.push(drop_snapshot_body(m));
                } else {
                    out.push(m.clone());
                }
            }
        } else {
            // Collapse: one brief summary line instead of the full pair.
            out.push(collapse_pair_to_brief(pair));
        }
    }

    // If we are still over budget, collapse more-recent pairs (after the
    // protected system + goal) until we fit.
    enforce_token_budget(out, budget)
}

/// Replace a snapshot-family tool-result body with a short placeholder,
/// preserving `role`, `tool_call_id`, and `name` so OpenAI tool-call
/// linkage stays intact.
fn drop_snapshot_body(m: &Message) -> Message {
    let placeholder = format!(
        "[{}: body dropped (older snapshot)]",
        m.name.as_deref().unwrap_or("snapshot")
    );
    Message {
        role: m.role,
        content: Some(Content::Text(placeholder)),
        reasoning_content: m.reasoning_content.clone(),
        tool_calls: m.tool_calls.clone(),
        tool_call_id: m.tool_call_id.clone(),
        name: m.name.clone(),
    }
}

fn group_into_pairs(messages: &[Message]) -> Vec<Vec<Message>> {
    let mut pairs: Vec<Vec<Message>> = Vec::new();
    let mut current: Vec<Message> = Vec::new();
    for m in messages {
        if m.role == Role::Assistant {
            if !current.is_empty() {
                pairs.push(std::mem::take(&mut current));
            }
            current.push(m.clone());
        } else {
            current.push(m.clone());
        }
    }
    if !current.is_empty() {
        pairs.push(current);
    }
    pairs
}

fn collapse_pair_to_brief(pair: &[Message]) -> Message {
    let asst = pair.iter().find(|m| m.role == Role::Assistant);
    let tool = pair.iter().find(|m| m.role == Role::Tool);
    let asst_kind = asst
        .and_then(|m| m.tool_calls.as_ref())
        .and_then(|tcs| tcs.first())
        .map(|tc| tc.function.name.clone())
        .unwrap_or_else(|| "text".to_string());
    let tool_kind = tool
        .and_then(|m| m.name.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let outcome = tool
        .and_then(|m| m.content_text().map(|t| truncate(t, 120)))
        .unwrap_or_default();
    Message {
        role: Role::Assistant,
        content: Some(Content::Text(format!(
            "[collapsed] action={} tool={} outcome={}",
            asst_kind, tool_kind, outcome
        ))),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
        name: None,
    }
}

fn truncate(s: &str, cap: usize) -> String {
    if s.len() <= cap {
        return s.to_string();
    }
    // Walk down to a UTF-8 char boundary so multibyte content
    // (tool outputs, UI labels) never panics. Matches the existing
    // `prompt.rs::truncate_summary` floor_char_boundary discipline.
    let mut boundary = cap;
    while boundary > 0 && !s.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}…", &s[..boundary])
}

fn is_snapshot_tool_result(m: &Message) -> bool {
    if m.role != Role::Tool {
        return false;
    }
    match m.name.as_deref() {
        Some(n) => SNAPSHOT_TOOL_NAMES.contains(&n),
        None => false,
    }
}

fn content_len(m: &Message) -> usize {
    m.content_text().map_or(0, |t| t.len())
}

fn enforce_token_budget(mut messages: Vec<Message>, budget: &CompactBudget) -> Vec<Message> {
    // Rough estimate: 4 characters per token (matches `context::estimate_tokens`).
    let est = |m: &Message| content_len(m) / 4 + 4;
    loop {
        let total: usize = messages.iter().map(est).sum();
        if total <= budget.max_tokens {
            return messages;
        }
        // Collapse the oldest non-system, non-goal body that still has a
        // meaningful payload (ignore bodies already collapsed by a prior
        // pass to guarantee progress).
        let collapse_idx = messages
            .iter()
            .enumerate()
            .skip(2)
            .find(|(_, m)| {
                let text = m.content_text().unwrap_or("");
                text.len() > 200
                    && !text.starts_with("[collapsed")
                    && !text.starts_with("[collapsed to fit budget]")
            })
            .map(|(i, _)| i);
        match collapse_idx {
            Some(i) => {
                let text = messages[i].content_text().unwrap_or("").to_string();
                let shortened = format!("[collapsed to fit budget] {}", truncate(&text, 80));
                messages[i].content = Some(Content::Text(shortened));
            }
            None => return messages, // cannot compact further
        }
    }
}

#[cfg(test)]
mod state_spine_compact_tests {
    use super::*;
    use clickweave_llm::{Content, FunctionCall, Message, Role, ToolCall};
    use serde_json::Value;

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: Some(Content::Text(content.to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    fn assistant_call(tool_name: &str, call_id: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: None,
            reasoning_content: None,
            tool_calls: Some(vec![ToolCall {
                id: call_id.to_string(),
                call_type: Default::default(),
                function: FunctionCall {
                    name: tool_name.to_string(),
                    arguments: Value::Object(Default::default()),
                },
            }]),
            tool_call_id: None,
            name: None,
        }
    }

    fn tool_result(name: &str, body: &str) -> Message {
        Message {
            role: Role::Tool,
            content: Some(Content::Text(body.to_string())),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some("tc-1".to_string()),
            name: Some(name.to_string()),
        }
    }

    fn content_of(m: &Message) -> &str {
        m.content_text().unwrap_or("")
    }

    #[test]
    fn system_and_goal_never_compacted() {
        let messages = vec![
            msg(Role::System, "system prompt"),
            msg(Role::User, "goal text"),
            msg(Role::Assistant, "I will start."),
            tool_result("cdp_click", "ok"),
        ];
        let budget = CompactBudget {
            max_tokens: 16,
            recent_n: 1,
        };
        let out = compact(messages.clone(), &budget);
        assert_eq!(content_of(&out[0]), "system prompt");
        assert_eq!(content_of(&out[1]), "goal text");
    }

    #[test]
    fn drops_snapshot_tool_result_bodies_even_when_budget_is_huge() {
        // A snapshot tool-result older than the current step must be dropped,
        // independent of budget. The state-block in the current user turn
        // supersedes it.
        let long_body = "uid=a1g3 button\n".repeat(500);
        let messages = vec![
            msg(Role::System, "sys"),
            msg(Role::User, "goal"),
            msg(Role::Assistant, "take snapshot"),
            tool_result("take_ax_snapshot", &long_body),
            msg(Role::Assistant, "click"),
            tool_result("ax_click", "ok"),
        ];
        let budget = CompactBudget {
            max_tokens: 100_000,
            recent_n: 2,
        };
        let out = compact(messages, &budget);
        let has_full_ax_body = out
            .iter()
            .any(|m| m.name.as_deref() == Some("take_ax_snapshot") && content_of(m).len() > 200);
        assert!(
            !has_full_ax_body,
            "old snapshot tool-result bodies must be dropped, not merely collapsed"
        );
    }

    #[test]
    fn recent_n_pairs_preserved_verbatim_when_under_budget() {
        let messages = vec![
            msg(Role::System, "sys"),
            msg(Role::User, "goal"),
            msg(Role::Assistant, "a1"),
            tool_result("cdp_click", "r1"),
            msg(Role::Assistant, "a2"),
            tool_result("cdp_click", "r2"),
        ];
        let budget = CompactBudget {
            max_tokens: 10_000,
            recent_n: 2,
        };
        let out = compact(messages, &budget);
        assert!(out.iter().any(|m| content_of(m) == "a1"));
        assert!(out.iter().any(|m| content_of(m) == "a2"));
    }

    #[test]
    fn beyond_recent_n_pairs_collapse_to_brief_summaries() {
        let mut messages = vec![msg(Role::System, "sys"), msg(Role::User, "goal")];
        for i in 0..10 {
            messages.push(msg(Role::Assistant, &format!("a{}", i)));
            messages.push(tool_result("cdp_click", &format!("r{}", i)));
        }
        let budget = CompactBudget {
            max_tokens: 2_000,
            recent_n: 2,
        };
        let out = compact(messages, &budget);
        // Oldest pairs must be collapsed — we should not see the full
        // "a0" assistant content in the output.
        let has_a0 = out.iter().any(|m| content_of(m) == "a0");
        assert!(!has_a0, "oldest assistant pair must be collapsed");
        // But the most recent 2 pairs must be present verbatim.
        assert!(out.iter().any(|m| content_of(m) == "a8"));
        assert!(out.iter().any(|m| content_of(m) == "a9"));
    }

    #[test]
    fn cross_phase_snapshot_family_does_not_accumulate_bodies() {
        let long = "x".repeat(5_000);
        let messages = vec![
            msg(Role::System, "sys"),
            msg(Role::User, "goal"),
            msg(Role::Assistant, "ax"),
            tool_result("take_ax_snapshot", &long),
            msg(Role::Assistant, "dom"),
            tool_result("cdp_take_dom_snapshot", &long),
            msg(Role::Assistant, "find"),
            tool_result("cdp_find_elements", &long),
        ];
        let budget = CompactBudget {
            max_tokens: 500_000,
            recent_n: 3,
        };
        let out = compact(messages, &budget);
        let total: usize = out.iter().map(content_len).sum();
        assert!(
            total < 50_000,
            "no snapshot-family body should survive verbatim into the compacted output; got {} chars",
            total
        );
    }

    #[test]
    fn collapsed_summary_surfaces_action_and_tool_names() {
        // When a pair is collapsed beyond the recent-N window, the brief
        // summary line should name the assistant's tool_call and the tool
        // result it paired with, so the LLM retains the ordering even
        // without the full bodies.
        let messages = vec![
            msg(Role::System, "sys"),
            msg(Role::User, "goal"),
            assistant_call("cdp_click", "tc-collapse"),
            tool_result("cdp_click", "r-old"),
            msg(Role::Assistant, "a-recent"),
            tool_result("cdp_click", "r-recent"),
        ];
        let budget = CompactBudget {
            max_tokens: 10_000,
            recent_n: 1,
        };
        let out = compact(messages, &budget);
        let collapsed = out
            .iter()
            .find(|m| content_of(m).starts_with("[collapsed]"))
            .expect("should contain a collapsed summary");
        let text = content_of(collapsed);
        assert!(
            text.contains("cdp_click"),
            "collapsed summary should mention the tool name; got: {text}"
        );
    }

    #[test]
    fn truncate_is_utf8_boundary_safe() {
        // Crafted so a naive byte-slice would land mid-multibyte, which
        // would panic. The cap lands mid-ellipsis char → helper must walk
        // down to a char boundary before slicing.
        let s = "aa…bbb";
        let out = truncate(s, 3);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn enforce_token_budget_shrinks_oversized_bodies() {
        let huge = "y".repeat(20_000);
        let messages = vec![
            msg(Role::System, "sys"),
            msg(Role::User, "goal"),
            msg(Role::Assistant, &huge),
            tool_result("cdp_click", "ok"),
        ];
        let budget = CompactBudget {
            max_tokens: 100,
            recent_n: 5,
        };
        let out = compact(messages, &budget);
        let total_chars: usize = out.iter().map(content_len).sum();
        assert!(
            total_chars < 5_000,
            "enforce_token_budget should shrink oversized bodies; got {total_chars} chars"
        );
        // System + goal are still the first two.
        assert_eq!(content_of(&out[0]), "sys");
        assert_eq!(content_of(&out[1]), "goal");
    }

    #[test]
    fn short_histories_are_returned_unchanged() {
        let messages = vec![msg(Role::System, "sys"), msg(Role::User, "goal")];
        let budget = CompactBudget {
            max_tokens: 10,
            recent_n: 0,
        };
        let out = compact(messages.clone(), &budget);
        assert_eq!(out.len(), 2);
        assert_eq!(content_of(&out[0]), "sys");
        assert_eq!(content_of(&out[1]), "goal");
    }
}
