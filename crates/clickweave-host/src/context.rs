use std::path::{Path, PathBuf};

use clickweave_engine::agent::episodic::EpisodicContext;
use clickweave_engine::agent::skills::SkillContext;

/// Build an [`EpisodicContext`] for a run.
///
/// Returns `EpisodicContext::disabled()` when either `persist_traces` or
/// `enabled` is false.
///
/// - `workflow_local_path` — path to the workflow-local `episodic.sqlite`
///   (usually `<workflow_dir>/episodic.sqlite`, i.e. `storage.base_path()`
///   `.join("episodic.sqlite")`).
/// - `global_path` — path to the cross-workflow global `episodic.sqlite` when
///   global participation is on; `None` otherwise.
/// - `project_id` — project UUID string carried in `ProjectManifest`.
pub fn build_episodic_context(
    workflow_local_path: PathBuf,
    global_path: Option<PathBuf>,
    project_id: String,
    persist_traces: bool,
    enabled: bool,
) -> EpisodicContext {
    if !persist_traces || !enabled {
        return EpisodicContext::disabled();
    }
    EpisodicContext {
        enabled: true,
        workflow_local_path,
        global_path,
        project_id,
    }
}

/// Build a [`SkillContext`] for a run.
///
/// Returns a context with `enabled = false` (but the directories still
/// populated for diagnostics) when either `persist_traces` or `enabled` is
/// false. This mirrors the Tauri command behaviour.
///
/// - `project_skills_dir` — project-local skills directory (usually
///   `storage.project_skills_dir()`).
/// - `global_skills_dir` — global skills directory when global participation
///   is on; `None` otherwise.
/// - `project_id` — project UUID string.
pub fn build_skill_context(
    project_skills_dir: PathBuf,
    global_skills_dir: Option<PathBuf>,
    project_id: String,
    persist_traces: bool,
    enabled: bool,
) -> SkillContext {
    SkillContext {
        enabled: persist_traces && enabled,
        project_skills_dir,
        global_skills_dir,
        project_id,
    }
}

/// Convenience: resolve the global episodic SQLite path under `app_data_dir`.
pub fn global_episodic_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("episodic.sqlite")
}

/// Convenience: resolve the global skills directory under `app_data_dir`.
/// Creates the directory on demand.
pub fn global_skills_dir(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    let dir = app_data_dir.join("skills_global");
    std::fs::create_dir_all(&dir).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create global skills dir at {}: {e}",
            dir.display()
        )
    })?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── EpisodicContext ──────────────────────────────────────────

    #[test]
    fn episodic_disabled_when_persist_false() {
        let ctx = build_episodic_context(
            PathBuf::from("/some/path/episodic.sqlite"),
            None,
            "proj-id".to_string(),
            false, // persist_traces = false
            true,
        );
        assert!(!ctx.enabled);
    }

    #[test]
    fn episodic_disabled_when_enabled_false() {
        let ctx = build_episodic_context(
            PathBuf::from("/some/path/episodic.sqlite"),
            None,
            "proj-id".to_string(),
            true,
            false, // enabled = false
        );
        assert!(!ctx.enabled);
    }

    #[test]
    fn episodic_enabled_with_global_path() {
        let local = PathBuf::from("/proj/.clickweave/runs/wf/episodic.sqlite");
        let global = PathBuf::from("/app/episodic.sqlite");
        let ctx = build_episodic_context(
            local.clone(),
            Some(global.clone()),
            "proj-id".to_string(),
            true,
            true,
        );
        assert!(ctx.enabled);
        assert_eq!(ctx.workflow_local_path, local);
        assert_eq!(ctx.global_path, Some(global));
        assert_eq!(ctx.project_id, "proj-id");
    }

    #[test]
    fn episodic_enabled_without_global_path() {
        let ctx = build_episodic_context(
            PathBuf::from("/wf/episodic.sqlite"),
            None,
            "proj-id".to_string(),
            true,
            true,
        );
        assert!(ctx.enabled);
        assert!(ctx.global_path.is_none());
    }

    // ── SkillContext ─────────────────────────────────────────────

    #[test]
    fn skill_disabled_when_persist_false() {
        let ctx = build_skill_context(
            PathBuf::from("/proj/skills"),
            None,
            "proj-id".to_string(),
            false, // persist_traces = false
            true,
        );
        assert!(!ctx.enabled);
    }

    #[test]
    fn skill_disabled_when_enabled_false() {
        let ctx = build_skill_context(
            PathBuf::from("/proj/skills"),
            None,
            "proj-id".to_string(),
            true,
            false, // enabled = false
        );
        assert!(!ctx.enabled);
    }

    #[test]
    fn skill_enabled_with_dirs() {
        let proj = PathBuf::from("/proj/.clickweave/skills");
        let global = PathBuf::from("/app/skills_global");
        let ctx = build_skill_context(
            proj.clone(),
            Some(global.clone()),
            "proj-id".to_string(),
            true,
            true,
        );
        assert!(ctx.enabled);
        assert_eq!(ctx.project_skills_dir, proj);
        assert_eq!(ctx.global_skills_dir, Some(global));
        assert_eq!(ctx.project_id, "proj-id");
    }
}
