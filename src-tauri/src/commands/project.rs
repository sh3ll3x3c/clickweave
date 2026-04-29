use super::error::CommandError;
use super::types::*;
use clickweave_core::permissions::CONFIRMABLE_TOOLS;
use clickweave_core::{NodeType, Workflow, validate_workflow};
use clickweave_engine::agent::skills::move_skills_to_project;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

#[tauri::command]
#[specta::specta]
pub fn ping() -> String {
    "pong".to_string()
}

/// Returns Ok(path) if the MCP sidecar was found at startup, or Err(reason) if not.
#[tauri::command]
#[specta::specta]
pub fn get_mcp_status(app: tauri::AppHandle) -> Result<String, String> {
    let status = app.state::<McpStatus>();
    status.0.clone()
}

#[tauri::command]
#[specta::specta]
pub async fn pick_workflow_file(app: tauri::AppHandle) -> Result<Option<String>, CommandError> {
    let file = app
        .dialog()
        .file()
        .add_filter("Clickweave Workflow", &["json"])
        .blocking_pick_file();
    Ok(file.map(|p| p.to_string()))
}

#[tauri::command]
#[specta::specta]
pub async fn pick_save_file(app: tauri::AppHandle) -> Result<Option<String>, CommandError> {
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
pub fn open_project(path: String) -> Result<ProjectData, CommandError> {
    let file_path = PathBuf::from(&path);

    if !file_path.exists() {
        return Err(CommandError::io(format!("File not found: {}", path)));
    }

    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| CommandError::io(format!("Failed to read file: {}", e)))?;

    let mut workflow: Workflow = serde_json::from_str(&content)
        .map_err(|e| CommandError::validation(format!("Failed to parse workflow: {}", e)))?;

    workflow.fixup_auto_ids();

    Ok(ProjectData { path, workflow })
}

#[tauri::command]
#[specta::specta]
pub fn save_project(
    app: tauri::AppHandle,
    path: String,
    workflow: Workflow,
) -> Result<(), CommandError> {
    let app_data = app.state::<AppDataDir>().0.clone();
    save_project_with_app_data(&app_data, path, workflow)
}

fn save_project_with_app_data(
    app_data_dir: &Path,
    path: String,
    workflow: Workflow,
) -> Result<(), CommandError> {
    let file_path = PathBuf::from(&path);
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CommandError::io(format!("Failed to create directory: {}", e)))?;
    }

    let content = serde_json::to_string_pretty(&workflow)
        .map_err(|e| CommandError::internal(format!("Failed to serialize workflow: {}", e)))?;

    std::fs::write(&file_path, content)
        .map_err(|e| CommandError::io(format!("Failed to write file: {}", e)))?;

    let unsaved_skills_root = app_data_dir.join("skills");
    let saved_project_dir = project_dir(&path);
    move_skills_to_project(
        &unsaved_skills_root,
        &workflow.id.to_string(),
        &saved_project_dir,
    )
    .map_err(|e| CommandError::io(format!("Failed to move skills to project: {e}")))?;

    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn validate(workflow: Workflow) -> ValidationResult {
    match validate_workflow(&workflow) {
        Ok(_) => ValidationResult {
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
            output_role: format!("{:?}", nt.output_role()),
            node_context: format!("{:?}", nt.node_context()),
            icon: nt.icon(),
            node_type: nt,
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn confirmable_tools() -> Vec<ConfirmableTool> {
    CONFIRMABLE_TOOLS
        .iter()
        .map(|(name, description)| ConfirmableTool { name, description })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn generate_auto_id(
    node_type_name: String,
    counters_json: String,
) -> Result<(String, String), String> {
    let mut counters: std::collections::HashMap<String, u32> =
        serde_json::from_str(&counters_json).map_err(|e| e.to_string())?;

    let node_type = NodeType::default_for_name(&node_type_name)
        .ok_or_else(|| format!("Unknown node type: {}", node_type_name))?;

    let auto_id = clickweave_core::auto_id::assign_auto_id(&node_type, &mut counters);

    let updated_counters = serde_json::to_string(&counters).map_err(|e| e.to_string())?;

    Ok((auto_id, updated_counters))
}

#[tauri::command]
#[specta::specta]
pub async fn import_asset(
    app: tauri::AppHandle,
    project_path: String,
) -> Result<Option<ImportedAsset>, CommandError> {
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
        .map_err(|e| CommandError::io(format!("Failed to create assets directory: {}", e)))?;

    let dest = assets_dir.join(&filename);
    std::fs::copy(&source, &dest)
        .map_err(|e| CommandError::io(format!("Failed to copy asset: {}", e)))?;

    let relative_path = format!("assets/{}", filename);
    let absolute_path = dest
        .to_str()
        .ok_or(CommandError::internal("Invalid path"))?
        .to_string();

    Ok(Some(ImportedAsset {
        relative_path,
        absolute_path,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_project_moves_unsaved_skills_to_saved_project_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path().join("app-data");
        let mut workflow = Workflow::default();
        workflow.id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        workflow.name = "Saved Workflow".to_string();

        let unsaved_dir = app_data.join("skills").join(workflow.id.to_string());
        std::fs::create_dir_all(&unsaved_dir).unwrap();
        std::fs::write(unsaved_dir.join("alpha-v1.md"), b"alpha").unwrap();
        std::fs::write(unsaved_dir.join("alpha-v1.proposal.json"), b"{}").unwrap();

        let workflow_path = tmp.path().join("saved").join("workflow.json");
        save_project_with_app_data(
            &app_data,
            workflow_path.to_string_lossy().into_owned(),
            workflow,
        )
        .unwrap();

        assert!(workflow_path.exists());
        assert!(!unsaved_dir.exists());
        assert_eq!(
            std::fs::read(tmp.path().join("saved/.clickweave/skills/alpha-v1.md")).unwrap(),
            b"alpha"
        );
        assert_eq!(
            std::fs::read(
                tmp.path()
                    .join("saved/.clickweave/skills/alpha-v1.proposal.json")
            )
            .unwrap(),
            b"{}"
        );
    }
}
