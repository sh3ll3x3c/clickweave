use clickweave_core::storage::RunStorage;
use clickweave_core::{NodeRun, NodeType, TraceEvent, Workflow, validate_workflow};
use clickweave_engine::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use clickweave_llm::LlmConfig;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

fn parse_uuid(s: &str, label: &str) -> Result<Uuid, String> {
    s.parse().map_err(|_| format!("Invalid {} ID", label))
}

/// Derive the project directory from a path that may be a file or directory.
fn project_dir(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.extension().is_some() {
        p.parent().unwrap_or(&p).to_path_buf()
    } else {
        p
    }
}

#[derive(Default)]
pub struct ExecutorHandle {
    stop_tx: Option<tokio::sync::mpsc::Sender<ExecutorCommand>>,
}

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
pub struct EndpointConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

impl EndpointConfig {
    fn into_llm_config(self, temperature: Option<f32>) -> LlmConfig {
        LlmConfig {
            base_url: self.base_url,
            api_key: self.api_key.filter(|k| !k.is_empty()),
            model: self.model,
            temperature,
            max_tokens: None,
        }
    }

    fn is_empty(&self) -> bool {
        self.base_url.is_empty() || self.model.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RunRequest {
    pub workflow: Workflow,
    pub project_path: Option<String>,
    pub agent: EndpointConfig,
    pub transform: EndpointConfig,
    pub vlm: Option<EndpointConfig>,
    pub mcp_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PlanRequest {
    pub intent: String,
    pub planner: EndpointConfig,
    pub allow_ai_transforms: bool,
    pub allow_agent_steps: bool,
    pub mcp_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PlanResponse {
    pub workflow: Workflow,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PatchRequest {
    pub workflow: Workflow,
    pub user_prompt: String,
    pub planner: EndpointConfig,
    pub allow_ai_transforms: bool,
    pub allow_agent_steps: bool,
    pub mcp_command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct WorkflowPatch {
    pub added_nodes: Vec<clickweave_core::Node>,
    pub removed_node_ids: Vec<String>,
    pub updated_nodes: Vec<clickweave_core::Node>,
    pub added_edges: Vec<clickweave_core::Edge>,
    pub removed_edges: Vec<clickweave_core::Edge>,
    pub warnings: Vec<String>,
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

#[tauri::command]
#[specta::specta]
pub fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
#[specta::specta]
pub async fn pick_workflow_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Clickweave Workflow", &["json"])
        .blocking_pick_file();
    Ok(file.map(|p| p.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn pick_save_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let file = app
        .dialog()
        .file()
        .add_filter("Clickweave Workflow", &["json"])
        .set_file_name("workflow.json")
        .blocking_save_file();
    Ok(file.map(|p| p.to_string()))
}

#[tauri::command]
#[specta::specta]
pub fn open_project(path: String) -> Result<ProjectData, String> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(format!("File not found: {}", path));
    }

    let content =
        std::fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {}", e))?;

    let workflow: Workflow =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse workflow: {}", e))?;

    Ok(ProjectData { path, workflow })
}

#[tauri::command]
#[specta::specta]
pub fn save_project(path: String, workflow: Workflow) -> Result<(), String> {
    let file_path = PathBuf::from(&path);

    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(&workflow)
        .map_err(|e| format!("Failed to serialize workflow: {}", e))?;

    std::fs::write(&file_path, content).map_err(|e| format!("Failed to write file: {}", e))?;

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
        Err(e) => ValidationResult {
            valid: false,
            errors: vec![e.to_string()],
        },
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

    let project_path = request.project_path.map(|p| project_dir(&p));

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<ExecutorEvent>(256);
    let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel::<ExecutorCommand>(8);

    {
        let handle = app.state::<Mutex<ExecutorHandle>>();
        handle.lock().unwrap().stop_tx = Some(cmd_tx);
    }

    let emit_handle = app.clone();
    let cleanup_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let mut executor = WorkflowExecutor::new(
            request.workflow,
            agent_config,
            vlm_config,
            request.mcp_command,
            project_path,
            event_tx,
        );
        executor.run(cmd_rx).await;
    });

    tauri::async_runtime::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                ExecutorEvent::Log(msg) | ExecutorEvent::Error(msg) => {
                    let _ = emit_handle.emit("executor://log", LogPayload { message: msg });
                }
                ExecutorEvent::StateChanged(state) => {
                    let state = match state {
                        ExecutorState::Idle => "idle",
                        ExecutorState::Running => "running",
                    };
                    let _ = emit_handle.emit(
                        "executor://state",
                        StatePayload {
                            state: state.to_string(),
                        },
                    );
                }
                ExecutorEvent::NodeStarted(id) => {
                    let _ = emit_handle.emit(
                        "executor://node_started",
                        NodePayload {
                            node_id: id.to_string(),
                        },
                    );
                }
                ExecutorEvent::NodeCompleted(id) => {
                    let _ = emit_handle.emit(
                        "executor://node_completed",
                        NodePayload {
                            node_id: id.to_string(),
                        },
                    );
                }
                ExecutorEvent::NodeFailed(id, err) => {
                    let _ = emit_handle.emit(
                        "executor://node_failed",
                        NodeErrorPayload {
                            node_id: id.to_string(),
                            error: err,
                        },
                    );
                }
                ExecutorEvent::WorkflowCompleted => {
                    let _ = emit_handle.emit("executor://workflow_completed", ());
                }
                ExecutorEvent::RunCreated(_, _) => {}
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
    guard
        .stop_tx
        .as_ref()
        .ok_or_else(|| "No workflow is running".to_string())
        .map(|tx| {
            let _ = tx.try_send(ExecutorCommand::Stop);
        })
}

#[tauri::command]
#[specta::specta]
pub fn list_runs(query: RunsQuery) -> Result<Vec<NodeRun>, String> {
    let workflow_id = parse_uuid(&query.workflow_id, "workflow")?;
    let node_id = parse_uuid(&query.node_id, "node")?;

    let storage = RunStorage::new(&project_dir(&query.project_path), workflow_id);
    storage
        .load_runs_for_node(node_id)
        .map_err(|e| format!("Failed to load runs: {}", e))
}

#[tauri::command]
#[specta::specta]
pub fn load_run_events(query: RunEventsQuery) -> Result<Vec<TraceEvent>, String> {
    let workflow_id = parse_uuid(&query.workflow_id, "workflow")?;
    let node_id = parse_uuid(&query.node_id, "node")?;
    let run_id = parse_uuid(&query.run_id, "run")?;

    let storage = RunStorage::new(&project_dir(&query.project_path), workflow_id);
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

    let assets_dir = project_dir(&project_path).join("assets");
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
