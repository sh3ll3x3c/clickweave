use clickweave_core::storage::RunStorage;
use clickweave_core::{
    NodeRun, NodeType, TraceEvent, ValidationError, Workflow, validate_workflow,
};
use clickweave_engine::{ExecutorCommand, ExecutorEvent, WorkflowExecutor};
use clickweave_llm::LlmConfig;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

// ============================================================
// Shared state for executor management
// ============================================================

#[derive(Default)]
pub struct ExecutorHandle {
    stop_tx: Option<tokio::sync::mpsc::Sender<ExecutorCommand>>,
}

// ============================================================
// Request / response types
// ============================================================

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct ProjectData {
    pub path: String,
    pub workflow: Workflow,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct NodeTypeInfo {
    pub name: &'static str,
    pub category: String,
    pub icon: &'static str,
    pub node_type: NodeType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RunRequest {
    pub workflow: Workflow,
    pub project_path: Option<String>,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_api_key: Option<String>,
    pub mcp_command: String,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct RunsQuery {
    pub project_path: String,
    pub workflow_id: String,
    pub node_id: String,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct RunEventsQuery {
    pub project_path: String,
    pub workflow_id: String,
    pub node_id: String,
    pub run_id: String,
}

// ============================================================
// Event payloads (emitted to frontend)
// ============================================================

#[derive(Debug, Clone, Serialize)]
pub struct LogPayload {
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatePayload {
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodePayload {
    pub node_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeErrorPayload {
    pub node_id: String,
    pub error: String,
}

// ============================================================
// Commands
// ============================================================

#[tauri::command]
#[specta::specta]
pub fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
#[specta::specta]
pub async fn pick_project_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let folder = app.dialog().file().blocking_pick_folder();
    Ok(folder.map(|p| p.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn open_project(path: String) -> Result<ProjectData, String> {
    let project_path = PathBuf::from(&path);
    let workflow_path = project_path.join("workflow.json");

    if !workflow_path.exists() {
        return Ok(ProjectData {
            path,
            workflow: Workflow::default(),
        });
    }

    let content = std::fs::read_to_string(&workflow_path)
        .map_err(|e| format!("Failed to read workflow.json: {}", e))?;

    let workflow: Workflow = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse workflow.json: {}", e))?;

    Ok(ProjectData { path, workflow })
}

#[tauri::command]
#[specta::specta]
pub fn save_project(path: String, workflow: Workflow) -> Result<(), String> {
    let project_path = PathBuf::from(&path);

    std::fs::create_dir_all(&project_path)
        .map_err(|e| format!("Failed to create project directory: {}", e))?;

    std::fs::create_dir_all(project_path.join("assets"))
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    let workflow_path = project_path.join("workflow.json");
    let content = serde_json::to_string_pretty(&workflow)
        .map_err(|e| format!("Failed to serialize workflow: {}", e))?;

    std::fs::write(&workflow_path, content)
        .map_err(|e| format!("Failed to write workflow.json: {}", e))?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn validate(workflow: Workflow) -> ValidationResult {
    match validate_workflow(&workflow) {
        Ok(()) => ValidationResult {
            valid: true,
            errors: vec![],
        },
        Err(e) => {
            let error_msg = match e {
                ValidationError::NoNodes => "Workflow has no nodes".to_string(),
                ValidationError::NoEntryPoint => {
                    "No entry point found (all nodes have incoming edges)".to_string()
                }
                ValidationError::MultipleOutgoingEdges(name) => {
                    format!("Node '{}' has multiple outgoing edges", name)
                }
                ValidationError::CycleDetected => "Cycle detected in workflow".to_string(),
            };
            ValidationResult {
                valid: false,
                errors: vec![error_msg],
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn node_type_defaults() -> Vec<NodeTypeInfo> {
    NodeType::all_defaults()
        .into_iter()
        .map(|nt| NodeTypeInfo {
            name: nt.display_name(),
            category: nt.category().display_name().to_string(),
            icon: nt.icon(),
            node_type: nt,
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub async fn run_workflow(app: tauri::AppHandle, request: RunRequest) -> Result<(), String> {
    // Check if already running
    {
        let handle = app.state::<Mutex<ExecutorHandle>>();
        let h = handle.lock().unwrap();
        if h.stop_tx.is_some() {
            return Err("Workflow is already running".to_string());
        }
    }

    // Validate first
    let validation = validate(request.workflow.clone());
    if !validation.valid {
        return Err(format!(
            "Validation failed: {}",
            validation.errors.join(", ")
        ));
    }

    let llm_config = LlmConfig {
        base_url: request.llm_base_url,
        api_key: if request.llm_api_key.as_deref() == Some("") {
            None
        } else {
            request.llm_api_key
        },
        model: request.llm_model,
        temperature: None,
        max_tokens: None,
    };

    let project_path = request.project_path.map(PathBuf::from);

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<ExecutorEvent>(256);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<ExecutorCommand>(8);

    // Store the stop channel
    {
        let handle = app.state::<Mutex<ExecutorHandle>>();
        let mut h = handle.lock().unwrap();
        h.stop_tx = Some(cmd_tx);
    }

    let app_handle = app.clone();

    // Spawn executor task
    tauri::async_runtime::spawn(async move {
        let mut executor = WorkflowExecutor::new(
            request.workflow,
            llm_config,
            request.mcp_command,
            project_path,
            event_tx,
        );
        executor.run(cmd_rx).await;
    });

    // Spawn event forwarding task
    let app_for_cleanup = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match &event {
                ExecutorEvent::Log(msg) => {
                    let _ = app_handle.emit(
                        "executor://log",
                        LogPayload {
                            message: msg.clone(),
                        },
                    );
                }
                ExecutorEvent::StateChanged(state) => {
                    let state_str = match state {
                        clickweave_engine::ExecutorState::Idle => "idle",
                        clickweave_engine::ExecutorState::Running => "running",
                    };
                    let _ = app_handle.emit(
                        "executor://state",
                        StatePayload {
                            state: state_str.to_string(),
                        },
                    );
                }
                ExecutorEvent::NodeStarted(id) => {
                    let _ = app_handle.emit(
                        "executor://node_started",
                        NodePayload {
                            node_id: id.to_string(),
                        },
                    );
                }
                ExecutorEvent::NodeCompleted(id) => {
                    let _ = app_handle.emit(
                        "executor://node_completed",
                        NodePayload {
                            node_id: id.to_string(),
                        },
                    );
                }
                ExecutorEvent::NodeFailed(id, err) => {
                    let _ = app_handle.emit(
                        "executor://node_failed",
                        NodeErrorPayload {
                            node_id: id.to_string(),
                            error: err.clone(),
                        },
                    );
                }
                ExecutorEvent::WorkflowCompleted => {
                    let _ = app_handle.emit("executor://workflow_completed", ());
                }
                ExecutorEvent::Error(msg) => {
                    let _ = app_handle.emit(
                        "executor://log",
                        LogPayload {
                            message: msg.clone(),
                        },
                    );
                }
                ExecutorEvent::RunCreated(_, _) => {
                    // Handled by runs UI later
                }
            }
        }

        // Cleanup when executor finishes
        let handle = app_for_cleanup.state::<Mutex<ExecutorHandle>>();
        let mut h = handle.lock().unwrap();
        h.stop_tx = None;
    });

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn stop_workflow(app: tauri::AppHandle) -> Result<(), String> {
    let handle = app.state::<Mutex<ExecutorHandle>>();
    let h = handle.lock().unwrap();
    if let Some(tx) = &h.stop_tx {
        let _ = tx.try_send(ExecutorCommand::Stop);
        Ok(())
    } else {
        Err("No workflow is running".to_string())
    }
}

#[tauri::command]
#[specta::specta]
pub fn list_runs(query: RunsQuery) -> Result<Vec<NodeRun>, String> {
    let workflow_id: Uuid = query
        .workflow_id
        .parse()
        .map_err(|_| "Invalid workflow ID".to_string())?;
    let node_id: Uuid = query
        .node_id
        .parse()
        .map_err(|_| "Invalid node ID".to_string())?;

    let storage = RunStorage::new(&PathBuf::from(&query.project_path), workflow_id);
    storage
        .load_runs_for_node(node_id)
        .map_err(|e| format!("Failed to load runs: {}", e))
}

#[tauri::command]
#[specta::specta]
pub fn load_run_events(query: RunEventsQuery) -> Result<Vec<TraceEvent>, String> {
    let workflow_id: Uuid = query
        .workflow_id
        .parse()
        .map_err(|_| "Invalid workflow ID".to_string())?;
    let node_id: Uuid = query
        .node_id
        .parse()
        .map_err(|_| "Invalid node ID".to_string())?;
    let run_id: Uuid = query
        .run_id
        .parse()
        .map_err(|_| "Invalid run ID".to_string())?;

    let storage = RunStorage::new(&PathBuf::from(&query.project_path), workflow_id);
    let events_path = storage.run_dir(node_id, run_id).join("events.jsonl");

    if !events_path.exists() {
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(&events_path)
        .map_err(|e| format!("Failed to read events.jsonl: {}", e))?;

    let events: Vec<TraceEvent> = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok(events)
}

#[tauri::command]
#[specta::specta]
pub fn read_artifact_base64(path: String) -> Result<String, String> {
    use base64::Engine;
    let data = std::fs::read(&path).map_err(|e| format!("Failed to read artifact: {}", e))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&data))
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct ImportedAsset {
    pub relative_path: String,
    pub absolute_path: String,
}

#[tauri::command]
#[specta::specta]
pub async fn import_asset(
    app: tauri::AppHandle,
    project_path: String,
) -> Result<Option<ImportedAsset>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"])
        .blocking_pick_file();

    let source = match file {
        Some(f) => PathBuf::from(f.to_string()),
        None => return Ok(None),
    };

    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let filename = format!("{}.{}", Uuid::new_v4(), ext);

    let assets_dir = PathBuf::from(&project_path).join("assets");
    std::fs::create_dir_all(&assets_dir)
        .map_err(|e| format!("Failed to create assets directory: {}", e))?;

    let dest = assets_dir.join(&filename);
    std::fs::copy(&source, &dest).map_err(|e| format!("Failed to copy asset: {}", e))?;

    let relative_path = format!("assets/{}", filename);
    let absolute_path = dest.to_str().ok_or("Invalid path")?.to_string();

    Ok(Some(ImportedAsset {
        relative_path,
        absolute_path,
    }))
}
