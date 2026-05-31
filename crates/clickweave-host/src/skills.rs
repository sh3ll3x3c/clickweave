use std::collections::HashMap;
use std::path::Path;

use clickweave_core::SkillRun;
use clickweave_core::storage::RunStorage;
use clickweave_engine::agent::skills::store::SkillStore;
use clickweave_engine::agent::skills::types::{Skill, SkillState};
use clickweave_engine::executor::error::ExecutorResult;
use clickweave_engine::executor::skill_runner::{SkillRunContext, run_skill_steps};
use serde_json::Value;
use tracing::warn;

/// Minimal summary of a skill — enough to render a list entry.
#[derive(Debug, Clone)]
pub struct SkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: u32,
    pub state: SkillState,
}

/// List every skill in `dir`, returning a `SkillSummary` for each.
///
/// Skills that fail to parse are logged and skipped.
pub fn list_skills(dir: &Path) -> anyhow::Result<Vec<SkillSummary>> {
    let store = SkillStore::new(dir.to_path_buf());
    let files = store.list_files()?;
    let mut summaries = Vec::new();
    for path in files {
        match store.read_skill(&path) {
            Ok(skill) => summaries.push(SkillSummary {
                id: skill.id.clone(),
                name: skill.name.clone(),
                description: skill.description.clone(),
                version: skill.version,
                state: skill.state,
            }),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Skipping unreadable skill");
            }
        }
    }
    Ok(summaries)
}

/// Load a single skill by `id` from `dir`.
///
/// Returns `Err` if the skill is not found or cannot be parsed.
pub fn load_skill(dir: &Path, id: &str) -> anyhow::Result<Skill> {
    let store = SkillStore::new(dir.to_path_buf());
    let files = store.list_files()?;
    for path in files {
        match store.read_skill(&path) {
            Ok(skill) if skill.id == id => return Ok(skill),
            Ok(_) => continue,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Skipping unreadable skill");
            }
        }
    }
    anyhow::bail!("Skill '{}' not found in {}", id, dir.display())
}

/// Execute a skill, persisting the run record via `storage`.
///
/// Mirrors the Tauri shell's `executor::run_skill_dispatch` but is
/// storage-backed so CLI skill runs are listable by `runs list/events`.
pub async fn run_skill<M>(
    skill: &Skill,
    variables: HashMap<String, Value>,
    mcp: &M,
    storage: &RunStorage,
) -> ExecutorResult<SkillRun>
where
    M: clickweave_engine::executor::Mcp + ?Sized,
{
    let run = storage.create_skill_run(&skill.id).map_err(|e| {
        clickweave_engine::executor::error::ExecutorError::ToolCall {
            tool: "storage".to_string(),
            message: format!("create skill run: {e}"),
        }
    })?;

    let mut ctx = SkillRunContext::new(mcp, variables);
    let outcome = run_skill_steps(&mut ctx, &skill.action_sketch).await;

    let mut updated = run.clone();
    updated.finished_at = Some(chrono::Utc::now());
    updated.duration_ms = Some(
        (updated.finished_at.unwrap() - updated.started_at)
            .num_milliseconds()
            .max(0) as u64,
    );
    updated.status = match &outcome {
        Ok(()) => clickweave_core::RunStatus::Ok,
        Err(_) => clickweave_core::RunStatus::Failed,
    };

    if let Err(e) = storage.save_skill_run(&updated) {
        warn!(error = %e, "Failed to persist skill-run terminal record");
    }

    outcome.map(|()| updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use clickweave_engine::agent::skills::emitter::emit_skill_md;
    use clickweave_engine::agent::skills::types::{
        ActionSketchStep, ApplicabilityHints, ApplicabilitySignature, ExpectedWorldModelDelta,
        OutcomePredicate, Skill, SkillScope, SkillState, SkillStats, SubgoalSignature,
    };
    use clickweave_engine::agent::test_stubs::StaticMcp;

    /// Build a minimal in-memory `Skill` and emit it to disk as SKILL.md.
    fn write_fixture_skill(dir: &Path, skill_id: &str) {
        let skill = make_fixture_skill(skill_id);
        let skill_dir = dir.join(skill_id);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let contents = emit_skill_md(&skill);
        std::fs::write(skill_dir.join("SKILL.md"), contents).unwrap();
    }

    fn make_fixture_skill(skill_id: &str) -> Skill {
        let now = Utc::now();
        Skill {
            id: skill_id.to_string(),
            version: 1,
            state: SkillState::Confirmed,
            scope: SkillScope::ProjectLocal,
            name: "Test Skill".to_string(),
            description: "A fixture skill for unit tests.".to_string(),
            tags: vec![],
            subgoal_text: "test skill".to_string(),
            subgoal_signature: SubgoalSignature(String::new()),
            applicability: ApplicabilityHints {
                apps: vec![],
                hosts: vec![],
                signature: ApplicabilitySignature(String::new()),
            },
            parameter_schema: vec![],
            action_sketch: vec![ActionSketchStep::ToolCall {
                step_id: "s_001".to_string(),
                tool: "take_ax_snapshot".to_string(),
                args: serde_json::json!({}),
                captures_pre: vec![],
                captures: vec![],
                expected_world_model_delta: ExpectedWorldModelDelta::default(),
                requires_approval: None,
            }],
            outputs: vec![],
            outcome_predicate: OutcomePredicate::SubgoalCompleted {
                post_state_world_model_signature: None,
            },
            provenance: vec![],
            stats: SkillStats::default(),
            edited_by_user: false,
            created_at: now,
            updated_at: now,
            produced_node_ids: vec![],
            body: "## Step One\n<!-- section: sec_001 -->\n<!-- step: s_001 -->\n\nDo something.\n"
                .to_string(),
            schema_version: 1,
            variables: vec![],
            sections: vec![clickweave_engine::agent::skills::types::SkillSection {
                id: "sec_001".to_string(),
                heading: "Step One".to_string(),
                level: 2,
                step_ids: vec!["s_001".to_string()],
                body_range: (0, 60),
            }],
            replay: None,
        }
    }

    #[test]
    fn list_skills_enumerates_fixture() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_skill(tmp.path(), "skl_test01");

        let summaries = list_skills(tmp.path()).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, "skl_test01");
        assert_eq!(summaries[0].name, "Test Skill");
    }

    #[test]
    fn load_skill_returns_correct_skill() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_skill(tmp.path(), "skl_test02");

        let skill = load_skill(tmp.path(), "skl_test02").unwrap();
        assert_eq!(skill.id, "skl_test02");
        assert!(!skill.description.is_empty());
    }

    #[test]
    fn load_skill_errors_for_missing_id() {
        let tmp = tempfile::tempdir().unwrap();
        write_fixture_skill(tmp.path(), "skl_exists");

        let err = load_skill(tmp.path(), "skl_missing").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn run_skill_executes_and_writes_run_record() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path();

        // Build a skill with a single tool call step.
        write_fixture_skill(project_dir, "skl_runtest");
        let skill = load_skill(project_dir, "skl_runtest").unwrap();

        // Set up storage.
        let storage = RunStorage::new(project_dir, "test-workflow");

        // Use a StaticMcp stub that returns "ok" for take_ax_snapshot.
        let mcp = StaticMcp::with_tools(&["take_ax_snapshot"]);

        let run_result = run_skill(&skill, HashMap::new(), &mcp, &storage).await;
        assert!(run_result.is_ok(), "run_skill should succeed");

        let skill_run = run_result.unwrap();
        assert_eq!(skill_run.skill_id, "skl_runtest");
        assert_eq!(skill_run.status, clickweave_core::RunStatus::Ok);

        // Verify the run record was persisted and is listable.
        let runs = storage.load_runs_for_skill("skl_runtest").unwrap();
        assert_eq!(runs.len(), 1, "run record should have been persisted");
        assert_eq!(runs[0].run_id, skill_run.run_id);
    }
}
