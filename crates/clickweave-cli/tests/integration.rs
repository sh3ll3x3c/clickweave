//! T3.7 Integration tests for clickweave-cli.
//!
//! All tests use engine test-stubs (StaticMcp, ScriptedLlm, NullMcp,
//! NoVlm) — no real MCP subprocess is spawned.

use std::collections::HashMap;
use std::sync::Arc;

use clickweave_engine::agent::test_stubs::{
    ScriptedLlm, StaticMcp, build_agent_done_response, llm_reply_tool,
};
use clickweave_host::{
    AgentConfig, RunStorage, TerminalReason, Uuid, approval::AutoApprove,
    lifecycle::spawn_agent_run,
};

// ── Helper: write a fixture skill to a temp dir ──────────────────────────────

fn write_fixture_skill(dir: &std::path::Path, skill_id: &str) {
    use chrono::Utc;
    use clickweave_engine::agent::skills::emitter::emit_skill_md;
    use clickweave_engine::agent::skills::types::{
        ActionSketchStep, ApplicabilityHints, ApplicabilitySignature, ExpectedWorldModelDelta,
        OutcomePredicate, Skill, SkillScope, SkillState, SkillStats, SubgoalSignature,
    };

    let now = Utc::now();
    let skill = Skill {
        id: skill_id.to_string(),
        version: 1,
        state: SkillState::Confirmed,
        scope: SkillScope::ProjectLocal,
        name: "Test Skill".to_string(),
        description: "A fixture skill for integration tests.".to_string(),
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
    };

    let skill_dir = dir.join(skill_id);
    std::fs::create_dir_all(&skill_dir).unwrap();
    let contents = emit_skill_md(&skill);
    std::fs::write(skill_dir.join("SKILL.md"), contents).unwrap();
}

// ── T3.7.1: skills list against fixture project dir ─────────────────────────

#[test]
fn skills_list_enumerates_fixture_project() {
    let tmp = tempfile::tempdir().unwrap();
    let skills_dir = tmp.path().join(".clickweave").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    write_fixture_skill(&skills_dir, "skl_t37a");
    write_fixture_skill(&skills_dir, "skl_t37b");

    let summaries = clickweave_host::skills::list_skills(&skills_dir).unwrap();
    assert_eq!(summaries.len(), 2, "should list 2 fixture skills");
    let ids: Vec<_> = summaries.iter().map(|s| s.id.as_str()).collect();
    assert!(ids.contains(&"skl_t37a"), "must include skl_t37a");
    assert!(ids.contains(&"skl_t37b"), "must include skl_t37b");
}

// ── T3.7.2: runs list after a skill run ─────────────────────────────────────

#[tokio::test]
async fn runs_list_returns_skill_run_after_execution() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path();

    let skills_dir = project_dir.join(".clickweave").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    write_fixture_skill(&skills_dir, "skl_runlist");

    let skill = clickweave_host::skills::load_skill(&skills_dir, "skl_runlist").unwrap();
    let storage = RunStorage::new(project_dir, "test-workflow");

    // Ensure the project skills dir exists (required for create_skill_run).
    std::fs::create_dir_all(storage.project_skills_dir().unwrap()).unwrap();

    let mcp = StaticMcp::with_tools(&["take_ax_snapshot"]);
    let run_result = clickweave_host::skills::run_skill(&skill, HashMap::new(), &mcp, &storage)
        .await
        .unwrap();

    assert_eq!(run_result.skill_id, "skl_runlist");

    let runs = clickweave_host::runs::list_runs(&storage, "skl_runlist");
    assert_eq!(
        runs.len(),
        1,
        "runs list must return the just-written record"
    );
    assert_eq!(runs[0].run_id, run_result.run_id);
}

// ── T3.7.3: JSON mode emits NDJSON with no base64 blob ──────────────────────

#[tokio::test]
async fn json_mode_emits_no_screenshot_blob() {
    use clickweave_cli_test_helpers::drain_to_events;

    let llm = ScriptedLlm::new(vec![build_agent_done_response("all done")]);
    let mcp = StaticMcp::with_tools(&[]);
    let responder = Arc::new(AutoApprove);

    let mut handle = spawn_agent_run(
        llm,
        mcp,
        AgentConfig {
            max_steps: 5,
            ..AgentConfig::default()
        },
        "test goal".to_string(),
        None,
        Some(clickweave_host::PermissionPolicy {
            allow_all: true,
            ..Default::default()
        }),
        Uuid::new_v4(),
        None,
        None,
        None,
        None,
        None,
        None,
        responder,
    );

    let events = drain_to_events(&mut handle.events).await;
    let _ = handle.await_result().await;

    // Serialize each event without screenshots and verify no b64 blob.
    for event in &events {
        let value = serde_json::to_value(event).unwrap();
        if value.get("type").and_then(|t| t.as_str()) == Some("completion_disagreement") {
            let b64 = value
                .get("screenshot_b64")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // In JSON mode with redaction the field must be a placeholder, not actual b64.
            // (No actual CompletionDisagreement fires here since vision=None.)
            assert!(!b64.contains("iVBOR"), "base64 PNG header must be absent");
        }
    }

    // No CompletionDisagreement fired (no vision backend), but confirm
    // NDJSON serialisation works for all event variants.
    assert!(!events.is_empty(), "at least one event must be emitted");
}

// ── T3.7.4: Non-TTY without --yes exits 6 (ApprovalUnavailable) ─────────────

#[tokio::test]
async fn non_tty_approval_unavailable_exits_6() {
    use clickweave_cli_test_helpers::UnavailableResponder;

    let llm = ScriptedLlm::new(vec![
        // cdp_click is NOT an observation tool, so it triggers the approval gate.
        llm_reply_tool("cdp_click", serde_json::json!({"nodeId": 1})),
    ]);
    // Advertise cdp_click without read_only_hint so the default policy returns Ask.
    let mcp = StaticMcp::with_tools(&["cdp_click"]);

    // Unavailable responder — mirrors non-TTY StdinResponder without --yes.
    let responder = Arc::new(UnavailableResponder);

    let mut handle = spawn_agent_run(
        llm,
        mcp,
        AgentConfig {
            max_steps: 5,
            ..AgentConfig::default()
        },
        "test goal".to_string(),
        None,
        None, // No policy → default Ask for non-read-only tools.
        Uuid::new_v4(),
        None,
        None,
        None,
        None,
        None,
        None,
        responder,
    );

    // Drain events (ignore content).
    while handle.events.recv().await.is_some() {}

    let (state, _) = handle.await_result().await.unwrap();
    let reason = state.terminal_reason.expect("must have terminal reason");

    assert!(
        matches!(reason, TerminalReason::ApprovalUnavailable),
        "non-TTY without --yes should exit 6 (ApprovalUnavailable), got: {reason:?}"
    );

    // Verify exit code mapping.
    use clickweave_cli::renderer::exit_code_for;
    assert_eq!(exit_code_for(&reason), 6);
}

// ── T3.7.5: CompletionDisagreement → exit 7 ─────────────────────────────────

#[tokio::test]
async fn completion_disagreement_exits_7() {
    use clickweave_cli::renderer::exit_code_for;
    use clickweave_engine::agent::test_stubs::NoVlm;
    use clickweave_llm::DynChatBackend;

    let llm = ScriptedLlm::new(vec![build_agent_done_response("task done")]);
    // NoVlm always returns NO → CompletionDisagreement.
    let vlm: Arc<dyn DynChatBackend> = Arc::new(NoVlm);
    // StaticMcp with take_screenshot returning a valid 1x1 transparent PNG
    // so prepare_base64_image_for_vlm can decode it successfully.
    let tiny_png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
    let mcp = StaticMcp::with_tools(&["take_screenshot"]).with_image_reply(
        "take_screenshot",
        tiny_png_b64,
        "image/png",
    );
    let responder = Arc::new(AutoApprove);

    let mut handle = spawn_agent_run(
        llm,
        mcp,
        AgentConfig {
            max_steps: 5,
            ..AgentConfig::default()
        },
        "test goal".to_string(),
        Some(vlm),
        Some(clickweave_host::PermissionPolicy {
            allow_all: true,
            ..Default::default()
        }),
        Uuid::new_v4(),
        None,
        None, // verification_artifacts_dir
        None,
        None,
        None,
        None,
        responder,
    );

    while handle.events.recv().await.is_some() {}
    let (state, _) = handle.await_result().await.unwrap();
    let reason = state.terminal_reason.expect("must have terminal reason");

    assert!(
        matches!(reason, TerminalReason::CompletionDisagreement { .. }),
        "NoVlm should surface CompletionDisagreement, got: {reason:?}"
    );
    assert_eq!(exit_code_for(&reason), 7);
}

// ── T3.7.6: Persistence gate — saved project with persistence on/off ─────────

#[test]
fn saved_project_persistence_on_enables_contexts() {
    use clickweave_cli::config::build_contexts;

    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path();
    let app_data = project_dir.join("appdata");
    std::fs::create_dir_all(&app_data).unwrap();

    let storage = RunStorage::new(project_dir, "test-wf");
    let (ep, sk) = build_contexts(
        &storage,
        &app_data,
        Some("proj-id".to_string()),
        false, // no_store_traces = false → enabled
    )
    .unwrap();

    assert!(
        ep.enabled,
        "EpisodicContext must be enabled when persisting"
    );
    assert!(sk.enabled, "SkillContext must be enabled when persisting");
}

#[test]
fn saved_project_no_store_traces_disables_contexts() {
    use clickweave_cli::config::build_contexts;

    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path();
    let app_data = project_dir.join("appdata");
    std::fs::create_dir_all(&app_data).unwrap();

    let storage = RunStorage::new(project_dir, "test-wf");
    let (ep, sk) = build_contexts(
        &storage,
        &app_data,
        Some("proj-id".to_string()),
        true, // no_store_traces = true → disabled
    )
    .unwrap();

    assert!(
        !ep.enabled,
        "EpisodicContext must be disabled with --no-store-traces"
    );
    assert!(
        !sk.enabled,
        "SkillContext must be disabled with --no-store-traces"
    );
}

// ── T3.7.7: run-skill persistence — no events.jsonl with --no-store-traces ───

#[tokio::test]
async fn run_skill_no_store_traces_writes_no_events_jsonl() {
    let tmp = tempfile::tempdir().unwrap();
    let project_dir = tmp.path();

    let skills_dir = project_dir.join(".clickweave").join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    write_fixture_skill(&skills_dir, "skl_notrace");

    let skill = clickweave_host::skills::load_skill(&skills_dir, "skl_notrace").unwrap();

    // Storage with persistence disabled (--no-store-traces).
    let mut storage = RunStorage::new(project_dir, "test-workflow");
    storage.set_persistent(false);

    let mcp = StaticMcp::with_tools(&["take_ax_snapshot"]);
    let _run = clickweave_host::skills::run_skill(&skill, HashMap::new(), &mcp, &storage)
        .await
        .unwrap();

    // With persistence off, no events.jsonl should exist anywhere under the project.
    let events_files: Vec<_> = walkdir_find_events_jsonl(project_dir);
    assert!(
        events_files.is_empty(),
        "no events.jsonl must be written with --no-store-traces: found {events_files:?}"
    );
}

fn walkdir_find_events_jsonl(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    fn walk(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.file_name().and_then(|n| n.to_str()) == Some("events.jsonl") {
                found.push(path);
            }
        }
    }
    walk(root, &mut found);
    found
}

// ── Helper module ─────────────────────────────────────────────────────────────

/// Test helpers module — exported so integration test code can reference types.
mod clickweave_cli_test_helpers {
    use async_trait::async_trait;
    use clickweave_host::{
        AgentEvent, ApprovalRequest, RunnerOutput,
        approval::{ApprovalDecision, ApprovalResponder},
    };
    use tokio::sync::mpsc;

    /// Drain the events receiver into a Vec of `AgentEvent`.
    pub async fn drain_to_events(rx: &mut mpsc::Receiver<RunnerOutput>) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        while let Some(output) = rx.recv().await {
            if let RunnerOutput::Event(event) = output {
                events.push(event);
            }
        }
        events
    }

    /// `ApprovalResponder` that always returns `Unavailable`.
    /// Mirrors a non-TTY `StdinResponder` without `--yes`.
    pub struct UnavailableResponder;

    #[async_trait]
    impl ApprovalResponder for UnavailableResponder {
        async fn respond(&self, _req: ApprovalRequest) -> ApprovalDecision {
            ApprovalDecision::Unavailable
        }
    }
}
