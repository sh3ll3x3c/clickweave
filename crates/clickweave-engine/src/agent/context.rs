use super::prompt::summarize_steps;
use super::types::AgentStep;
use clickweave_llm::{Content, Message};
use std::collections::HashMap;

/// Rough token estimate: ~4 characters per token for English text.
const CHARS_PER_TOKEN: usize = 4;

/// Tools whose results embed a full page snapshot (accessibility tree, DOM
/// snapshot, element listing, wait-for result, or OCR screenshot). Each
/// successive call returns a fresh view of the same page, so retaining
/// older payloads in message history wastes context for no planning benefit.
///
/// The legacy aliases (`wait_for`) are included because tool manifests exposed
/// to the agent sometimes surface both the prefixed CDP form and the short
/// alias for the same underlying behavior.
pub(crate) const SNAPSHOT_PRODUCING_TOOLS: &[&str] = &[
    "cdp_take_ax_snapshot",
    "cdp_take_dom_snapshot",
    "cdp_take_snapshot",
    "cdp_find_elements",
    "cdp_wait_for",
    "take_ax_snapshot",
    "take_screenshot",
    "wait_for",
];

/// Replace the content of a tool-result `Message` with a one-line
/// supersession placeholder. Preserves the `tool_call_id` so the OpenAI
/// tool-call linkage stays intact — stripping it would produce an orphan
/// `tool` message that some providers reject.
fn make_superseded_placeholder(tool_name: &str) -> String {
    format!(
        "[superseded {} result — a newer snapshot of the same page was captured; \
         only the most recent snapshot is retained at full fidelity]",
        tool_name
    )
}

/// Estimate the number of tokens in a string.
pub fn estimate_tokens(text: &str) -> usize {
    // Rough approximation: 1 token ≈ 4 characters
    text.len().div_ceil(CHARS_PER_TOKEN)
}

/// Estimate the total token count across a list of messages.
pub fn estimate_messages_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            let content_len = m.content_text().map_or(0, |t| t.len());
            let tool_calls_len = m.tool_calls.as_ref().map_or(0, |tcs| {
                tcs.iter()
                    .map(|tc| tc.function.name.len() + tc.function.arguments.len())
                    .sum()
            });
            (content_len + tool_calls_len).div_ceil(CHARS_PER_TOKEN)
        })
        .sum()
}

/// Collapse snapshot-producing tool-result payloads that have been superseded
/// by a more recent call to the same tool.
///
/// Snapshot-producing tools (`cdp_take_ax_snapshot`, `cdp_take_dom_snapshot`,
/// `cdp_find_elements`, `cdp_wait_for`, `take_ax_snapshot`, `take_screenshot`,
/// and variants) each embed a full view of the current page in their result.
/// When several such calls occur back-to-back, the older payloads rarely carry
/// planning-relevant information — the newest snapshot reflects the current
/// state of the page. Without supersession, every snapshot stays in history
/// at full size and the prompt grows linearly with tool-call count.
///
/// This function returns a new `Vec<Message>` where, for every
/// snapshot-producing tool in [`SNAPSHOT_PRODUCING_TOOLS`], all but the most
/// recent tool-result body is replaced with a short supersession placeholder.
/// The `tool_call_id` is preserved so the OpenAI tool-call linkage remains
/// valid. Tool-call arguments (on the assistant side) are untouched — they
/// are tiny.
///
/// Returns `None` when no messages would change, so callers can cheaply skip
/// the log line and the copy in the common case.
pub fn collapse_superseded_snapshots(messages: &[Message]) -> Option<Vec<Message>> {
    // Map each tool_call_id to the tool name (from the preceding assistant
    // message). Tool-result messages carry only the id, so we need this
    // mapping to decide whether a result came from a snapshot tool.
    let mut id_to_tool: HashMap<&str, &str> = HashMap::new();
    for msg in messages {
        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                id_to_tool.insert(tc.id.as_str(), tc.function.name.as_str());
            }
        }
    }

    // Find the index of the most recent tool-result for each snapshot tool.
    // "Most recent" means highest message index whose linked tool name matches.
    // We key by tool name (not call id) so that the latest snapshot of any
    // flavor survives even if multiple snapshot tools were invoked.
    let mut latest_index_by_tool: HashMap<&str, usize> = HashMap::new();
    for (idx, msg) in messages.iter().enumerate() {
        if msg.role != "tool" {
            continue;
        }
        let Some(call_id) = msg.tool_call_id.as_deref() else {
            continue;
        };
        let Some(tool_name) = id_to_tool.get(call_id).copied() else {
            continue;
        };
        if SNAPSHOT_PRODUCING_TOOLS.contains(&tool_name) {
            latest_index_by_tool.insert(tool_name, idx);
        }
    }

    if latest_index_by_tool.is_empty() {
        return None;
    }

    // Walk the messages; rewrite any snapshot tool-result that is not the
    // latest for its tool name.
    let mut changed = false;
    let mut out: Vec<Message> = Vec::with_capacity(messages.len());
    for (idx, msg) in messages.iter().enumerate() {
        if msg.role != "tool" {
            out.push(msg.clone());
            continue;
        }
        let Some(call_id) = msg.tool_call_id.as_deref() else {
            out.push(msg.clone());
            continue;
        };
        let Some(tool_name) = id_to_tool.get(call_id).copied() else {
            out.push(msg.clone());
            continue;
        };
        if !SNAPSHOT_PRODUCING_TOOLS.contains(&tool_name) {
            out.push(msg.clone());
            continue;
        }
        let is_latest = latest_index_by_tool
            .get(tool_name)
            .copied()
            .is_some_and(|latest| latest == idx);
        if is_latest {
            out.push(msg.clone());
            continue;
        }

        // Supersede: skip if the body is already a placeholder (idempotence),
        // otherwise rewrite the content and mark the transcript as changed.
        let already_collapsed = msg
            .content_text()
            .is_some_and(|t| t.starts_with("[superseded "));
        if already_collapsed {
            out.push(msg.clone());
            continue;
        }

        let mut replaced = msg.clone();
        replaced.content = Some(Content::Text(make_superseded_placeholder(tool_name)));
        out.push(replaced);
        changed = true;
    }

    if changed { Some(out) } else { None }
}

/// Compact old step details into a summary when the context window is getting full.
///
/// Replaces individual step messages with a compact summary of the oldest steps,
/// keeping the most recent `keep_recent` steps in full detail.
///
/// Returns `None` if no compaction is needed (messages are within budget).
pub fn compact_step_summaries(
    messages: &[Message],
    steps: &[AgentStep],
    token_budget: usize,
    keep_recent: usize,
) -> Option<Vec<Message>> {
    let current_tokens = estimate_messages_tokens(messages);
    if current_tokens <= token_budget {
        return None;
    }

    if steps.len() <= keep_recent {
        // Not enough steps to compact
        return None;
    }

    // Split steps into old (to summarize) and recent (to keep)
    let split_at = steps.len().saturating_sub(keep_recent);
    let old_steps = &steps[..split_at];

    // Build a compact summary of old steps
    let summary = summarize_steps(old_steps);

    // Rebuild messages: system prompt + goal + summary + recent step messages
    let mut compacted = Vec::new();

    // Keep the system message (always first)
    if let Some(system_msg) = messages.first() {
        if system_msg.role == "system" {
            compacted.push(system_msg.clone());
        }
    }

    // Keep the goal message (second message — user-controlled goal text
    // that must survive compaction to keep the LLM on-task).
    if let Some(goal_msg) = messages.get(1) {
        if goal_msg.role == "user" {
            compacted.push(goal_msg.clone());
        }
    }

    // Add compact summary as a user message
    compacted.push(Message::user(summary));

    // LLM steps contribute 3 messages (user observation + assistant tool-call + tool result).
    // Cache-replayed steps contribute 2 (tool-call + tool-result).
    // Use 3 (the maximum across step types) to avoid discarding context prematurely.
    let messages_per_step = 3;
    let recent_message_count = keep_recent * messages_per_step;
    // Start copying from at least index 3 to skip the system message,
    // goal message, and any previously injected summary that were already
    // prepended above. This prevents repeated compaction from accumulating
    // stale summaries. Index 3 is safe because compaction only runs when
    // steps.len() > keep_recent, guaranteeing enough step messages exist.
    let skip = messages.len().saturating_sub(recent_message_count).max(3);
    for msg in messages.iter().skip(skip) {
        compacted.push(msg.clone());
    }

    Some(compacted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::types::{AgentCommand, AgentStep, StepOutcome};
    use clickweave_core::cdp::CdpFindElementMatch;

    #[test]
    fn estimate_tokens_basic() {
        // 12 characters → 3 tokens
        assert_eq!(estimate_tokens("hello world!"), 3);
    }

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_tokens_single_char() {
        assert_eq!(estimate_tokens("a"), 1);
    }

    #[test]
    fn estimate_messages_tokens_sums_content() {
        let messages = vec![
            Message::system("You are a helper."), // 18 chars → 5 tokens
            Message::user("Do something."),       // 13 chars → 4 tokens
        ];
        let total = estimate_messages_tokens(&messages);
        assert!(total > 0);
        assert_eq!(total, 5 + 4);
    }

    fn make_step(index: usize) -> AgentStep {
        AgentStep {
            index,
            elements: vec![CdpFindElementMatch {
                uid: format!("1_{}", index),
                role: "button".to_string(),
                label: "Click me".to_string(),
                tag: "button".to_string(),
                disabled: false,
                parent_role: None,
                parent_name: None,
            }],
            command: AgentCommand::ToolCall {
                tool_name: "click".to_string(),
                arguments: serde_json::json!({"uid": format!("1_{}", index)}),
                tool_call_id: format!("call_{}", index),
            },
            outcome: StepOutcome::Success("Clicked".to_string()),
            page_url: "https://example.com".to_string(),
        }
    }

    #[test]
    fn compact_returns_none_within_budget() {
        let messages = vec![
            Message::system("System prompt"),
            Message::user("Step 0"),
            Message::assistant("Action 0"),
        ];
        let steps = vec![make_step(0)];

        let result = compact_step_summaries(&messages, &steps, 100_000, 2);
        assert!(result.is_none());
    }

    #[test]
    fn compact_returns_none_when_few_steps() {
        let messages = vec![
            Message::system("System prompt"),
            Message::user("Step 0"),
            Message::assistant("Action 0"),
        ];
        let steps = vec![make_step(0)];

        // Budget is tiny but only 1 step which is <= keep_recent
        let result = compact_step_summaries(&messages, &steps, 1, 2);
        assert!(result.is_none());
    }

    #[test]
    fn compact_produces_shorter_messages() {
        // Create enough messages to exceed a small token budget
        let mut messages = vec![Message::system("System prompt")];
        let mut steps = Vec::new();
        for i in 0..10 {
            messages.push(Message::user(format!(
                "Observation step {} with a lot of element details and page info repeated",
                i
            )));
            messages.push(Message::assistant(format!("Action for step {}", i)));
            steps.push(make_step(i));
        }

        // Set a tiny budget to force compaction
        let result = compact_step_summaries(&messages, &steps, 10, 2);
        assert!(result.is_some());
        let compacted = result.unwrap();

        // Compacted should have fewer messages than original
        assert!(compacted.len() < messages.len());

        // Should start with system message
        assert_eq!(compacted[0].role, "system");

        // Should contain a summary message
        let has_summary = compacted.iter().any(|m| {
            m.content_text()
                .map_or(false, |t| t.contains("Previous Steps Summary"))
        });
        assert!(has_summary);
    }

    #[test]
    fn compact_preserves_goal_message() {
        // Simulate: [system, goal, obs0, asst0, tool0, obs1, asst1, tool1, ..., obs9, asst9, tool9]
        let mut messages = vec![
            Message::system("System prompt"),
            Message::user("## Goal\nOpen the calculator app"),
        ];
        let mut steps = Vec::new();
        for i in 0..10 {
            messages.push(Message::user(format!("Observation {}", i)));
            messages.push(Message::assistant(format!("Action {}", i)));
            messages.push(Message::tool_result(&format!("call_{}", i), "ok"));
            steps.push(make_step(i));
        }

        let result = compact_step_summaries(&messages, &steps, 10, 3);
        assert!(result.is_some());
        let compacted = result.unwrap();

        // Goal must survive compaction
        assert!(
            compacted.iter().any(|m| m
                .content_text()
                .map_or(false, |t| t.contains("Open the calculator app"))),
            "Goal message was dropped during compaction"
        );
    }

    #[test]
    fn compact_repeated_does_not_duplicate_goal_or_summary() {
        let mut messages = vec![
            Message::system("System prompt"),
            Message::user("## Goal\nDo the thing"),
        ];
        let mut steps = Vec::new();
        for i in 0..10 {
            messages.push(Message::user(format!("Observation {}", i)));
            messages.push(Message::assistant(format!("Action {}", i)));
            messages.push(Message::tool_result(&format!("call_{}", i), "ok"));
            steps.push(make_step(i));
        }

        // First compaction
        let first = compact_step_summaries(&messages, &steps, 10, 3).unwrap();

        // Second compaction on already-compacted transcript
        let second = compact_step_summaries(&first, &steps, 10, 3).unwrap();

        // Count goal messages — should be exactly 1
        let goal_count = second
            .iter()
            .filter(|m| {
                m.content_text()
                    .map_or(false, |t| t.contains("Do the thing"))
            })
            .count();
        assert_eq!(goal_count, 1, "Goal duplicated after repeated compaction");

        // Count summary messages — should be exactly 1
        let summary_count = second
            .iter()
            .filter(|m| {
                m.content_text()
                    .map_or(false, |t| t.contains("Previous Steps Summary"))
            })
            .count();
        assert_eq!(
            summary_count, 1,
            "Summary duplicated after repeated compaction"
        );
    }

    // -----------------------------------------------------------------
    // Supersession tests
    // -----------------------------------------------------------------

    use clickweave_llm::{FunctionCall, ToolCall};

    /// Build a synthetic (assistant tool_call, tool result) pair for the
    /// given tool name. The result body is large so supersession produces a
    /// measurable token drop.
    fn snapshot_pair(tool_name: &str, call_id: &str, body_kb: usize) -> (Message, Message) {
        let big_body = "x".repeat(body_kb * 1024);
        let assistant = Message::assistant_tool_calls(vec![ToolCall {
            id: call_id.to_string(),
            call_type: "function".to_string(),
            function: FunctionCall {
                name: tool_name.to_string(),
                arguments: "{}".to_string(),
            },
        }]);
        let result = Message::tool_result(call_id, big_body);
        (assistant, result)
    }

    #[test]
    fn collapse_returns_none_when_no_snapshot_tools() {
        let messages = vec![
            Message::system("System"),
            Message::user("Goal"),
            snapshot_pair("click", "call_0", 1).0,
            snapshot_pair("click", "call_0", 1).1,
        ];
        assert!(collapse_superseded_snapshots(&messages).is_none());
    }

    #[test]
    fn collapse_returns_none_with_single_snapshot() {
        // Only one snapshot result in history — nothing to supersede.
        let mut messages = vec![Message::system("System"), Message::user("Goal")];
        let (asst, result) = snapshot_pair("cdp_find_elements", "call_0", 4);
        messages.push(asst);
        messages.push(result);
        assert!(collapse_superseded_snapshots(&messages).is_none());
    }

    #[test]
    fn collapse_keeps_most_recent_snapshot_at_full_fidelity() {
        let mut messages = vec![Message::system("System"), Message::user("Goal")];
        for i in 0..4 {
            let (asst, result) = snapshot_pair("cdp_find_elements", &format!("call_{}", i), 4);
            messages.push(asst);
            messages.push(result);
        }

        let collapsed = collapse_superseded_snapshots(&messages)
            .expect("expected supersession to change the transcript");

        // Same message count: we rewrite in place, never drop.
        assert_eq!(collapsed.len(), messages.len());

        // Locate tool-result messages; all but the last should be placeholders.
        let tool_results: Vec<(usize, &Message)> = collapsed
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "tool")
            .collect();
        assert_eq!(tool_results.len(), 4);

        for (_, m) in &tool_results[..3] {
            let text = m.content_text().expect("placeholder has text");
            assert!(
                text.starts_with("[superseded cdp_find_elements"),
                "older snapshot was not collapsed: {:?}",
                text,
            );
            // tool_call_id must remain for OpenAI linkage.
            assert!(m.tool_call_id.is_some(), "tool_call_id was stripped");
        }

        // The newest snapshot must still have its full body.
        let latest = tool_results.last().unwrap().1;
        let latest_text = latest.content_text().unwrap();
        assert!(
            latest_text.len() > 1024,
            "most recent snapshot was collapsed ({}b)",
            latest_text.len()
        );
    }

    #[test]
    fn collapse_is_idempotent() {
        let mut messages = vec![Message::system("System"), Message::user("Goal")];
        for i in 0..3 {
            let (asst, result) = snapshot_pair("cdp_wait_for", &format!("call_{}", i), 4);
            messages.push(asst);
            messages.push(result);
        }

        let once = collapse_superseded_snapshots(&messages).expect("first pass rewrites");
        let twice = collapse_superseded_snapshots(&once);
        assert!(twice.is_none(), "second pass must be a no-op");
    }

    #[test]
    fn collapse_leaves_most_recent_per_tool_name() {
        // Interleaved snapshot tools. Each tool's own latest must survive,
        // while its older entries are collapsed. The newest snapshot of a
        // different tool must not be collapsed just because a newer snapshot
        // of some other tool arrived afterward.
        let mut messages = vec![Message::system("System"), Message::user("Goal")];
        let specs = [
            ("cdp_find_elements", "a0"),
            ("cdp_take_dom_snapshot", "b0"),
            ("cdp_find_elements", "a1"), // supersedes a0
            ("cdp_wait_for", "c0"),
            ("cdp_take_dom_snapshot", "b1"), // supersedes b0
        ];
        for (tool, id) in specs {
            let (asst, result) = snapshot_pair(tool, id, 2);
            messages.push(asst);
            messages.push(result);
        }

        let collapsed = collapse_superseded_snapshots(&messages)
            .expect("supersession should fire for multi-tool history");

        // Expected collapsed ids: a0 and b0 only.
        let collapsed_ids: Vec<String> = collapsed
            .iter()
            .filter(|m| m.role == "tool")
            .filter(|m| {
                m.content_text()
                    .is_some_and(|t| t.starts_with("[superseded "))
            })
            .filter_map(|m| m.tool_call_id.clone())
            .collect();
        assert_eq!(collapsed_ids, vec!["a0".to_string(), "b0".to_string()]);
    }

    #[test]
    fn collapse_ignores_non_snapshot_tools() {
        // A `click` result should never be collapsed even if it appears
        // before a newer snapshot.
        let mut messages = vec![Message::system("System"), Message::user("Goal")];
        let (asst, result) = snapshot_pair("click", "call_0", 1);
        messages.push(asst);
        messages.push(result);

        // Two snapshots so that supersession does fire on the newer one.
        for i in 0..2 {
            let (asst, result) = snapshot_pair("cdp_find_elements", &format!("snap_{}", i), 2);
            messages.push(asst);
            messages.push(result);
        }

        let collapsed = collapse_superseded_snapshots(&messages).unwrap();

        // The click result must still carry its full original body.
        let click_body = collapsed
            .iter()
            .find(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some("call_0"))
            .and_then(|m| m.content_text().map(|s| s.len()))
            .unwrap();
        assert!(
            click_body > 500,
            "click tool result was incorrectly collapsed (len={})",
            click_body
        );
    }

    #[test]
    fn collapse_bounds_history_tokens_across_many_snapshot_calls() {
        // Regression: without supersession, 8 back-to-back snapshot calls
        // of ~8 KiB each would push retained history well past 10k tokens.
        // With supersession, only the last snapshot keeps its full body,
        // so history must stay well under a sane threshold.
        let mut messages = vec![
            Message::system("You are an agent."),
            Message::user("## Goal\nMulti-step CDP workflow"),
        ];
        for i in 0..8 {
            let (asst, result) = snapshot_pair("cdp_find_elements", &format!("snap_{}", i), 8);
            messages.push(asst);
            messages.push(result);
        }

        let before_tokens = estimate_messages_tokens(&messages);
        let collapsed =
            collapse_superseded_snapshots(&messages).expect("expected collapse to fire");
        let after_tokens = estimate_messages_tokens(&collapsed);

        assert!(
            before_tokens > 10_000,
            "precondition: uncompressed history must be heavy, was {}",
            before_tokens
        );
        // Post-collapse budget. One full 8 KiB snapshot ≈ 2048 tokens; the
        // rest is tiny placeholders + assistant tool-call wrappers.
        assert!(
            after_tokens < 4_000,
            "collapsed history too large: {} tokens (before={})",
            after_tokens,
            before_tokens,
        );
    }
}
