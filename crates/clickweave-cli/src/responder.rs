use std::io::IsTerminal;

use async_trait::async_trait;
use clickweave_host::{
    ApprovalRequest,
    approval::{ApprovalDecision, ApprovalResponder},
};

/// TTY-aware responder for the CLI.
///
/// Decision table:
///   - `--yes` / `--allow-all` → always `Approve`
///   - non-TTY stdin, no `--yes`/`--allow-all` → `Unavailable`
///   - TTY stdin → interactive prompt (Approve / Reject)
pub struct StdinResponder {
    /// When true, always approve without prompting.
    auto_approve: bool,
    /// Whether stdin is a TTY.
    is_tty: bool,
}

impl StdinResponder {
    /// Build a `StdinResponder` from CLI flags.
    ///
    /// `auto_approve` is true when `--yes` or `--allow-all` was passed.
    pub fn new(auto_approve: bool) -> Self {
        let is_tty = std::io::stdin().is_terminal();
        Self {
            auto_approve,
            is_tty,
        }
    }

    /// Override the TTY check (for tests).
    #[cfg(test)]
    pub fn with_tty(auto_approve: bool, is_tty: bool) -> Self {
        Self {
            auto_approve,
            is_tty,
        }
    }
}

#[async_trait]
impl ApprovalResponder for StdinResponder {
    async fn respond(&self, req: ApprovalRequest) -> ApprovalDecision {
        if self.auto_approve {
            return ApprovalDecision::Approve;
        }
        if !self.is_tty {
            return ApprovalDecision::Unavailable;
        }

        // Interactive TTY prompt.
        use std::io::{BufRead, Write};
        let stderr = std::io::stderr();
        let mut out = stderr.lock();
        let _ = writeln!(
            out,
            "\nApproval required for `{}` (step {}):",
            req.tool_name, req.step_index
        );
        let _ = writeln!(out, "  Description: {}", req.description);
        let _ = write!(out, "  Allow? [y/N] ");
        drop(out);

        let stdin = std::io::stdin();
        let mut line = String::new();
        let _ = stdin.lock().read_line(&mut line);
        let trimmed = line.trim().to_lowercase();
        if trimmed == "y" || trimmed == "yes" {
            ApprovalDecision::Approve
        } else {
            ApprovalDecision::Reject
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_host::ApprovalRequest;
    use serde_json::json;

    fn make_req() -> ApprovalRequest {
        ApprovalRequest {
            step_index: 0,
            tool_name: "some_tool".to_string(),
            arguments: json!({}),
            description: "do something".to_string(),
        }
    }

    #[tokio::test]
    async fn auto_approve_always_returns_approve() {
        let responder = StdinResponder::with_tty(true, false);
        let decision = responder.respond(make_req()).await;
        assert_eq!(decision, ApprovalDecision::Approve);
    }

    #[tokio::test]
    async fn non_tty_without_auto_approve_returns_unavailable() {
        let responder = StdinResponder::with_tty(false, false);
        let decision = responder.respond(make_req()).await;
        assert_eq!(decision, ApprovalDecision::Unavailable);
    }
}
