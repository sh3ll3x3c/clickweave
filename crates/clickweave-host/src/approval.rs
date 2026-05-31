use async_trait::async_trait;
use clickweave_engine::agent::ApprovalRequest;
use tokio::sync::oneshot;

/// The three possible outcomes when an approval is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    /// Allow the tool call to proceed.
    Approve,
    /// Deny the tool call; the agent replans.
    Reject,
    /// The responder cannot answer (e.g. no TTY). The engine's `recv`
    /// error surfaces as `ApprovalResult::Unavailable` →
    /// `TerminalReason::ApprovalUnavailable`.
    Unavailable,
}

/// Trait implemented by anything that can respond to an approval request.
#[async_trait]
pub trait ApprovalResponder: Send + Sync {
    async fn respond(&self, req: ApprovalRequest) -> ApprovalDecision;
}

/// Responder that always approves every request.
pub struct AutoApprove;

#[async_trait]
impl ApprovalResponder for AutoApprove {
    async fn respond(&self, _req: ApprovalRequest) -> ApprovalDecision {
        ApprovalDecision::Approve
    }
}

/// Map an `ApprovalDecision` onto the engine's oneshot `Sender<bool>`.
///
/// - `Approve` → `send(true)` (continue)
/// - `Reject`  → `send(false)` (replan; run continues)
/// - `Unavailable` → drop the sender (engine recv error → `ApprovalUnavailable`)
pub async fn bridge_approval(
    req: ApprovalRequest,
    tx: oneshot::Sender<bool>,
    responder: &dyn ApprovalResponder,
) {
    let decision = responder.respond(req).await;
    match decision {
        ApprovalDecision::Approve => {
            let _ = tx.send(true);
        }
        ApprovalDecision::Reject => {
            let _ = tx.send(false);
        }
        ApprovalDecision::Unavailable => {
            // Drop tx — the engine's recv will return Err, surfacing
            // TerminalReason::ApprovalUnavailable.
            drop(tx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    async fn approve_sends_true() {
        let (tx, rx) = oneshot::channel::<bool>();
        bridge_approval(make_req(), tx, &AutoApprove).await;
        assert!(rx.await.unwrap());
    }

    #[tokio::test]
    async fn reject_sends_false() {
        struct AlwaysReject;
        #[async_trait]
        impl ApprovalResponder for AlwaysReject {
            async fn respond(&self, _: ApprovalRequest) -> ApprovalDecision {
                ApprovalDecision::Reject
            }
        }
        let (tx, rx) = oneshot::channel::<bool>();
        bridge_approval(make_req(), tx, &AlwaysReject).await;
        assert!(!rx.await.unwrap());
    }

    #[tokio::test]
    async fn unavailable_drops_sender_giving_recv_error() {
        struct Unavailable;
        #[async_trait]
        impl ApprovalResponder for Unavailable {
            async fn respond(&self, _: ApprovalRequest) -> ApprovalDecision {
                ApprovalDecision::Unavailable
            }
        }
        let (tx, rx) = oneshot::channel::<bool>();
        bridge_approval(make_req(), tx, &Unavailable).await;
        // Dropped sender → Err
        assert!(rx.await.is_err(), "Unavailable must drop the sender");
    }

    #[tokio::test]
    async fn auto_approve_always_approves() {
        let responder = AutoApprove;
        let decision = responder.respond(make_req()).await;
        assert_eq!(decision, ApprovalDecision::Approve);
    }
}
