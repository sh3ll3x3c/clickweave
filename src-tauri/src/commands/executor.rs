use super::error::CommandError;
use super::types::*;
use clickweave_engine::agent::skills::SkillStore;
use clickweave_engine::{ExecutorCommand, ExecutorEvent, ExecutorState};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uuid::Uuid;

#[derive(Default)]
pub struct ExecutorHandle {
    cancel_token: Option<CancellationToken>,
    cmd_tx: Option<tokio::sync::mpsc::Sender<ExecutorCommand>>,
    task_handle: Option<tauri::async_runtime::JoinHandle<()>>,
    run_generation: u64,
}

impl ExecutorHandle {
    /// Stop the running executor task. Signals cancellation via the token
    /// (graceful), then aborts the tokio task (forceful fallback). The MCP
    /// subprocess is killed as a side effect: aborting the task drops
    /// `McpClient`, whose `Drop` impl calls `kill()`.
    /// Returns `true` if a task was actually running.
    pub fn force_stop(&mut self) -> bool {
        let had_task = self.task_handle.is_some();
        // Signal cancellation first (graceful)
        if let Some(token) = self.cancel_token.take() {
            token.cancel();
        }
        // Then abort the task (forceful fallback)
        if let Some(task) = self.task_handle.take() {
            task.abort();
        }
        self.cmd_tx = None;
        had_task
    }
}

/// IPC payload for `run_skill` (D33). Replaces the legacy `RunRequest`
/// which carried a full `Workflow` graph. Every field on the legacy
/// request that fed downstream privacy / supervision gates is preserved
/// here so Phase 1.L acceptance still passes.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RunSkillRequest {
    /// Saved-project workspace path. `None` for unsaved projects — in that
    /// case `RunStorage::new_app_data(app_data, &project_name, project_id)`
    /// resolves the storage from the manifest identity below.
    pub project_path: Option<String>,
    /// Project identity carried forward from `ProjectManifest` (D33).
    /// Required for unsaved-project skill resolution and for run-trace
    /// storage paths.
    pub project_id: Uuid,
    pub project_name: String,
    pub skill_id: String,
    #[serde(default)]
    pub variables: HashMap<String, serde_json::Value>,
    pub agent: EndpointConfig,
    pub fast: Option<EndpointConfig>,
    /// Optional supervisor model for Test mode.
    pub supervisor: Option<EndpointConfig>,
    pub execution_mode: clickweave_core::ExecutionMode,
    #[serde(default = "default_supervision_delay_ms")]
    pub supervision_delay_ms: u64,
    /// Privacy kill switch — `Some(false)` disables run/skill artifact
    /// persistence (D31). `None` falls back to settings.
    pub store_traces: Option<bool>,
}

fn default_supervision_delay_ms() -> u64 {
    500
}

/// Phase 1.C stub: resolves the requested skill from the project's
/// skill store and returns Ok without dispatching the executor. Full
/// executor wiring lands in Phase 1.D, when the native `Skill`-driven
/// runner replaces the deleted `WorkflowExecutor`.
//
// 1.D WIRE-UP: replace the load-and-return body below with the real
// dispatch into the new `skill_runner` once it lands in Phase 1.D.
#[tauri::command]
#[specta::specta]
pub async fn run_skill(
    app: tauri::AppHandle,
    request: RunSkillRequest,
) -> Result<(), CommandError> {
    {
        let handle = app.state::<Mutex<ExecutorHandle>>();
        if handle.lock().unwrap().cmd_tx.is_some() {
            return Err(CommandError::already_running());
        }
    }

    let storage = resolve_storage(
        &app,
        &request.project_path,
        &request.project_name,
        request.project_id,
    );

    let skills_dir = storage
        .project_skills_dir()
        .map_err(|e| CommandError::io(format!("resolve project_skills_dir: {e}")))?;
    let store = SkillStore::new(skills_dir);

    // Load all skills in the directory and confirm the requested
    // `skill_id` exists. The full executor wiring lands in 1.D — for
    // now we just verify that the IPC plumbing surfaces the right
    // error when the caller references a missing skill.
    let mut found = false;
    for path in store
        .list_files()
        .map_err(|e| CommandError::io(format!("list skills: {e}")))?
    {
        if let Ok(skill) = store.read_skill(&path)
            && skill.id == request.skill_id
        {
            found = true;
            break;
        }
    }

    if !found {
        return Err(CommandError::validation(format!(
            "Skill not found: {}",
            request.skill_id
        )));
    }

    // Privacy kill switch carried forward from the legacy RunRequest —
    // surfaced here so the field is not silently dropped while the
    // executor wiring is staged in 1.D.
    let _persist_traces = request.store_traces.unwrap_or(true);

    Ok(())
}

fn spawn_executor_event_forwarder(
    emit_handle: tauri::AppHandle,
    mut event_rx: tokio::sync::mpsc::Receiver<ExecutorEvent>,
    run_generation: u64,
) {
    let cleanup_handle = emit_handle.clone();
    tauri::async_runtime::spawn(async move {
        let mut saw_idle = false;
        while let Some(event) = event_rx.recv().await {
            if matches!(event, ExecutorEvent::StateChanged(ExecutorState::Idle)) {
                saw_idle = true;
            }
            if let Err(e) = emit_executor_event(&emit_handle, event) {
                warn!("Failed to emit executor event to UI: {}", e);
            }
        }

        // On forceful abort the executor task is killed before it can emit
        // StateChanged(Idle), so the UI would stay stuck on "Running".
        // Only emit the fallback idle if the executor didn't send one itself.
        if !saw_idle {
            let _ = emit_handle.emit(
                "executor://state",
                StatePayload {
                    state: "idle".to_owned(),
                },
            );
        }

        let state = cleanup_handle.state::<Mutex<ExecutorHandle>>();
        let mut guard = state.lock().unwrap();
        clear_executor_handle_if_current(&mut guard, run_generation);
    });
}

fn clear_executor_handle_if_current(guard: &mut ExecutorHandle, run_generation: u64) {
    if guard.run_generation != run_generation {
        return;
    }

    guard.cancel_token = None;
    guard.cmd_tx = None;
    guard.task_handle = None;
}

fn emit_executor_event(emit_handle: &tauri::AppHandle, event: ExecutorEvent) -> tauri::Result<()> {
    match event {
        ExecutorEvent::Log(msg) | ExecutorEvent::Error(msg) => {
            emit_handle.emit("executor://log", LogPayload { message: msg })
        }
        ExecutorEvent::StateChanged(state) => {
            emit_handle.emit("executor://state", StatePayload::from_state(state))
        }
        ExecutorEvent::NodeStarted(id) => emit_handle.emit(
            "executor://node_started",
            NodePayload {
                node_id: id.to_string(),
            },
        ),
        ExecutorEvent::NodeCompleted(id) => emit_handle.emit(
            "executor://node_completed",
            NodePayload {
                node_id: id.to_string(),
            },
        ),
        ExecutorEvent::NodeFailed(id, err) => emit_handle.emit(
            "executor://node_failed",
            NodeErrorPayload {
                node_id: id.to_string(),
                error: err,
            },
        ),
        ExecutorEvent::WorkflowCompleted => emit_handle.emit("executor://workflow_completed", ()),
        ExecutorEvent::ChecksCompleted(verdicts) => {
            emit_handle.emit("executor://checks_completed", verdicts)
        }
        ExecutorEvent::RunCreated(_, _) => Ok(()),
        ExecutorEvent::SupervisionPassed {
            node_id,
            node_name,
            summary,
        } => emit_handle.emit(
            "executor://supervision_passed",
            SupervisionPassedPayload {
                node_id: node_id.to_string(),
                node_name,
                summary,
            },
        ),
        ExecutorEvent::SupervisionPaused {
            node_id,
            node_name,
            finding,
            screenshot,
        } => emit_handle.emit(
            "executor://supervision_paused",
            SupervisionPausedPayload {
                node_id: node_id.to_string(),
                node_name,
                finding,
                screenshot,
            },
        ),
        ExecutorEvent::AmbiguityResolved {
            node_id,
            target,
            candidates,
            chosen_uid,
            reasoning,
            viewport_width,
            viewport_height,
            screenshot_path,
            screenshot_base64,
        } => emit_handle.emit(
            "executor://ambiguity_resolved",
            AmbiguityResolvedPayload {
                node_id: node_id.to_string(),
                target,
                candidates: candidates
                    .into_iter()
                    .map(CandidateViewPayload::from)
                    .collect(),
                chosen_uid,
                reasoning,
                viewport_width,
                viewport_height,
                screenshot_path,
                screenshot_base64,
            },
        ),
        ExecutorEvent::NodeCancelled(id) => emit_handle.emit(
            "executor://node_cancelled",
            NodePayload {
                node_id: id.to_string(),
            },
        ),
    }
}

impl StatePayload {
    fn from_state(state: ExecutorState) -> Self {
        Self {
            state: match state {
                ExecutorState::Idle => "idle".to_owned(),
                ExecutorState::Running => "running".to_owned(),
            },
        }
    }
}

impl From<clickweave_engine::CandidateView> for CandidateViewPayload {
    fn from(candidate: clickweave_engine::CandidateView) -> Self {
        Self {
            uid: candidate.uid,
            snippet: candidate.snippet,
            rect: candidate.rect.map(|r| CandidateRectPayload {
                x: r.x,
                y: r.y,
                width: r.width,
                height: r.height,
            }),
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn stop_workflow(app: tauri::AppHandle) -> Result<(), CommandError> {
    let handle = app.state::<Mutex<ExecutorHandle>>();
    let mut guard = handle.lock().unwrap();
    if !guard.force_stop() {
        return Err(CommandError::validation("No workflow is running"));
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn supervision_respond(
    app: tauri::AppHandle,
    action: String,
) -> Result<(), CommandError> {
    let handle = app.state::<Mutex<ExecutorHandle>>();
    let guard = handle.lock().unwrap();
    let tx = guard
        .cmd_tx
        .as_ref()
        .ok_or(CommandError::validation("No workflow is running"))?
        .clone();
    drop(guard);

    let command = match action.as_str() {
        "retry" => ExecutorCommand::Resume,
        "skip" => ExecutorCommand::Skip,
        "abort" => ExecutorCommand::Abort,
        _ => {
            return Err(CommandError::validation(format!(
                "Unknown supervision action: {}",
                action
            )));
        }
    };
    tx.try_send(command)
        .map_err(|e| CommandError::internal(format!("Failed to send command: {}", e)))
}

// Suppresses "function never used" while the executor wiring is staged
// in 1.D. The forwarder is kept here so the wire-up commit only adds
// the `tauri::async_runtime::spawn` call site, not the helper itself.
#[allow(dead_code)]
fn _phase1c_keep_forwarder_alive() {
    let _ = spawn_executor_event_forwarder;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executor_cleanup_clears_current_generation_handles() {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(1);
        let mut handle = ExecutorHandle {
            cancel_token: Some(CancellationToken::new()),
            cmd_tx: Some(cmd_tx),
            task_handle: None,
            run_generation: 7,
        };

        clear_executor_handle_if_current(&mut handle, 7);

        assert!(handle.cancel_token.is_none());
        assert!(handle.cmd_tx.is_none());
        assert!(handle.task_handle.is_none());
    }

    #[test]
    fn executor_cleanup_preserves_newer_generation_handles() {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel(1);
        let mut handle = ExecutorHandle {
            cancel_token: Some(CancellationToken::new()),
            cmd_tx: Some(cmd_tx),
            task_handle: None,
            run_generation: 8,
        };

        clear_executor_handle_if_current(&mut handle, 7);

        assert!(handle.cancel_token.is_some());
        assert!(handle.cmd_tx.is_some());
        assert!(handle.task_handle.is_none());
    }
}
