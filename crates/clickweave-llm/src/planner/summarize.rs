use super::conversation::{ChatEntry, ChatRole, ConversationSession};
use crate::{ChatBackend, Message};
use anyhow::Result;
use tracing::info;

/// Build a compact text representation of chat entries for the summarizer.
fn entries_to_text(entries: &[ChatEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            let role = match e.role {
                ChatRole::User => "User",
                ChatRole::Assistant => "Assistant",
            };
            let mut line = format!("{}: {}", role, e.content);
            if let Some(ps) = &e.patch_summary {
                line.push_str(&format!(
                    " [patch: +{} -{} ~{}]",
                    ps.added, ps.removed, ps.updated
                ));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Summarize older conversation entries using the LLM.
///
/// Returns the new summary string. The caller is responsible for calling
/// `session.set_summary()` with the result.
pub async fn summarize_overflow(
    backend: &impl ChatBackend,
    session: &ConversationSession,
    window_size: Option<usize>,
) -> Result<String> {
    let overflow = session.unsummarized_overflow(window_size);
    if overflow.is_empty() {
        return Ok(session.summary.clone().unwrap_or_default());
    }

    let overflow_text = entries_to_text(overflow);

    let prompt = if let Some(existing) = &session.summary {
        format!(
            "You are summarizing a conversation about workflow modifications for a UI automation tool.\n\n\
             Previous summary:\n{}\n\n\
             New messages to incorporate:\n{}\n\n\
             Write a concise updated summary (2-5 sentences). Focus on: what the user wanted, \
             what was changed, what problems were encountered. Output ONLY the summary text.",
            existing, overflow_text
        )
    } else {
        format!(
            "You are summarizing a conversation about workflow modifications for a UI automation tool.\n\n\
             Messages:\n{}\n\n\
             Write a concise summary (2-5 sentences). Focus on: what the user wanted, \
             what was changed, what problems were encountered. Output ONLY the summary text.",
            overflow_text
        )
    };

    info!(
        overflow_count = overflow.len(),
        "Summarizing conversation overflow"
    );

    let messages = vec![Message::user(&prompt)];

    let response = backend.chat(messages, None).await?;

    let text = response
        .choices
        .first()
        .and_then(|c| c.message.content_text())
        .unwrap_or("")
        .to_string();

    Ok(text)
}
