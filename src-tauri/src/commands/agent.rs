use super::error::CommandError;
use super::types::*;
use clickweave_core::variant_index::{VariantEntry, VariantIndex};
use clickweave_engine::agent::{AgentCache, AgentCommand, AgentConfig, AgentStep, StepOutcome};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;

// ── Request / payload types ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct AgentRunRequest {
    pub goal: String,
    pub agent: EndpointConfig,
    pub project_path: Option<String>,
    pub workflow_name: String,
    pub workflow_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStepPayload {
    pub summary: String,
    pub tool_name: String,
    pub step_number: usize,
}

// ── Handle ──────────────────────────────────────────────────────

#[derive(Default)]
pub struct AgentHandle {
    cancel_token: Option<CancellationToken>,
    steering_tx: Option<tokio::sync::mpsc::Sender<String>>,
    task_handle: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl AgentHandle {
    /// Cancel the running agent task and abort the tokio task.
    /// Returns `true` if a task was actually running.
    pub fn force_stop(&mut self) -> bool {
        let had_task = self.task_handle.is_some();
        if let Some(token) = self.cancel_token.take() {
            token.cancel();
        }
        if let Some(task) = self.task_handle.take() {
            task.abort();
        }
        self.steering_tx = None;
        had_task
    }
}

// ── Helpers ─────────────────────────────────────────────────────

fn step_summary(step: &AgentStep) -> String {
    match &step.outcome {
        StepOutcome::Success(text) => {
            if text.len() > 120 {
                let end = text.floor_char_boundary(120);
                format!("{}...", &text[..end])
            } else {
                text.clone()
            }
        }
        StepOutcome::Error(err) => format!("Error: {}", err),
        StepOutcome::Done(summary) => format!("Done: {}", summary),
        StepOutcome::Replan(reason) => format!("Replan: {}", reason),
    }
}

fn step_tool_name(step: &AgentStep) -> String {
    match &step.command {
        AgentCommand::ToolCall { tool_name, .. } => tool_name.clone(),
        AgentCommand::Done { .. } => "agent_done".to_string(),
        AgentCommand::Replan { .. } => "agent_replan".to_string(),
        AgentCommand::TextOnly { .. } => "text".to_string(),
    }
}

// ── Commands ────────────────────────────────────────────────────

#[tauri::command]
#[specta::specta]
pub async fn run_agent(
    app: tauri::AppHandle,
    request: AgentRunRequest,
) -> Result<(), CommandError> {
    {
        let handle = app.state::<Mutex<AgentHandle>>();
        if handle.lock().unwrap().task_handle.is_some() {
            return Err(CommandError::already_running());
        }
    }

    let agent_config = request.agent.into_llm_config(None);
    let mcp_binary_path =
        crate::mcp_resolve::resolve_mcp_binary().map_err(|e| CommandError::mcp(format!("{e}")))?;

    let workflow_id: uuid::Uuid = request
        .workflow_id
        .parse()
        .map_err(|_| CommandError::validation("Invalid workflow ID"))?;

    let mut storage = resolve_storage(
        &app,
        &request.project_path,
        &request.workflow_name,
        workflow_id,
    );

    let cancel_token = CancellationToken::new();
    let agent_token = cancel_token.clone();

    let (_steering_tx, _steering_rx) = tokio::sync::mpsc::channel::<String>(8);

    let emit_handle = app.clone();
    let cleanup_handle = app.clone();
    let goal = request.goal.clone();

    // Channel used to signal the cleanup task when the agent task finishes.
    let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();

    let task_handle = tauri::async_runtime::spawn(async move {
        // Spawn MCP server
        let mcp = match clickweave_mcp::McpClient::spawn(&mcp_binary_path, &[]).await {
            Ok(m) => m,
            Err(e) => {
                let _ = emit_handle.emit(
                    "agent://error",
                    serde_json::json!({ "message": format!("MCP spawn failed: {e}") }),
                );
                let _ = done_tx.send(());
                return;
            }
        };

        // Create LLM client
        let llm = clickweave_llm::LlmClient::new(agent_config);
        let config = AgentConfig::default();

        // Begin storage execution
        let _exec_dir = storage.begin_execution();

        // Load cross-run variant index and decision cache
        let variant_index = VariantIndex::load(&storage.variant_index_path());
        let variant_context = variant_index.as_context_text();
        let cache = AgentCache::load_from_path(&storage.agent_cache_path());

        // Run the agent loop
        let result = tokio::select! {
            res = clickweave_engine::agent::run_agent_workflow(
                &llm,
                config,
                goal,
                &mcp,
                if variant_context.is_empty() { None } else { Some(&variant_context) },
                Some(cache),
            ) => res,
            _ = agent_token.cancelled() => {
                let _ = emit_handle.emit(
                    "agent://error",
                    serde_json::json!({ "message": "Agent cancelled" }),
                );
                let _ = done_tx.send(());
                return;
            }
        };

        match result {
            Ok((state, updated_cache)) => {
                // Persist the updated cache
                let _ = updated_cache.save_to_path(&storage.agent_cache_path());

                // Record this run as a variant entry
                let variant_entry = VariantEntry {
                    execution_dir: storage
                        .execution_dir_name()
                        .unwrap_or("unknown")
                        .to_string(),
                    diverged_at_step: None,
                    divergence_summary: if state.completed {
                        "Followed reference trajectory".to_string()
                    } else {
                        format!(
                            "Stopped after {} steps without completing",
                            state.steps.len()
                        )
                    },
                    success: state.completed,
                };
                let _ = VariantIndex::append(&storage.variant_index_path(), &variant_entry);

                // Emit step events for each completed step
                for step in &state.steps {
                    let _ = emit_handle.emit(
                        "agent://step",
                        AgentStepPayload {
                            summary: step_summary(step),
                            tool_name: step_tool_name(step),
                            step_number: step.index,
                        },
                    );
                }
                let _ = emit_handle.emit("agent://complete", ());
            }
            Err(e) => {
                let _ = emit_handle.emit(
                    "agent://error",
                    serde_json::json!({ "message": format!("{e}") }),
                );
            }
        }

        let _ = done_tx.send(());
    });

    {
        let handle = app.state::<Mutex<AgentHandle>>();
        let mut guard = handle.lock().unwrap();
        guard.cancel_token = Some(cancel_token);
        guard.steering_tx = Some(_steering_tx);
        guard.task_handle = Some(task_handle);
    }

    // Spawn cleanup task: wait for the agent task to signal completion, then clear the handle.
    tauri::async_runtime::spawn(async move {
        let _ = done_rx.await;

        let handle = cleanup_handle.state::<Mutex<AgentHandle>>();
        let mut guard = handle.lock().unwrap();
        guard.cancel_token = None;
        guard.steering_tx = None;
        guard.task_handle = None;
    });

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_agent(app: tauri::AppHandle) -> Result<(), CommandError> {
    let handle = app.state::<Mutex<AgentHandle>>();
    let mut guard = handle.lock().unwrap();
    if !guard.force_stop() {
        return Err(CommandError::validation("No agent is running"));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn steer_agent(app: tauri::AppHandle, message: String) -> Result<(), CommandError> {
    let handle = app.state::<Mutex<AgentHandle>>();
    let guard = handle.lock().unwrap();
    let tx = guard
        .steering_tx
        .as_ref()
        .ok_or(CommandError::validation("No agent is running"))?
        .clone();
    drop(guard);

    tx.try_send(message)
        .map_err(|e| CommandError::internal(format!("Failed to send steering message: {e}")))
}
