use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clickweave_host::{
    AgentConfig, LlmClient, PermissionPolicy, RunnerOutput, Uuid,
    mcp::{EnvOverride, spawn_mcp},
    storage::{app_data_dir, resolve_storage},
};

use crate::args::RunArgs;
use crate::config::{build_contexts, resolve_llm_config, resolve_project_location};
use crate::renderer::{exit_code_for, print_terminal_summary, render_human, render_json};
use crate::responder::StdinResponder;

/// Execute the `run` subcommand.
///
/// Returns the exit code.
pub async fn execute(args: RunArgs) -> Result<i32> {
    // ── LLM config ────────────────────────────────────────────────────────
    let llm_config = resolve_llm_config(args.base_url, args.model, args.api_key)?;
    let llm = LlmClient::new(llm_config);

    // ── MCP ───────────────────────────────────────────────────────────────
    let mcp_binary = match args.mcp_binary {
        Some(p) => p,
        None => clickweave_host::mcp::resolve_mcp_binary(EnvOverride::Always)?,
    };
    let mcp = spawn_mcp(&mcp_binary, &[]).await?;

    // ── Project + storage ─────────────────────────────────────────────────
    let app_data = app_data_dir();
    let loc = resolve_project_location(args.project, args.project_name, args.project_id)?;

    let project_id_str = match &loc {
        clickweave_host::storage::ProjectLocation::Saved { id, .. } => Some(id.to_string()),
        clickweave_host::storage::ProjectLocation::Unsaved { id, .. } => Some(id.to_string()),
    };

    let mut storage = resolve_storage(&app_data, loc);
    storage.set_persistent(!args.no_store_traces);
    let exec_dir = storage
        .begin_execution()
        .context("Failed to begin execution")?;

    // ── Contexts ──────────────────────────────────────────────────────────
    let (episodic_ctx, skill_ctx) =
        build_contexts(&storage, &app_data, project_id_str, args.no_store_traces)?;

    // ── Permission policy ─────────────────────────────────────────────────
    let permissions = match args.policy {
        Some(ref path) => Some(load_policy(path)?),
        None => None,
    };

    // ── Responder ─────────────────────────────────────────────────────────
    let auto_approve = args.yes || args.allow_all;
    let responder = Arc::new(StdinResponder::new(auto_approve));

    // ── AgentConfig ───────────────────────────────────────────────────────
    let config = AgentConfig {
        max_steps: args.max_steps.unwrap_or(30),
        ..AgentConfig::default()
    };

    // ── Verification artifacts dir ────────────────────────────────────────
    let verification_artifacts_dir = storage.execution_artifacts_dir();

    // Wrap storage in Arc<Mutex> for the engine and for our drain loop.
    let storage_arc = Arc::new(Mutex::new(storage));
    let storage_for_drain = Arc::clone(&storage_arc);

    // ── Spawn the run ─────────────────────────────────────────────────────
    let mut handle = clickweave_host::lifecycle::spawn_agent_run(
        llm,
        mcp,
        config,
        args.goal,
        None, // vision
        permissions,
        Uuid::new_v4(),
        None, // anchor_node_id
        verification_artifacts_dir,
        Some(Arc::clone(&storage_arc)),
        Some(episodic_ctx),
        Some(skill_ctx),
        None, // system_prompt_override
        responder,
    );

    // ── Drain loop ────────────────────────────────────────────────────────
    // Keep last-seen GoalComplete/terminal events to determine exit code.
    let json_mode = args.json;
    let include_screenshots = args.include_screenshots;
    while let Some(output) = handle.events.recv().await {
        // Persist each AgentEvent to the execution trace.
        if let RunnerOutput::Event(ref event) = output {
            let guard = storage_for_drain.lock().unwrap();
            let _ = guard.append_agent_event(event);
            drop(guard);
        }

        if json_mode {
            if let RunnerOutput::Event(ref event) = output {
                render_json(event, include_screenshots);
            }
        } else {
            render_human(&output);
        }
    }

    // ── Collect final state ───────────────────────────────────────────────
    let (state, _writer_tx) = handle.await_result().await?;

    // ── Print execution dir (plan §13) ────────────────────────────────────
    let base_path = {
        let guard = storage_arc.lock().unwrap();
        guard.base_path().join(&exec_dir)
    };
    eprintln!("Execution dir: {}", base_path.display());

    // ── Determine exit code ───────────────────────────────────────────────
    let exit_code = if let Some(ref reason) = state.terminal_reason {
        if !json_mode {
            print_terminal_summary(reason);
        }
        exit_code_for(reason)
    } else {
        // No terminal reason — treat as success (shouldn't normally happen).
        0
    };

    Ok(exit_code)
}

/// Load a `PermissionPolicy` from a JSON file.
fn load_policy(path: &Path) -> Result<PermissionPolicy> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read policy file: {}", path.display()))?;
    serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse policy file: {}", path.display()))
}
