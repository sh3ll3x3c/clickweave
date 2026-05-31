use std::sync::{Arc, Mutex};

use clickweave_engine::agent::episodic::EpisodicContext;
use clickweave_engine::agent::episodic::types::WriteRequest;
use clickweave_engine::agent::skills::SkillContext;
use clickweave_engine::agent::{
    AgentChannels, AgentConfig, AgentState, ApprovalRequest, PermissionPolicy, RunnerOutput,
};
use clickweave_engine::executor::Mcp;
use clickweave_llm::{ChatBackend, DynChatBackend};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::approval::ApprovalResponder;
use crate::run::{AgentRunParams, run_agent};

/// Live-run handle returned by [`spawn_agent_run`].
///
/// The caller drains `events` until the channel closes, then calls
/// `await_result` to collect the final agent state.
pub struct AgentRunHandle {
    /// Receive end of the runner's live event stream.
    pub events: mpsc::Receiver<RunnerOutput>,
    cancel: CancellationToken,
    join: JoinHandle<anyhow::Result<(AgentState, Option<mpsc::Sender<WriteRequest>>)>>,
}

impl AgentRunHandle {
    /// Cancel the in-flight run.
    ///
    /// Cancels the `CancellationToken`. The runner task's outer `select!`
    /// observes the token, abandons `run_agent`, and resolves
    /// [`AgentRunHandle::await_result`] with a synthetic [`AgentState`] whose
    /// `terminal_reason` is `None` — an external stop, mirroring the Tauri
    /// agent task's `agent://stopped` path, never `ApprovalUnavailable`.
    ///
    /// The approval bridge task observes the same token and, if an approval is
    /// in flight, sends `false` to its pending oneshot (defence-in-depth so the
    /// engine never sees a dropped sender → `ApprovalUnavailable`). Dropping the
    /// sender is reserved exclusively for the `Unavailable` decision in the
    /// approval bridge.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Await the runner task to completion.
    pub async fn await_result(
        self,
    ) -> anyhow::Result<(AgentState, Option<mpsc::Sender<WriteRequest>>)> {
        self.join.await?
    }
}

/// Resolve a single in-flight approval, racing the responder's decision
/// against the run's cancellation signal.
///
/// The oneshot `sender` stays owned by this future across the `respond`
/// await, so whichever arm of the `select!` wins still holds it:
///
/// - **responder wins** → apply the `Approve→true / Reject→false /
///   Unavailable→drop` mapping via [`crate::approval::apply_decision`].
/// - **cancel wins** (a `cancel()` arrived while the approval was in flight)
///   → `send(false)`, so the engine's `recv` sees `Ok(false)` (rejection /
///   replan) rather than `Err` (which would surface as
///   `TerminalReason::ApprovalUnavailable`). The sender is **never** dropped
///   on cancel — dropping is reserved exclusively for the responder's own
///   `Unavailable` decision.
async fn serve_one_approval(
    req: ApprovalRequest,
    sender: oneshot::Sender<bool>,
    responder: &dyn ApprovalResponder,
    cancel: &CancellationToken,
) {
    tokio::select! {
        decision = responder.respond(req) => {
            crate::approval::apply_decision(decision, sender);
        }
        _ = cancel.cancelled() => {
            let _ = sender.send(false);
        }
    }
}

/// Spawn an agent run in the background, wiring approval through the bridge.
///
/// Receives all `AgentRunParams` fields as separate owned/`Arc` values,
/// constructs `AgentChannels` inside the spawned task (where `llm` and `mcp`
/// are in scope), and calls `run_agent`. This avoids the self-referential
/// struct problem that a closure-based params builder would introduce.
///
/// Returns an `AgentRunHandle` whose `events` receiver the caller drains.
#[allow(clippy::too_many_arguments)]
pub fn spawn_agent_run<B, M>(
    llm: B,
    mcp: M,
    config: AgentConfig,
    goal: String,
    vision: Option<Arc<dyn DynChatBackend>>,
    permissions: Option<PermissionPolicy>,
    run_id: Uuid,
    anchor_node_id: Option<Uuid>,
    verification_artifacts_dir: Option<std::path::PathBuf>,
    storage: Option<Arc<Mutex<clickweave_core::storage::RunStorage>>>,
    episodic_ctx: Option<EpisodicContext>,
    skill_ctx: Option<SkillContext>,
    system_prompt_override: Option<String>,
    responder: Arc<dyn ApprovalResponder>,
) -> AgentRunHandle
where
    B: ChatBackend + Send + Sync + 'static,
    M: Mcp + Send + Sync + 'static,
{
    let cancel = CancellationToken::new();

    let (event_tx, event_rx) = mpsc::channel::<RunnerOutput>(64);
    let (approval_tx, mut approval_rx) =
        mpsc::channel::<(ApprovalRequest, oneshot::Sender<bool>)>(1);

    // Approval bridge task: reads requests from the runner channel and, for
    // each one, races the responder's decision against the run's
    // cancellation signal. The oneshot sender stays owned by this task across
    // the `respond` await, so whichever arm wins still holds the sender:
    //
    // - responder wins → apply the `Approve→true / Reject→false /
    //   Unavailable→drop` mapping (`apply_decision`).
    // - cancel wins (a `cancel()` arrived while the approval was in flight) →
    //   `send(false)`, so the engine sees `Ok(false)` (rejection/replan)
    //   rather than `Err` (`ApprovalUnavailable`). The sender is never
    //   dropped on cancel.
    //
    // The bridge terminates when `approval_rx` closes (the runner dropped its
    // `approval_tx`) — a cancellation does not stop the bridge itself, it only
    // resolves the in-flight approval; the runner task observes the same token
    // and unwinds the run.
    let cancel_for_bridge = cancel.clone();
    let responder_bridge = Arc::clone(&responder);
    tokio::spawn(async move {
        while let Some((req, tx)) = approval_rx.recv().await {
            serve_one_approval(req, tx, responder_bridge.as_ref(), &cancel_for_bridge).await;
        }
    });

    // Clone the cancel token for use inside the task.
    let cancel_for_task = cancel.clone();

    // Runner task: owns `llm` and `mcp`. Constructs `AgentChannels` and
    // `AgentRunParams` here where both backends are in scope as owned values.
    // Uses `tokio::select!` to honour cancellation while `run_agent` is in
    // progress — matching the Tauri task's `agent_token.cancelled()` pattern.
    let join = tokio::spawn(async move {
        let channels = AgentChannels {
            event_tx,
            approval_tx,
        };
        let params = AgentRunParams {
            llm: &llm,
            mcp: &mcp,
            config,
            goal,
            channels: Some(channels),
            vision,
            permissions,
            run_id,
            anchor_node_id,
            verification_artifacts_dir,
            storage,
            episodic_ctx,
            skill_ctx,
            system_prompt_override,
        };
        tokio::select! {
            res = run_agent(params) => res,
            _ = cancel_for_task.cancelled() => {
                // Cancellation won the race — abandon `run_agent` and return a
                // synthetic state carrying *no* terminal reason. A cancel is an
                // external stop, not a loop outcome, so it must not be labelled
                // with any `TerminalReason` (least of all `ApprovalUnavailable`,
                // whose meaning is "the approval channel is permanently gone").
                // This mirrors the Tauri agent task, which represents a cancel
                // out-of-band via the `agent://stopped` event and never produces
                // a `terminal_reason`. `AgentState::new` already defaults
                // `terminal_reason` to `None`.
                use clickweave_engine::agent::trace_graph::AgentTraceGraph;
                use clickweave_engine::agent::AgentState;
                let state = AgentState::new(AgentTraceGraph::new());
                Ok((state, None))
            }
        }
    });

    AgentRunHandle {
        events: event_rx,
        cancel,
        join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_engine::agent::test_stubs::{NullMcp, ScriptedLlm, build_agent_done_response};

    use crate::approval::{ApprovalDecision, AutoApprove};

    fn default_responder() -> Arc<dyn ApprovalResponder> {
        Arc::new(AutoApprove)
    }

    fn spawn_simple<B: ChatBackend + Send + Sync + 'static, M: Mcp + Send + Sync + 'static>(
        llm: B,
        mcp: M,
    ) -> AgentRunHandle {
        spawn_agent_run(
            llm,
            mcp,
            AgentConfig::default(),
            "test goal".to_string(),
            None,
            None,
            Uuid::new_v4(),
            None,
            None,
            None,
            None,
            None,
            None,
            default_responder(),
        )
    }

    #[tokio::test]
    async fn events_drain_to_completion() {
        let llm = ScriptedLlm::new(vec![build_agent_done_response("finished")]);
        let mcp = NullMcp;

        let handle = spawn_simple(llm, mcp);
        let AgentRunHandle {
            mut events, join, ..
        } = handle;
        while events.recv().await.is_some() {}
        let result = join.await.unwrap();
        assert!(result.is_ok(), "run must complete without error");
    }

    #[tokio::test]
    async fn cancel_before_completion_resolves_join() {
        use clickweave_engine::agent::test_stubs::{StaticMcp, llm_reply_tool};

        // A repeating LLM that would run forever without cancellation.
        let llm = ScriptedLlm::repeat(|| llm_reply_tool("take_ax_snapshot", serde_json::json!({})));
        let mcp = StaticMcp::with_tools(&["take_ax_snapshot"]);

        let handle = spawn_agent_run(
            llm,
            mcp,
            AgentConfig {
                max_steps: 1000,
                ..Default::default()
            },
            "loop forever".to_string(),
            None,
            None,
            Uuid::new_v4(),
            None,
            None,
            None,
            None,
            None,
            None,
            default_responder(),
        );

        handle.cancel();

        let result = handle.await_result().await;
        assert!(result.is_ok(), "cancelled run must return Ok");
    }

    /// Responder that signals once it is asked (so a test can observe that
    /// an approval is genuinely in flight) and then parks forever, never
    /// returning a decision. Models a TTY prompt blocked on operator input.
    struct ParkingResponder {
        entered_tx: Mutex<Option<oneshot::Sender<()>>>,
    }

    impl ParkingResponder {
        /// Returns the responder plus a receiver that fires when `respond`
        /// is first entered.
        fn new() -> (Arc<Self>, oneshot::Receiver<()>) {
            let (entered_tx, entered_rx) = oneshot::channel();
            let responder = Arc::new(Self {
                entered_tx: Mutex::new(Some(entered_tx)),
            });
            (responder, entered_rx)
        }
    }

    #[async_trait::async_trait]
    impl ApprovalResponder for ParkingResponder {
        async fn respond(&self, _req: ApprovalRequest) -> ApprovalDecision {
            if let Some(tx) = self.entered_tx.lock().unwrap().take() {
                let _ = tx.send(());
            }
            // Park forever: only cancellation can resolve the in-flight
            // approval. `pending()` never completes, so if the bridge ever
            // dropped the sender instead of racing the token, the engine's
            // recv would error → ApprovalUnavailable — the bug this guards.
            std::future::pending::<()>().await;
            unreachable!("ParkingResponder::respond never returns");
        }
    }

    fn approval_request() -> ApprovalRequest {
        ApprovalRequest {
            step_index: 0,
            tool_name: "destructive_tool".to_string(),
            arguments: serde_json::json!({}),
            description: "do something".to_string(),
        }
    }

    /// Regression: when `cancel()` fires while an approval is parked inside
    /// the responder, the bridge must `send(false)` to the engine's oneshot
    /// — not drop it. A dropped sender makes the engine's `recv` error,
    /// surfacing as `TerminalReason::ApprovalUnavailable`; an explicit
    /// `Ok(false)` lets the engine treat the stop as a rejection/replan.
    ///
    /// This drives the real bridge primitive (`serve_one_approval`) — the
    /// exact future the bridge task awaits per request — so it covers the
    /// live cancel-during-approval path, not a field-level stand-in.
    #[tokio::test]
    async fn cancel_during_in_flight_approval_sends_rejection_through_bridge() {
        let (responder, entered_rx) = ParkingResponder::new();
        let cancel = CancellationToken::new();
        let (engine_tx, engine_rx) = oneshot::channel::<bool>();

        let cancel_for_bridge = cancel.clone();
        let responder_for_bridge = Arc::clone(&responder);
        let bridge = tokio::spawn(async move {
            serve_one_approval(
                approval_request(),
                engine_tx,
                responder_for_bridge.as_ref(),
                &cancel_for_bridge,
            )
            .await;
        });

        // Wait until the responder is genuinely parked so the cancel races a
        // real in-flight approval (the buggy window).
        entered_rx
            .await
            .expect("responder must report it was entered");

        cancel.cancel();

        // The engine-side receiver must observe an explicit rejection, never
        // an error from a dropped sender. The bounded timeout turns the buggy
        // behavior (sender dropped / never sent → recv hangs or errors) into a
        // deterministic failure instead of a hang.
        let received = tokio::time::timeout(std::time::Duration::from_secs(5), engine_rx)
            .await
            .expect("engine receiver must resolve on cancel, not hang");
        assert_eq!(
            received,
            Ok(false),
            "cancel during an in-flight approval must send false, not drop the sender",
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), bridge)
            .await
            .expect("bridge future must resolve, not hang")
            .expect("bridge task must not panic");
    }

    #[tokio::test]
    async fn serve_one_approval_maps_approve_to_true() {
        let cancel = CancellationToken::new();
        let (tx, rx) = oneshot::channel::<bool>();
        serve_one_approval(approval_request(), tx, &AutoApprove, &cancel).await;
        assert_eq!(rx.await, Ok(true), "Approve must send true");
    }

    #[tokio::test]
    async fn serve_one_approval_maps_reject_to_false() {
        struct AlwaysReject;
        #[async_trait::async_trait]
        impl ApprovalResponder for AlwaysReject {
            async fn respond(&self, _: ApprovalRequest) -> ApprovalDecision {
                ApprovalDecision::Reject
            }
        }
        let cancel = CancellationToken::new();
        let (tx, rx) = oneshot::channel::<bool>();
        serve_one_approval(approval_request(), tx, &AlwaysReject, &cancel).await;
        assert_eq!(rx.await, Ok(false), "Reject must send false");
    }

    #[tokio::test]
    async fn serve_one_approval_drops_sender_on_unavailable() {
        struct AlwaysUnavailable;
        #[async_trait::async_trait]
        impl ApprovalResponder for AlwaysUnavailable {
            async fn respond(&self, _: ApprovalRequest) -> ApprovalDecision {
                ApprovalDecision::Unavailable
            }
        }
        let cancel = CancellationToken::new();
        let (tx, rx) = oneshot::channel::<bool>();
        serve_one_approval(approval_request(), tx, &AlwaysUnavailable, &cancel).await;
        assert!(
            rx.await.is_err(),
            "Unavailable must drop the sender (engine recv → ApprovalUnavailable)"
        );
    }

    /// End-to-end: spawning a run whose only tool is approval-gated, then
    /// cancelling while the responder is parked on the approval, must resolve
    /// and join the run without hanging. Exercises the full bridge wiring in
    /// `spawn_agent_run` (not just the extracted primitive).
    #[tokio::test]
    async fn cancel_during_in_flight_approval_through_spawn_resolves() {
        use clickweave_engine::agent::test_stubs::{StaticMcp, llm_reply_tool};

        let (responder, entered_rx) = ParkingResponder::new();

        // The scripted LLM keeps asking for an approval-gated tool. With the
        // default permission policy and no read-only hint, the engine
        // resolves the call to `Ask` and routes it through the bridge.
        let llm = ScriptedLlm::repeat(|| llm_reply_tool("destructive_tool", serde_json::json!({})));
        let mcp = StaticMcp::with_tools(&["destructive_tool"]);

        let handle = spawn_agent_run(
            llm,
            mcp,
            AgentConfig {
                max_steps: 1000,
                ..Default::default()
            },
            "needs approval".to_string(),
            None,
            Some(PermissionPolicy::default()),
            Uuid::new_v4(),
            None,
            None,
            None,
            None,
            None,
            None,
            responder as Arc<dyn ApprovalResponder>,
        );

        // Once the responder is parked, an approval is genuinely in flight.
        entered_rx
            .await
            .expect("responder must report the approval is in flight");

        handle.cancel();

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), handle.await_result())
            .await
            .expect("cancelled run must resolve, not deadlock");
        let (state, _) = result.expect("cancelled run must return Ok");
        assert!(
            state.terminal_reason.is_none(),
            "a cancel is an external stop, not a loop outcome — it must carry \
             no terminal reason (never Some(ApprovalUnavailable))",
        );
    }
}
