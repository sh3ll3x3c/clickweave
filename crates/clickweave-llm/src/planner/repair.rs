use crate::{ChatBackend, ChatResponse, Message};
use anyhow::{Context, Result, anyhow};
use tracing::{debug, info};

const MAX_REPAIR_ATTEMPTS: usize = 1;

/// Chat with the LLM, retrying once with error feedback on failure.
/// `label` is used for log messages (e.g. "Planner", "Patcher").
/// `process` receives the raw text content and returns Ok(T) or Err to trigger a repair.
pub(crate) async fn chat_with_repair<T>(
    backend: &impl ChatBackend,
    label: &str,
    messages: Vec<Message>,
    mut process: impl FnMut(&str) -> Result<T>,
) -> Result<T> {
    let mut messages = messages;
    let mut last_error: Option<String> = None;

    for attempt in 0..=MAX_REPAIR_ATTEMPTS {
        if let Some(ref err) = last_error {
            info!("Repair attempt {} for {} error: {}", attempt, label, err);
            messages.push(Message::user(format!(
                "Your previous output had an error: {}\n\nPlease fix the JSON and try again. Output ONLY the corrected JSON object.",
                err
            )));
        }

        let response: ChatResponse = backend
            .chat(messages.clone(), None)
            .await
            .context(format!("{} LLM call failed", label))?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No response from {}", label.to_lowercase()))?;

        let content = choice
            .message
            .text_content()
            .ok_or_else(|| anyhow!("{} returned no text content", label))?;

        debug!("{} raw output (attempt {}): {}", label, attempt, content);

        messages.push(Message::assistant(content));

        match process(content) {
            Ok(result) => return Ok(result),
            Err(e) if attempt < MAX_REPAIR_ATTEMPTS => {
                last_error = Some(e.to_string());
            }
            Err(e) => return Err(e),
        }
    }

    Err(anyhow!("{} failed after repair attempts", label))
}
