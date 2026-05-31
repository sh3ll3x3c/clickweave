use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clickweave_host::{
    EpisodicContext, LlmConfig, RunStorage, SkillContext, Uuid, storage::ProjectLocation,
};

/// Resolve the LLM configuration from CLI flags, then env vars, then defaults.
///
/// Priority: explicit flag > env var > built-in default.
/// `api_key`: normalised via `host::llm_config` (empty string → None).
pub fn resolve_llm_config(
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<LlmConfig> {
    let base_url = base_url
        .or_else(|| std::env::var("CLICKWEAVE_BASE_URL").ok())
        .context("LLM base URL is required (set --base-url or CLICKWEAVE_BASE_URL)")?;
    let model = model
        .or_else(|| std::env::var("CLICKWEAVE_MODEL").ok())
        .context("LLM model is required (set --model or CLICKWEAVE_MODEL)")?;
    let api_key = api_key.or_else(|| std::env::var("CLICKWEAVE_API_KEY").ok());

    Ok(clickweave_host::config::llm_config(
        base_url, model, api_key, None, None,
    ))
}

/// Determine `ProjectLocation` from CLI flags.
///
/// Loads the project manifest when a `--project` path is given.
pub fn resolve_project_location(
    project: Option<PathBuf>,
    project_name: Option<String>,
    project_id: Option<String>,
) -> Result<ProjectLocation> {
    match (project, project_name, project_id) {
        (Some(path), None, None) => {
            let manifest = clickweave_host::storage::load_project(&path)
                .with_context(|| format!("Failed to load project from {}", path.display()))?;
            Ok(ProjectLocation::Saved {
                path,
                name: manifest.name,
                id: manifest.id,
            })
        }
        (None, Some(name), Some(id_str)) => {
            let id = id_str
                .parse::<Uuid>()
                .with_context(|| format!("Invalid project ID: {id_str}"))?;
            Ok(ProjectLocation::Unsaved { name, id })
        }
        (None, None, None) => {
            // Ephemeral project
            Ok(ProjectLocation::Unsaved {
                name: "clickweave-cli".to_string(),
                id: Uuid::new_v4(),
            })
        }
        _ => {
            anyhow::bail!("Specify either --project <path> or both --project-name and --project-id")
        }
    }
}

/// Build episodic and skill contexts for a run.
///
/// Both are enabled by default (matching GUI behaviour) and disabled when
/// `no_store_traces` is true.
pub fn build_contexts(
    storage: &RunStorage,
    app_data_dir: &Path,
    project_id: Option<String>,
    no_store_traces: bool,
) -> Result<(EpisodicContext, SkillContext)> {
    let persist = !no_store_traces;

    let skills_dir = storage
        .project_skills_dir()
        .context("Failed to resolve project skills directory")?;

    let project_id_str = project_id.unwrap_or_default();

    // Episodic context
    let episodic_local = storage.base_path().join("episodic.sqlite");
    let episodic_global = if persist {
        Some(clickweave_host::context::global_episodic_path(app_data_dir))
    } else {
        None
    };
    let episodic_ctx = clickweave_host::context::build_episodic_context(
        episodic_local,
        episodic_global,
        project_id_str.clone(),
        persist,
        persist,
    );

    // Skill context
    let global_skills = if persist {
        clickweave_host::context::global_skills_dir(app_data_dir).ok()
    } else {
        None
    };
    let skill_ctx = clickweave_host::context::build_skill_context(
        skills_dir,
        global_skills,
        project_id_str,
        persist,
        persist,
    );

    Ok((episodic_ctx, skill_ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_host::RunStorage;

    #[test]
    fn resolve_llm_config_from_flags() {
        let cfg = resolve_llm_config(
            Some("http://localhost:1234/v1".to_string()),
            Some("gpt-4o".to_string()),
            Some("sk-test".to_string()),
        )
        .unwrap();
        assert_eq!(cfg.base_url, "http://localhost:1234/v1");
        assert_eq!(cfg.model, "gpt-4o");
        assert_eq!(cfg.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn resolve_llm_config_empty_api_key_normalised() {
        let cfg = resolve_llm_config(
            Some("http://localhost:1234/v1".to_string()),
            Some("gpt-4o".to_string()),
            Some(String::new()),
        )
        .unwrap();
        assert_eq!(
            cfg.api_key, None,
            "empty api key should be normalised to None"
        );
    }

    #[test]
    fn resolve_llm_config_missing_base_url_errors() {
        // Remove env vars that might interfere.
        // SAFETY: single-threaded test, no concurrent env access.
        unsafe {
            std::env::remove_var("CLICKWEAVE_BASE_URL");
            std::env::remove_var("CLICKWEAVE_MODEL");
        }
        let result = resolve_llm_config(None, Some("model".to_string()), None);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_project_location_saved_path_returns_unsaved_on_missing_manifest() {
        // No project.json at /tmp/does_not_exist → error
        let result = resolve_project_location(
            Some(PathBuf::from("/tmp/does_not_exist_clickweave_test")),
            None,
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn resolve_project_location_unsaved_parses_uuid() {
        let loc = resolve_project_location(
            None,
            Some("my-wf".to_string()),
            Some("00000000-0000-0000-0000-000000000001".to_string()),
        )
        .unwrap();
        assert!(matches!(loc, ProjectLocation::Unsaved { name, .. } if name == "my-wf"));
    }

    #[test]
    fn resolve_project_location_invalid_uuid_errors() {
        let result = resolve_project_location(
            None,
            Some("my-wf".to_string()),
            Some("not-a-uuid".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn build_contexts_enabled_when_persistent() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = RunStorage::new(tmp.path(), "test-wf");
        let app_data = tmp.path().join("appdata");
        std::fs::create_dir_all(&app_data).unwrap();

        let (ep, sk) = build_contexts(
            &storage,
            &app_data,
            Some("proj-id".to_string()),
            false, // no_store_traces = false → enabled
        )
        .unwrap();

        assert!(ep.enabled, "episodic context should be enabled");
        assert!(sk.enabled, "skill context should be enabled");
    }

    #[test]
    fn build_contexts_disabled_when_no_store_traces() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = RunStorage::new(tmp.path(), "test-wf");
        let app_data = tmp.path().join("appdata");
        std::fs::create_dir_all(&app_data).unwrap();

        let (ep, sk) = build_contexts(
            &storage,
            &app_data,
            Some("proj-id".to_string()),
            true, // no_store_traces = true → disabled
        )
        .unwrap();

        assert!(
            !ep.enabled,
            "episodic context should be disabled with --no-store-traces"
        );
        assert!(
            !sk.enabled,
            "skill context should be disabled with --no-store-traces"
        );
    }
}
