use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::args::{SkillsArgs, SkillsSubcommand};

/// Execute the `skills` subcommand.
///
/// Returns the exit code.
pub async fn execute(args: SkillsArgs) -> Result<i32> {
    match args.subcommand {
        SkillsSubcommand::List { project } => list_skills(project).await,
        SkillsSubcommand::Show { skill_id, project } => show_skill(skill_id, project).await,
    }
}

async fn list_skills(project: Option<PathBuf>) -> Result<i32> {
    let skills_dir = resolve_skills_dir(project.as_deref())?;
    let summaries =
        clickweave_host::skills::list_skills(&skills_dir).context("Failed to list skills")?;

    if summaries.is_empty() {
        println!("No skills found in {}", skills_dir.display());
    } else {
        for s in &summaries {
            println!("{}\t{}\tv{}\t[{:?}]", s.id, s.name, s.version, s.state);
        }
    }

    Ok(0)
}

async fn show_skill(skill_id: String, project: Option<PathBuf>) -> Result<i32> {
    let skills_dir = resolve_skills_dir(project.as_deref())?;
    let skill = clickweave_host::skills::load_skill(&skills_dir, &skill_id)
        .with_context(|| format!("Skill '{skill_id}' not found"))?;

    println!("ID:          {}", skill.id);
    println!("Name:        {}", skill.name);
    println!("Version:     {}", skill.version);
    println!("State:       {:?}", skill.state);
    println!("Description: {}", skill.description);
    println!("Steps:       {}", skill.action_sketch.len());

    Ok(0)
}

/// Resolve the skills directory from an optional project path.
///
/// When `project` is `None`, looks in the current directory for a `.clickweave/skills/` subtree.
fn resolve_skills_dir(project: Option<&std::path::Path>) -> Result<PathBuf> {
    match project {
        Some(path) => {
            let dir = clickweave_host::storage::project_dir(path);
            Ok(dir.join(".clickweave").join("skills"))
        }
        None => {
            let cwd = std::env::current_dir().context("Failed to get current directory")?;
            Ok(cwd.join(".clickweave").join("skills"))
        }
    }
}
