use clickweave_core::{NodeType, ValidationError, Workflow, validate_workflow};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;

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
        // Return a new default workflow if none exists
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

    // Ensure project directory exists
    std::fs::create_dir_all(&project_path)
        .map_err(|e| format!("Failed to create project directory: {}", e))?;

    // Ensure assets directory exists
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
