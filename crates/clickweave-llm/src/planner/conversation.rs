use serde::{Deserialize, Serialize};

/// A single entry in the assistant conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatEntry {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch_summary: Option<PatchSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_context: Option<RunContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
}

/// Compact summary of what a patch did (for conversation context, not the full patch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchSummary {
    pub added: usize,
    pub removed: usize,
    pub updated: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Execution results available at the time of a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunContext {
    pub execution_dir: String,
    pub node_results: Vec<NodeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResult {
    pub node_name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Persistent conversation session for a workflow.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConversationSession {
    pub messages: Vec<ChatEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub summary_cutoff: usize,
}

const DEFAULT_WINDOW_SIZE: usize = 5;

impl ConversationSession {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a user message.
    pub fn push_user(&mut self, content: String, run_context: Option<RunContext>) {
        self.messages.push(ChatEntry {
            role: ChatRole::User,
            content,
            timestamp: now_epoch_ms(),
            patch_summary: None,
            run_context,
        });
    }

    /// Push an assistant message.
    pub fn push_assistant(&mut self, content: String, patch_summary: Option<PatchSummary>) {
        self.messages.push(ChatEntry {
            role: ChatRole::Assistant,
            content,
            timestamp: now_epoch_ms(),
            patch_summary,
            run_context: None,
        });
    }

    /// Messages in the recent window (last N exchanges).
    pub fn recent_window(&self, window_size: Option<usize>) -> &[ChatEntry] {
        let n = window_size.unwrap_or(DEFAULT_WINDOW_SIZE) * 2;
        let len = self.messages.len();
        if len <= n {
            &self.messages[..]
        } else {
            &self.messages[len - n..]
        }
    }

    /// Messages that have aged out of the window but haven't been summarized yet.
    pub fn unsummarized_overflow(&self, window_size: Option<usize>) -> &[ChatEntry] {
        let n = window_size.unwrap_or(DEFAULT_WINDOW_SIZE) * 2;
        let len = self.messages.len();
        if len <= n {
            &[]
        } else {
            let window_start = len - n;
            if window_start > self.summary_cutoff {
                &self.messages[self.summary_cutoff..window_start]
            } else {
                &[]
            }
        }
    }

    /// Whether summarization is needed (overflow exists).
    pub fn needs_summarization(&self, window_size: Option<usize>) -> bool {
        !self.unsummarized_overflow(window_size).is_empty()
    }

    /// Compute the summary_cutoff value for the current message count.
    pub fn current_cutoff(&self, window_size: Option<usize>) -> usize {
        let n = window_size.unwrap_or(DEFAULT_WINDOW_SIZE) * 2;
        let len = self.messages.len();
        if len > n { len - n } else { len }
    }

    /// Update the summary after summarization.
    pub fn set_summary(&mut self, summary: String, window_size: Option<usize>) {
        self.summary = Some(summary);
        self.summary_cutoff = self.current_cutoff(window_size);
    }
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
