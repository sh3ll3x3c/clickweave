use std::future::Future;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clickweave_host::{
    AgentConfig, AgentState, LlmClient, PermissionPolicy, RunStorage, RunnerOutput, Uuid,
    lifecycle::AgentRunHandle,
    mcp::{EnvOverride, spawn_mcp},
    storage::{app_data_dir, resolve_storage},
};

use crate::args::RunArgs;
use crate::config::{build_contexts, resolve_llm_config, resolve_project_location};
use crate::renderer::{exit_code_for, print_terminal_summary, render_human, render_json};
use crate::responder::StdinResponder;

/// Outcome of draining a run to completion, possibly via a user stop request.
///
/// Carries the `await_result` outcome plus whether the run was cancelled by a
/// stop signal. The cancel flag lets the finalize step treat a post-cancel
/// transport `Err` (e.g. the MCP child dying from the terminal's process-group
/// SIGINT) as a clean stop rather than a CLI error.
pub struct DrainOutcome {
    /// The collected final agent state, or the error from awaiting the runner
    /// task. The episodic writer handle from `await_result` is not needed for
    /// exit-code mapping, so it is dropped here.
    pub result: Result<AgentState>,
    /// `true` if a stop was requested (Ctrl-C) and `cancel()` was called.
    pub cancelled: bool,
}

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

    let project_id_str = Some(loc.id().to_string());
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
    let handle = clickweave_host::lifecycle::spawn_agent_run(
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

    // ── Drain loop, racing the run against a Ctrl-C stop signal ────────────
    let json_mode = args.json;
    let include_screenshots = args.include_screenshots;
    let outcome = drain_until_stop(
        handle,
        ctrl_c_stop(),
        json_mode,
        include_screenshots,
        &storage_for_drain,
    )
    .await;

    // ── Print execution dir ────────────────────────────────────────────────
    let base_path = {
        let guard = storage_arc.lock().unwrap();
        guard.base_path().join(&exec_dir)
    };
    eprintln!("Execution dir: {}", base_path.display());

    // ── Determine exit code ───────────────────────────────────────────────
    Ok(finalize_exit_code(outcome, json_mode))
}

/// Drain an in-flight run, racing event delivery against a stop trigger.
///
/// Selects between the next live event and `stop`. While `stop` has not fired,
/// each event is persisted and rendered exactly as before. When `stop`
/// resolves (a user-requested cancel), this calls `handle.cancel()` once,
/// prints `Stopping…` to stderr (human mode only), and keeps draining until
/// the channel closes so the run unwinds and the trace finalizes — then awaits
/// the runner task. A re-fired stop after the first does nothing (the `cancel`
/// flag guards against a second `cancel()`).
///
/// `stop` is injected as a `Future` so production can back it with
/// `tokio::signal::ctrl_c()` while tests drive it with a controllable trigger;
/// the unit performs no signal I/O itself.
pub async fn drain_until_stop(
    mut handle: AgentRunHandle,
    stop: impl Future<Output = ()>,
    json_mode: bool,
    include_screenshots: bool,
    storage_for_drain: &Arc<Mutex<RunStorage>>,
) -> DrainOutcome {
    let mut cancelled = false;
    // Pin the stop future so it can be polled repeatedly across `select!`
    // iterations without being moved.
    tokio::pin!(stop);

    loop {
        tokio::select! {
            maybe_output = handle.events.recv() => {
                let Some(output) = maybe_output else {
                    // Channel closed: the run has fully unwound.
                    break;
                };
                drain_one(&output, json_mode, include_screenshots, storage_for_drain);
            }
            _ = &mut stop, if !cancelled => {
                // First stop request: trip the cancel token once and keep
                // draining so the run unwinds cleanly and the trace finalizes.
                handle.cancel();
                cancelled = true;
                if !json_mode {
                    eprintln!("Stopping… (press Ctrl-C again to force-quit)");
                }
            }
        }
    }

    DrainOutcome {
        result: handle.await_result().await.map(|(state, _writer_tx)| state),
        cancelled,
    }
}

/// Persist and render a single drained `RunnerOutput`.
fn drain_one(
    output: &RunnerOutput,
    json_mode: bool,
    include_screenshots: bool,
    storage_for_drain: &Arc<Mutex<RunStorage>>,
) {
    // Persist each AgentEvent to the execution trace.
    if let RunnerOutput::Event(event) = output
        && let Ok(guard) = storage_for_drain.lock()
    {
        let _ = guard.append_agent_event(event);
    }

    if json_mode {
        if let RunnerOutput::Event(event) = output {
            render_json(event, include_screenshots);
        }
    } else {
        render_human(output);
    }
}

/// Map a [`DrainOutcome`] to a CLI exit code, printing the human-mode summary.
///
/// - A real `TerminalReason` keeps today's mapping (`exit_code_for`).
/// - A `None` terminal reason is an external stop → `Run stopped.` / exit 0.
/// - A post-cancel `Err` (e.g. the MCP child died from the terminal's
///   process-group SIGINT) is treated as a clean stop when the user requested
///   one; for a non-cancelled run the error propagates via a panic-free exit 1.
fn finalize_exit_code(outcome: DrainOutcome, json_mode: bool) -> i32 {
    let DrainOutcome { result, cancelled } = outcome;
    match result {
        Ok(state) => {
            if let Some(ref reason) = state.terminal_reason {
                if !json_mode {
                    print_terminal_summary(reason);
                }
                exit_code_for(reason)
            } else {
                // No terminal reason means the run was stopped externally (a
                // cancel), not a loop outcome — mirroring the Tauri
                // `agent://stopped` path. A user-requested stop is a clean
                // stop, so exit 0 (never route it to the approval-unavailable
                // code 6).
                if !json_mode {
                    eprintln!("Run stopped.");
                }
                0
            }
        }
        Err(_err) if cancelled => {
            // The user asked to stop; a broken transport while the run was
            // unwinding (the MCP child often dies first from the terminal's
            // SIGINT) is still a clean stop, not a CLI failure.
            if !json_mode {
                eprintln!("Run stopped.");
            }
            0
        }
        Err(err) => {
            // Non-cancelled run: surface the real error as today.
            eprintln!("Error: {err:#}");
            1
        }
    }
}

/// Production stop trigger: resolves when the process receives SIGINT
/// (Ctrl-C). The first press resolves this future; a second press inside the
/// same run force-quits with exit code 130.
///
/// `ctrl_c()`'s `Result` is intentionally ignored — any resolution (success or
/// the rare handler-install error) is treated as "stop requested", and the
/// force-quit guard re-arms a fresh `ctrl_c()` for the second press.
async fn ctrl_c_stop() {
    let _ = tokio::signal::ctrl_c().await;

    // Re-arm: a second Ctrl-C while the run is unwinding force-quits hard.
    tokio::spawn(async {
        let _ = tokio::signal::ctrl_c().await;
        eprintln!("Force quit.");
        std::process::exit(130);
    });
}

/// Load a `PermissionPolicy` from a JSON file.
fn load_policy(path: &Path) -> Result<PermissionPolicy> {
    let data = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read policy file: {}", path.display()))?;
    serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse policy file: {}", path.display()))
}
