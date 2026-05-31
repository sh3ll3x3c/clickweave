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
    /// Pending approval oneshot sender; held so `cancel()` can send `false`
    /// instead of dropping (dropping surfaces as `ApprovalUnavailable`).
    pending_approval_tx: Arc<Mutex<Option<oneshot::Sender<bool>>>>,
    join: JoinHandle<anyhow::Result<(AgentState, Option<mpsc::Sender<WriteRequest>>)>>,
}

impl AgentRunHandle {
    /// Cancel the in-flight run.
    ///
    /// Cancels the `CancellationToken` **and** sends `false` to any pending
    /// approval oneshot so the engine treats the stop as a rejection/replan
    /// rather than `ApprovalUnavailable`. Dropping the sender is reserved
    /// exclusively for the `Unavailable` decision in the approval bridge.
    pub fn cancel(&self) {
        // Send explicit rejection to pending approval before cancelling the
        // token so the engine's recv sees Ok(false) (replan), not Err
        // (approval_unavailable).
        if let Ok(mut guard) = self.pending_approval_tx.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(false);
        }
        self.cancel.cancel();
    }

    /// Await the runner task to completion.
    pub async fn await_result(
        self,
    ) -> anyhow::Result<(AgentState, Option<mpsc::Sender<WriteRequest>>)> {
        self.join.await?
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
    let pending_approval_tx: Arc<Mutex<Option<oneshot::Sender<bool>>>> = Arc::new(Mutex::new(None));

    let (event_tx, event_rx) = mpsc::channel::<RunnerOutput>(64);
    let (approval_tx, mut approval_rx) =
        mpsc::channel::<(ApprovalRequest, oneshot::Sender<bool>)>(1);

    // Approval bridge task: reads requests from the runner channel, parks the
    // pending oneshot in shared state (so `cancel()` can reject it), then
    // delegates to the responder.
    let pending_tx_for_bridge = Arc::clone(&pending_approval_tx);
    let responder_bridge = Arc::clone(&responder);
    tokio::spawn(async move {
        while let Some((req, tx)) = approval_rx.recv().await {
            // Park the sender so `cancel()` can reject it if called before
            // the responder returns.
            {
                let mut g = pending_tx_for_bridge.lock().unwrap();
                *g = Some(tx);
            }
            // Take it back out — if cancel() already sent false, `take()`
            // returns None and we skip the bridge call.
            let tx = pending_tx_for_bridge.lock().unwrap().take();
            if let Some(tx) = tx {
                crate::approval::bridge_approval(req, tx, responder_bridge.as_ref()).await;
            }
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
                // Cancellation won the race — return a synthetic state.
                use clickweave_engine::agent::trace_graph::AgentTraceGraph;
                use clickweave_engine::agent::{AgentState, TerminalReason};
                let mut state = AgentState::new(AgentTraceGraph::new());
                state.terminal_reason = Some(TerminalReason::ApprovalUnavailable);
                Ok((state, None))
            }
        }
    });

    AgentRunHandle {
        events: event_rx,
        cancel,
        pending_approval_tx,
        join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_engine::agent::test_stubs::{NullMcp, ScriptedLlm, build_agent_done_response};

    use crate::approval::AutoApprove;

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

    #[tokio::test]
    async fn cancel_sends_rejection_not_drop_to_pending_approval() {
        // We test the invariant directly on the pending_approval_tx field:
        // after `cancel()`, a parked oneshot receiver must see Ok(false).
        let (tx, rx) = oneshot::channel::<bool>();
        let pending: Arc<Mutex<Option<oneshot::Sender<bool>>>> = Arc::new(Mutex::new(Some(tx)));
        let cancel = CancellationToken::new();

        let handle = AgentRunHandle {
            events: mpsc::channel(1).1,
            cancel: cancel.clone(),
            pending_approval_tx: Arc::clone(&pending),
            join: tokio::spawn(async {
                use clickweave_engine::agent::trace_graph::AgentTraceGraph;
                Ok((AgentState::new(AgentTraceGraph::new()), None))
            }),
        };

        handle.cancel();

        assert!(
            !rx.await.unwrap(),
            "cancel must send false, not drop the oneshot"
        );
        assert!(cancel.is_cancelled());
    }
}
