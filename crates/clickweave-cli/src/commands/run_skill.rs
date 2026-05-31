use std::collections::HashMap;

use anyhow::{Context, Result};
use clickweave_host::{
    mcp::{EnvOverride, spawn_mcp},
    storage::{app_data_dir, resolve_storage},
};
use serde_json::Value;

use crate::args::RunSkillArgs;
use crate::config::resolve_project_location;

/// Execute the `run-skill` subcommand.
///
/// Returns the exit code (0 = success, 1 = failure).
pub async fn execute(args: RunSkillArgs) -> Result<i32> {
    // ── MCP ───────────────────────────────────────────────────────────────
    let mcp_binary = match args.mcp_binary {
        Some(p) => p,
        None => clickweave_host::mcp::resolve_mcp_binary(EnvOverride::Always)?,
    };
    let mcp = spawn_mcp(&mcp_binary, &[]).await?;

    // ── Project + storage ─────────────────────────────────────────────────
    let app_data = app_data_dir();
    let loc = resolve_project_location(args.project, args.project_name, args.project_id)?;

    let mut storage = resolve_storage(&app_data, loc);
    storage.set_persistent(!args.no_store_traces);

    // ── Skills directory ──────────────────────────────────────────────────
    let skills_dir = storage
        .project_skills_dir()
        .context("Failed to resolve project skills directory")?;

    // ── Load skill ────────────────────────────────────────────────────────
    let skill = clickweave_host::skills::load_skill(&skills_dir, &args.skill_id)
        .with_context(|| format!("Skill '{}' not found", args.skill_id))?;

    // ── Parse variables ───────────────────────────────────────────────────
    let mut variables = parse_vars(&args.vars)?;
    if let Some(ref vars_file) = args.vars_file {
        let data = std::fs::read_to_string(vars_file)
            .with_context(|| format!("Failed to read vars file: {}", vars_file.display()))?;
        let file_vars: HashMap<String, Value> = serde_json::from_str(&data)
            .with_context(|| format!("Failed to parse vars file: {}", vars_file.display()))?;
        // File vars are defaults; flag vars take precedence.
        for (k, v) in file_vars {
            variables.entry(k).or_insert(v);
        }
    }

    // ── Run the skill ─────────────────────────────────────────────────────
    let result = clickweave_host::skills::run_skill(&skill, variables, &mcp, &storage).await;

    match result {
        Ok(skill_run) => {
            println!(
                "Skill run completed: {} (status: {:?})",
                skill_run.run_id, skill_run.status
            );
            Ok(0)
        }
        Err(e) => {
            eprintln!("Skill run failed: {e}");
            Ok(1)
        }
    }
}

/// Parse `key=value` pairs from `--var` flags.
fn parse_vars(vars: &[String]) -> Result<HashMap<String, Value>> {
    let mut map = HashMap::new();
    for s in vars {
        let (k, v) = s
            .split_once('=')
            .with_context(|| format!("Invalid --var syntax (expected key=value): {s}"))?;
        // Try to parse as JSON; fall back to a string value.
        let value = serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_owned()));
        map.insert(k.to_owned(), value);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_vars_string_values() {
        let vars = vec!["name=Alice".to_string(), "city=London".to_string()];
        let map = parse_vars(&vars).unwrap();
        assert_eq!(map.get("name"), Some(&Value::String("Alice".to_string())));
        assert_eq!(map.get("city"), Some(&Value::String("London".to_string())));
    }

    #[test]
    fn parse_vars_json_value() {
        let vars = vec!["count=5".to_string()];
        let map = parse_vars(&vars).unwrap();
        assert_eq!(map.get("count"), Some(&Value::Number(5.into())));
    }

    #[test]
    fn parse_vars_invalid_format_errors() {
        let vars = vec!["no-equals-sign".to_string()];
        assert!(parse_vars(&vars).is_err());
    }

    #[test]
    fn parse_vars_empty_is_ok() {
        let map = parse_vars(&[]).unwrap();
        assert!(map.is_empty());
    }
}
