//! Spec 3 Phase 3 integration tests for `maybe_extract_skill`.
//!
//! Drives the extractor through synthetic `RecordedStep` streams and
//! asserts the on-disk store + in-memory index reach the expected
//! shape: single insert, repeat-merge, divergent fork, and cross-step
//! provenance threading.

use std::sync::Arc;

use clickweave_engine::agent::skills::extractor::maybe_extract_skill;
use clickweave_engine::agent::skills::signature::compute_subgoal_signature;
use clickweave_engine::agent::skills::{
    ActionSketchStep, RecordedStep, Skill, SkillContext, SkillIndex, SkillStore,
};
use clickweave_engine::agent::step_record::WorldModelSnapshot;
use clickweave_engine::agent::task_state::{Milestone, SubgoalId};
use clickweave_engine::agent::world_model::WorldModel;
use parking_lot::RwLock;
use tempfile::TempDir;
use uuid::Uuid;

fn step(tool: &str, args: serde_json::Value, result: &str) -> RecordedStep {
    let wm = WorldModel::default();
    RecordedStep {
        tool_name: tool.into(),
        arguments: args,
        result_text: result.into(),
        world_model_pre: WorldModelSnapshot::from_world_model(&wm),
        world_model_post: WorldModelSnapshot::from_world_model(&wm),
    }
}

fn milestone(text: &str) -> Milestone {
    Milestone {
        subgoal_id: SubgoalId::new(),
        text: text.into(),
        summary: "ok".into(),
        pushed_at_step: 0,
        completed_at_step: 1,
    }
}

fn fixture(
    enabled: bool,
) -> (
    TempDir,
    SkillStore,
    Arc<RwLock<SkillIndex>>,
    SkillContext,
    WorldModel,
) {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().to_path_buf();
    let store = SkillStore::new(dir.clone());
    let embedder = Arc::new(clickweave_engine::agent::episodic::HashedShingleEmbedder::default());
    let index = Arc::new(RwLock::new(SkillIndex::empty(embedder)));
    let ctx = SkillContext {
        enabled,
        project_skills_dir: dir,
        global_skills_dir: None,
        project_id: "p".into(),
    };
    (tmp, store, index, ctx, WorldModel::default())
}

#[tokio::test]
async fn single_subgoal_three_steps_writes_one_draft() {
    let (_tmp, store, index, ctx, wm) = fixture(true);
    let m = milestone("open vesna chat");
    let actions = vec![
        step("click", serde_json::json!({"x": 1}), r#"{"ok":1}"#),
        step("type_text", serde_json::json!({"text": "hi"}), r#"{}"#),
        step("press_key", serde_json::json!({"key": "Enter"}), r#"{}"#),
    ];

    let out = maybe_extract_skill(
        &m,
        &actions,
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf-1",
        3,
        &[],
    )
    .await
    .unwrap();

    use clickweave_engine::agent::skills::MaybeExtracted;
    matches!(out, MaybeExtracted::Inserted { .. });
    assert_eq!(index.read().len(), 1);
    let drafts = index
        .read()
        .skills_in_state(clickweave_engine::agent::skills::SkillState::Draft);
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].stats.occurrence_count, 1);
}

#[tokio::test]
async fn second_identical_invocation_merges_with_occurrence_bump() {
    let (_tmp, store, index, ctx, wm) = fixture(true);
    let m = milestone("open vesna chat");
    let actions = vec![step("click", serde_json::json!({"x": 1}), r#"{}"#)];

    maybe_extract_skill(
        &m,
        &actions,
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf-1",
        1,
        &[],
    )
    .await
    .unwrap();

    let out = maybe_extract_skill(
        &m,
        &actions,
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf-1",
        2,
        &[],
    )
    .await
    .unwrap();

    use clickweave_engine::agent::skills::MaybeExtracted;
    match out {
        MaybeExtracted::Merged {
            occurrence_count, ..
        } => assert_eq!(occurrence_count, 2),
        other => panic!("expected Merged, got {:?}", other),
    }
}

#[tokio::test]
async fn divergent_invocation_inserts_new_version_in_same_family() {
    let (_tmp, store, index, ctx, wm) = fixture(true);
    let m = milestone("open vesna chat");

    maybe_extract_skill(
        &m,
        &[step("click", serde_json::json!({"x": 1}), r#"{}"#)],
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf-1",
        1,
        &[],
    )
    .await
    .unwrap();
    let out = maybe_extract_skill(
        &m,
        &[
            step("type_text", serde_json::json!({"text": "x"}), r#"{}"#),
            step("press_key", serde_json::json!({"key": "Enter"}), r#"{}"#),
        ],
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf-1",
        2,
        &[],
    )
    .await
    .unwrap();

    use clickweave_engine::agent::skills::MaybeExtracted;
    match out {
        MaybeExtracted::Inserted { version, skill_id } => {
            assert_eq!(version, 2);
            // The id must match the v1 skill's id (same signature
            // family, divergent sketch produces v + 1, not a new id).
            let drafts = index
                .read()
                .skills_in_state(clickweave_engine::agent::skills::SkillState::Draft);
            let ids: Vec<_> = drafts.iter().map(|s: &Arc<Skill>| s.id.clone()).collect();
            assert!(ids.iter().all(|id| id == &skill_id));
            assert_eq!(drafts.len(), 2);
        }
        other => panic!("expected Inserted v2, got {:?}", other),
    }
}

#[tokio::test]
async fn cross_step_provenance_threads_captured_reference() {
    // Step 0 produces a result containing the literal "Vesna Petrovich".
    // Step 1 then types that exact literal — the action sketch should
    // route it as `{{captured.*}}` rather than baking the literal in.
    let (_tmp, store, index, ctx, wm) = fixture(true);
    let m = milestone("open chat");
    let actions = vec![
        step(
            "ax_select",
            serde_json::json!({"role": "row"}),
            r#"{"selected_name": "Vesna Petrovich"}"#,
        ),
        step(
            "type_text",
            serde_json::json!({"text": "Vesna Petrovich"}),
            r#"{}"#,
        ),
    ];
    maybe_extract_skill(
        &m,
        &actions,
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf-1",
        2,
        &[],
    )
    .await
    .unwrap();

    let drafts = index
        .read()
        .skills_in_state(clickweave_engine::agent::skills::SkillState::Draft);
    let skill = drafts.first().expect("draft skill present").clone();
    let step1_args = match &skill.action_sketch[1] {
        ActionSketchStep::ToolCall { args, .. } => args.clone(),
        other => panic!("expected ToolCall, got {:?}", other),
    };
    let text = step1_args
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        text.contains("{{captured."),
        "cross-step literal should rewrite to a captured reference; got: {text}",
    );
}

#[tokio::test]
async fn extraction_skipped_when_disabled() {
    let (_tmp, store, index, ctx, wm) = fixture(false);
    let m = milestone("any subgoal");
    let out = maybe_extract_skill(
        &m,
        &[step("click", serde_json::json!({"x": 1}), r#"{}"#)],
        compute_subgoal_signature(&m.text, &wm),
        &wm,
        &index,
        &store,
        &ctx,
        Uuid::nil(),
        "wf",
        1,
        &[],
    )
    .await
    .unwrap();
    use clickweave_engine::agent::skills::MaybeExtracted;
    match out {
        MaybeExtracted::Skipped { reason } => assert_eq!(reason, "disabled"),
        other => panic!("expected Skipped, got {:?}", other),
    }
    assert_eq!(index.read().len(), 0);
}
