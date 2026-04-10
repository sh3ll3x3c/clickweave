mod cache;
mod context;
mod loop_runner;
mod prompt;
mod recovery;
mod transition;
mod types;

pub use loop_runner::AgentRunner;
pub use types::*;

use clickweave_llm::ChatBackend;
use clickweave_mcp::McpClient;

/// Public entry point for running the agent loop from outside the engine crate.
///
/// This wraps `AgentRunner::run` and resolves the `pub(crate)` Mcp trait
/// boundary so that callers (e.g. Tauri commands) can pass a `McpClient`
/// directly.
///
/// When `cache` is `Some`, the runner is seeded with cross-run decisions.
/// Returns both the final agent state and the (possibly updated) cache.
pub async fn run_agent_workflow(
    llm: &impl ChatBackend,
    config: AgentConfig,
    goal: String,
    mcp: &McpClient,
    variant_context: Option<&str>,
    cache: Option<AgentCache>,
) -> anyhow::Result<(AgentState, AgentCache)> {
    let tools = mcp.tools_as_openai();
    let workflow = clickweave_core::Workflow::default();
    let mut runner = match cache {
        Some(c) => AgentRunner::with_cache(llm, config, c),
        None => AgentRunner::new(llm, config),
    };
    let state = runner
        .run(goal, workflow, mcp, variant_context, tools)
        .await?;
    Ok((state, runner.into_cache()))
}

#[cfg(test)]
mod tests;
