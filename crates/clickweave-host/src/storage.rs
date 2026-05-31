use std::path::{Path, PathBuf};

use anyhow::Context;
use clickweave_core::ProjectManifest;
use clickweave_core::storage::RunStorage;
use uuid::Uuid;

/// Idiomatic per-platform app data directory.
///
/// - macOS: `~/Library/Application Support/com.clickweave.app/`
/// - Windows: `%APPDATA%\Clickweave\`
/// - Linux: `$XDG_DATA_HOME/clickweave/` or `~/.local/share/clickweave/`
pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var("HOME").expect("HOME should be set"))
            .join("Library/Application Support/com.clickweave.app")
    }
    #[cfg(target_os = "windows")]
    {
        PathBuf::from(std::env::var("APPDATA").expect("APPDATA should be set")).join("Clickweave")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        PathBuf::from(std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME should be set");
            format!("{home}/.local/share")
        }))
        .join("clickweave")
    }
}

/// Normalize a project path: if the path has a file extension (e.g.
/// `foo.json`), return its parent directory. An extension-less path is
/// returned as-is.
///
/// This matches the GUI's behaviour: `open_project` forwards the `.json`
/// file path but `RunStorage::new` must receive the directory.
pub fn project_dir(path: &Path) -> PathBuf {
    if path.extension().is_some() {
        path.parent().unwrap_or(path).to_path_buf()
    } else {
        path.to_path_buf()
    }
}

/// Where to store run data for this project.
pub enum ProjectLocation {
    /// A saved project whose files live on disk.
    Saved {
        /// Path to the project file or directory.
        path: PathBuf,
        name: String,
        id: Uuid,
    },
    /// An unsaved / transient project (app-data fallback).
    Unsaved { name: String, id: Uuid },
}

impl ProjectLocation {
    /// Return the project UUID, present on both variants.
    pub fn id(&self) -> Uuid {
        match self {
            ProjectLocation::Saved { id, .. } => *id,
            ProjectLocation::Unsaved { id, .. } => *id,
        }
    }
}

/// Build a `RunStorage` for the given project location.
///
/// For `Saved` projects the storage root is placed inside the project
/// directory (after file→dir normalization via [`project_dir`]).
/// For `Unsaved` projects the app-data dir is used.
pub fn resolve_storage(app_data: &Path, loc: ProjectLocation) -> RunStorage {
    match loc {
        ProjectLocation::Saved { path, name, .. } => RunStorage::new(&project_dir(&path), &name),
        ProjectLocation::Unsaved { name, id } => RunStorage::new_app_data(app_data, &name, id),
    }
}

/// Load a `ProjectManifest` from the given path.
///
/// The path may point to either the project directory or the `project.json`
/// file inside it — both are handled via [`project_dir`].
pub fn load_project(path: &Path) -> anyhow::Result<ProjectManifest> {
    let dir = project_dir(path);
    let manifest_path = dir.join("project.json");
    let data = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse {}", manifest_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_core::project::PROJECT_SCHEMA_VERSION;

    // ── project_dir normalization ────────────────────────────────

    #[test]
    fn project_dir_strips_extension() {
        let p = Path::new("/some/project/workflow.json");
        assert_eq!(project_dir(p), PathBuf::from("/some/project"));
    }

    #[test]
    fn project_dir_passthrough_without_extension() {
        let p = Path::new("/some/project");
        assert_eq!(project_dir(p), PathBuf::from("/some/project"));
    }

    // Regression: a saved project opened as `/proj/workflow.json` must
    // place `.clickweave` under `/proj/`, not under the JSON file path.
    #[test]
    fn saved_project_storage_rooted_at_dir_not_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let file_path = dir.join("workflow.json");
        // Write a dummy file so the path looks plausible.
        std::fs::write(&file_path, b"{}").unwrap();

        let loc = ProjectLocation::Saved {
            path: file_path.clone(),
            name: "test".to_string(),
            id: Uuid::new_v4(),
        };
        let storage = resolve_storage(dir, loc);
        // base_path should live under `dir/.clickweave/runs/…`, not under the
        // file path itself.
        assert!(
            storage.base_path().starts_with(dir),
            "storage base_path {} must be inside {}",
            storage.base_path().display(),
            dir.display()
        );
        // Must NOT start with the file path.
        assert!(
            !storage.base_path().starts_with(&file_path),
            "storage base_path {} must not be inside the JSON file path",
            storage.base_path().display()
        );
    }

    #[test]
    fn unsaved_project_storage_under_app_data() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path();
        let id = Uuid::new_v4();
        let loc = ProjectLocation::Unsaved {
            name: "my-workflow".to_string(),
            id,
        };
        let storage = resolve_storage(app_data, loc);
        assert!(storage.base_path().starts_with(app_data));
    }

    // ── load_project round-trip ──────────────────────────────────

    fn write_manifest(dir: &Path, manifest: &ProjectManifest) {
        let data = serde_json::to_string_pretty(manifest).unwrap();
        std::fs::write(dir.join("project.json"), data).unwrap();
    }

    #[test]
    fn load_project_from_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = ProjectManifest {
            id: Uuid::new_v4(),
            name: "test-project".to_string(),
            intent: None,
            schema_version: PROJECT_SCHEMA_VERSION,
        };
        write_manifest(tmp.path(), &manifest);

        let loaded = load_project(tmp.path()).unwrap();
        assert_eq!(loaded.id, manifest.id);
        assert_eq!(loaded.name, "test-project");
    }

    #[test]
    fn load_project_from_json_file_path() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = ProjectManifest {
            id: Uuid::new_v4(),
            name: "file-test".to_string(),
            intent: None,
            schema_version: PROJECT_SCHEMA_VERSION,
        };
        write_manifest(tmp.path(), &manifest);
        // Pass the .json file path directly.
        let file_path = tmp.path().join("project.json");

        let loaded = load_project(&file_path).unwrap();
        assert_eq!(loaded.id, manifest.id);
        assert_eq!(loaded.name, "file-test");
    }
}
