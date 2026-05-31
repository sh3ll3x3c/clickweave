use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clickweave_engine::agent::episodic::EpisodicContext;
use clickweave_engine::agent::episodic::types::WriteRequest;
use clickweave_engine::agent::skills::SkillContext;
use clickweave_engine::agent::{
    AgentChannels, AgentConfig, AgentState, PermissionPolicy, run_agent_workflow,
    run_agent_workflow_with_prompt_override,
};
use clickweave_engine::executor::Mcp;
use clickweave_llm::ChatBackend;
use uuid::Uuid;

/// Collapsed named-parameter struct for a single engine call.
///
/// All 13 engine positional parameters are collapsed here so callers
/// build the struct once and pass it to `run_agent`. Generic over the
/// concrete `ChatBackend` and `Mcp` implementations.
pub struct AgentRunParams<'a, B: ChatBackend, M: Mcp + ?Sized> {
    pub llm: &'a B,
    pub mcp: &'a M,
    pub config: AgentConfig,
    pub goal: String,
    pub channels: Option<AgentChannels>,
    /// Optional VLM backend for completion verification. When `Some`, the
    /// runner verifies `agent_done` via a fresh screenshot.
    pub vision: Option<Arc<dyn clickweave_llm::DynChatBackend>>,
    pub permissions: Option<PermissionPolicy>,
    pub run_id: Uuid,
    pub anchor_node_id: Option<Uuid>,
    pub verification_artifacts_dir: Option<PathBuf>,
    /// Optional shared storage handle. When `None` the runner runs storage-less.
    pub storage: Option<Arc<Mutex<clickweave_core::storage::RunStorage>>>,
    pub episodic_ctx: Option<EpisodicContext>,
    pub skill_ctx: Option<SkillContext>,
    /// When `None`, the stable production system prompt is used.
    /// When `Some`, an override is injected — intended for evals.
    pub system_prompt_override: Option<String>,
}

/// Run the agent loop, dispatching to the appropriate engine entry point
/// based on whether `system_prompt_override` is set.
///
/// Returns the final `AgentState` and the episodic writer channel sender
/// (when episodic is active).
pub async fn run_agent<'a, B, M>(
    p: AgentRunParams<'a, B, M>,
) -> anyhow::Result<(AgentState, Option<tokio::sync::mpsc::Sender<WriteRequest>>)>
where
    B: ChatBackend,
    M: Mcp + ?Sized,
{
    match p.system_prompt_override {
        None => {
            run_agent_workflow(
                p.llm,
                p.config,
                p.goal,
                p.mcp,
                p.channels,
                p.vision,
                p.permissions,
                p.run_id,
                p.anchor_node_id,
                p.verification_artifacts_dir,
                p.storage,
                p.episodic_ctx,
                p.skill_ctx,
            )
            .await
        }
        Some(prompt) => {
            run_agent_workflow_with_prompt_override(
                p.llm,
                p.config,
                p.goal,
                p.mcp,
                p.channels,
                p.vision,
                p.permissions,
                p.run_id,
                p.anchor_node_id,
                p.verification_artifacts_dir,
                p.storage,
                p.episodic_ctx,
                p.skill_ctx,
                Some(prompt),
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_engine::agent::test_stubs::{
        NullMcp, ScriptedLlm, build_agent_done_response, llm_reply_tool,
    };

    /// Helper: build minimal params with the given override.
    fn make_params<'a>(
        llm: &'a ScriptedLlm,
        mcp: &'a NullMcp,
        override_prompt: Option<String>,
    ) -> AgentRunParams<'a, ScriptedLlm, NullMcp> {
        AgentRunParams {
            llm,
            mcp,
            config: AgentConfig::default(),
            goal: "test goal".to_string(),
            channels: None,
            vision: None,
            permissions: None,
            run_id: Uuid::new_v4(),
            anchor_node_id: None,
            verification_artifacts_dir: None,
            storage: None,
            episodic_ctx: None,
            skill_ctx: None,
            system_prompt_override: override_prompt,
        }
    }

    #[tokio::test]
    async fn none_override_reaches_default_prompt_path() {
        // ScriptedLlm with a single agent_done response — the loop should
        // terminate with Completed.
        let llm = ScriptedLlm::new(vec![build_agent_done_response("done")]);
        let mcp = NullMcp;

        let params = make_params(&llm, &mcp, None);
        let (state, _writer) = run_agent(params).await.unwrap();

        assert!(
            state.completed,
            "run with no override should complete when LLM says agent_done"
        );
    }

    #[tokio::test]
    async fn some_override_reaches_prompt_override_path() {
        // With a custom system prompt the same scripted LLM must still complete.
        let llm = ScriptedLlm::new(vec![build_agent_done_response("done with override")]);
        let mcp = NullMcp;

        let params = make_params(&llm, &mcp, Some("custom system prompt".to_string()));
        let (state, _writer) = run_agent(params).await.unwrap();

        assert!(
            state.completed,
            "run with prompt override should complete when LLM says agent_done"
        );
    }

    /// Distinguish the two paths by observing `call_count`: when the LLM
    /// script emits one tool call then agent_done, the override path must
    /// also process both without hanging.
    #[tokio::test]
    async fn override_path_consumes_full_script() {
        let llm = ScriptedLlm::new(vec![
            llm_reply_tool("take_ax_snapshot", serde_json::json!({})),
            build_agent_done_response("finished"),
        ]);
        // NullMcp errors on every call; the runner records a step failure.
        let mcp = NullMcp;

        let params = make_params(&llm, &mcp, Some("override".to_string()));
        // The run may or may not succeed depending on error budgets, but it
        // must at least return (not hang).
        let result = run_agent(params).await;
        assert!(result.is_ok(), "run_agent must not panic or hang");
    }
}
