use super::types::*;
use clickweave_core::validate_workflow;
use clickweave_engine::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tracing::warn;

#[derive(Default)]
pub struct ExecutorHandle {
    stop_tx: Option<tokio::sync::mpsc::Sender<ExecutorCommand>>,
}

#[tauri::command]
#[specta::specta]
pub async fn run_workflow(app: tauri::AppHandle, request: RunRequest) -> Result<(), String> {
    {
        let handle = app.state::<Mutex<ExecutorHandle>>();
        if handle.lock().unwrap().stop_tx.is_some() {
            return Err("Workflow is already running".to_string());
        }
    }

    validate_workflow(&request.workflow).map_err(|e| format!("Validation failed: {}", e))?;

    let agent_config = request.agent.into_llm_config(None);
    let vlm_config = request
        .vlm
        .filter(|v| !v.is_empty())
        .map(|v| v.into_llm_config(Some(0.1)));

    let storage = resolve_storage(
        &app,
        &request.project_path,
        &request.workflow.name,
        request.workflow.id,
    );
    let project_path = request.project_path.map(|p| project_dir(&p));

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<ExecutorEvent>(256);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<ExecutorCommand>(8);

    {
        let handle = app.state::<Mutex<ExecutorHandle>>();
        handle.lock().unwrap().stop_tx = Some(cmd_tx);
    }

    let emit_handle = app.clone();
    let cleanup_handle = emit_handle.clone();

    tauri::async_runtime::spawn(async move {
        let mut executor = WorkflowExecutor::new(
            request.workflow,
            agent_config,
            vlm_config,
            request.mcp_command,
            project_path,
            event_tx,
            storage,
        );
        executor.run(cmd_rx).await;
    });

    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let emit_result = match event {
                ExecutorEvent::Log(msg) | ExecutorEvent::Error(msg) => {
                    emit_handle.emit("executor://log", LogPayload { message: msg })
                }
                ExecutorEvent::StateChanged(state) => emit_handle.emit(
                    "executor://state",
                    StatePayload {
                        state: match state {
                            ExecutorState::Idle => "idle".to_owned(),
                            ExecutorState::Running => "running".to_owned(),
                        },
                    },
                ),
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
                ExecutorEvent::WorkflowCompleted => {
                    emit_handle.emit("executor://workflow_completed", ())
                }
                ExecutorEvent::ChecksCompleted(verdicts) => {
                    emit_handle.emit("executor://checks_completed", verdicts)
                }
                ExecutorEvent::RunCreated(_, _) => Ok(()),
            };
            if let Err(e) = emit_result {
                warn!("Failed to emit executor event to UI: {}", e);
            }
        }

        cleanup_handle
            .state::<Mutex<ExecutorHandle>>()
            .lock()
            .unwrap()
            .stop_tx = None;
    });

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_workflow(app: tauri::AppHandle) -> Result<(), String> {
    let guard = app.state::<Mutex<ExecutorHandle>>();
    let guard = guard.lock().unwrap();
    let tx = guard
        .stop_tx
        .as_ref()
        .ok_or_else(|| "No workflow is running".to_string())?;
    tx.try_send(ExecutorCommand::Stop)
        .map_err(|e| format!("Failed to send stop command: {}", e))
}
