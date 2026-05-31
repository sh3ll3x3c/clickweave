use std::path::PathBuf;

use anyhow::{Context, Result};
use clickweave_host::{
    RunStorage, Uuid,
    storage::{ProjectLocation, app_data_dir, resolve_storage},
};

use crate::args::{RunsArgs, RunsSubcommand};

/// Execute the `runs` subcommand.
///
/// Returns the exit code.
pub async fn execute(args: RunsArgs) -> Result<i32> {
    match args.subcommand {
        RunsSubcommand::List { skill_id, project } => list_runs(skill_id, project).await,
        RunsSubcommand::Events {
            skill_id,
            run_id,
            project,
        } => show_events(skill_id, run_id, project).await,
    }
}

async fn list_runs(skill_id: String, project: Option<PathBuf>) -> Result<i32> {
    let storage = build_storage_for_project(project)?;
    let runs = clickweave_host::runs::list_runs(&storage, &skill_id);

    if runs.is_empty() {
        println!("No runs found for skill '{skill_id}'");
    } else {
        for r in &runs {
            println!("{}\t{:?}\t{}", r.run_id, r.status, r.started_at);
        }
    }

    Ok(0)
}

async fn show_events(
    skill_id: String,
    run_id_str: String,
    project: Option<PathBuf>,
) -> Result<i32> {
    let storage = build_storage_for_project(project)?;
    let run_id = run_id_str
        .parse::<Uuid>()
        .with_context(|| format!("Invalid run ID: {run_id_str}"))?;

    let events = clickweave_host::runs::load_run_events(&storage, &skill_id, run_id, None)?;

    if events.is_empty() {
        println!("No events found for run '{run_id}'");
    } else {
        for event in &events {
            let line = serde_json::to_string(event).unwrap_or_default();
            println!("{line}");
        }
    }

    Ok(0)
}

/// Build a `RunStorage` from an optional project path, falling back to a
/// generic unsaved project backed by the app-data directory.
fn build_storage_for_project(project: Option<PathBuf>) -> Result<RunStorage> {
    let app_data = app_data_dir();
    match project {
        Some(path) => {
            let manifest = clickweave_host::storage::load_project(&path)
                .with_context(|| format!("Failed to load project at {}", path.display()))?;
            let loc = ProjectLocation::Saved {
                path,
                name: manifest.name,
                id: manifest.id,
            };
            Ok(resolve_storage(&app_data, loc))
        }
        None => {
            // No project specified — use current directory as project root.
            let cwd = std::env::current_dir().context("Failed to get current directory")?;
            let loc = ProjectLocation::Saved {
                path: cwd,
                name: "clickweave-cli".to_string(),
                id: Uuid::nil(),
            };
            Ok(resolve_storage(&app_data, loc))
        }
    }
}
