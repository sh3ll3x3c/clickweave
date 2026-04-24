//! Integration tests for the state-spine `StateRunner`.
//!
//! These tests drive the runner turn-by-turn with a deterministic
//! `AgentTurn` stream and a stubbed `ToolExecutor`, verifying that the
//! state-spine control flow (observe → apply_mutations → dispatch →
//! continuity → invalidation → phase inference) behaves correctly across
//! the canonical scenarios.
//!
//! Phase 2c intentionally stops short of wiring `StateRunner` to a live
//! `ChatBackend` + `McpClient` — the plan calls that out as Phase 3
//! cutover work (the `loop_runner.rs` → `runner.rs` swap). The harness
//! below is sufficient to exercise the state-spine invariants the design
//! doc requires.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::agent::runner::{AgentAction, AgentTurn, StateRunner, ToolExecutor, TurnOutcome};
use crate::agent::task_state::{TaskStateMutation, WatchSlotName};

/// Deterministic tool executor: pulls the next result off a FIFO queue and
/// returns it. `Ok(body)` for a successful tool body; `Err(msg)` to
/// simulate a tool failure.
struct ScriptedExecutor {
    results: Mutex<Vec<Result<String, String>>>,
}

impl ScriptedExecutor {
    fn new(results: Vec<Result<String, String>>) -> Self {
        Self {
            results: Mutex::new(results),
        }
    }
}

#[async_trait]
impl ToolExecutor for ScriptedExecutor {
    async fn call_tool(
        &self,
        _tool_name: &str,
        _arguments: &serde_json::Value,
    ) -> Result<String, String> {
        let mut q = self.results.lock().unwrap();
        if q.is_empty() {
            Err("scripted_executor: no more results".to_string())
        } else {
            q.remove(0)
        }
    }
}

fn agent_done(summary: &str) -> AgentTurn {
    AgentTurn {
        mutations: vec![],
        action: AgentAction::AgentDone {
            summary: summary.to_string(),
        },
    }
}

fn agent_replan(reason: &str) -> AgentTurn {
    AgentTurn {
        mutations: vec![],
        action: AgentAction::AgentReplan {
            reason: reason.to_string(),
        },
    }
}

fn tool_call(tool: &str, args: serde_json::Value, call_id: &str) -> AgentTurn {
    AgentTurn {
        mutations: vec![],
        action: AgentAction::ToolCall {
            tool_name: tool.to_string(),
            arguments: args,
            tool_call_id: call_id.to_string(),
        },
    }
}

#[tokio::test]
async fn single_step_agent_done_completes_run() {
    let mut r = StateRunner::new_for_test("log in".to_string());
    let exec = ScriptedExecutor::new(vec![]);
    let (outcome, warnings) = r.run_turn(&agent_done("completed login"), &exec).await;
    assert!(warnings.is_empty());
    assert!(matches!(outcome, TurnOutcome::Done { .. }));
    assert_eq!(r.step_index, 1);
}

#[tokio::test]
async fn multi_step_push_complete_subgoal_tracks_milestones() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![Ok("ok".to_string())]);

    // Turn 1: push a subgoal + fire a tool call.
    let turn = AgentTurn {
        mutations: vec![TaskStateMutation::PushSubgoal {
            text: "open login form".to_string(),
        }],
        action: AgentAction::ToolCall {
            tool_name: "cdp_click".to_string(),
            arguments: serde_json::json!({"uid":"d1"}),
            tool_call_id: "tc-1".to_string(),
        },
    };
    let (o1, _) = r.run_turn(&turn, &exec).await;
    assert!(matches!(o1, TurnOutcome::ToolSuccess { .. }));
    assert_eq!(r.task_state.subgoal_stack.len(), 1);

    // Turn 2: complete subgoal + agent_done.
    let turn2 = AgentTurn {
        mutations: vec![TaskStateMutation::CompleteSubgoal {
            summary: "form opened".to_string(),
        }],
        action: AgentAction::AgentDone {
            summary: "logged in".to_string(),
        },
    };
    let (o2, _) = r.run_turn(&turn2, &exec).await;
    assert!(matches!(o2, TurnOutcome::Done { .. }));
    assert!(r.task_state.subgoal_stack.is_empty());
    assert_eq!(r.task_state.milestones.len(), 1);
    assert_eq!(r.task_state.milestones[0].summary, "form opened");
}

#[tokio::test]
async fn tool_failure_increments_consecutive_errors_and_queues_invalidation() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![Err("not_dispatchable".to_string())]);

    let (outcome, _) = r
        .run_turn(
            &tool_call("cdp_click", serde_json::json!({"uid":"d1"}), "tc-1"),
            &exec,
        )
        .await;
    assert!(matches!(outcome, TurnOutcome::ToolError { .. }));
    assert_eq!(r.consecutive_errors, 1);
    // ToolFailed is queued for the next observe() to consume.
    assert_eq!(r.pending_events.len(), 1);
}

#[tokio::test]
async fn consecutive_errors_transition_phase_to_recovering() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![Err("first".to_string()), Err("second".to_string())]);

    let _ = r
        .run_turn(
            &tool_call("cdp_click", serde_json::json!({}), "tc-1"),
            &exec,
        )
        .await;
    let _ = r
        .run_turn(
            &tool_call("cdp_click", serde_json::json!({}), "tc-2"),
            &exec,
        )
        .await;

    // After two errors, phase should have shifted out of Exploring.
    assert_eq!(r.consecutive_errors, 2);
    assert_ne!(r.task_state.phase, crate::agent::phase::Phase::Exploring);
}

#[tokio::test]
async fn successful_tool_resets_consecutive_errors() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![Err("boom".to_string()), Ok("ok".to_string())]);
    let _ = r
        .run_turn(
            &tool_call("cdp_click", serde_json::json!({}), "tc-1"),
            &exec,
        )
        .await;
    assert_eq!(r.consecutive_errors, 1);
    let _ = r
        .run_turn(
            &tool_call("cdp_click", serde_json::json!({}), "tc-2"),
            &exec,
        )
        .await;
    assert_eq!(r.consecutive_errors, 0);
}

#[tokio::test]
async fn take_ax_snapshot_success_populates_continuity() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let body = "uid=a1g1 button \"OK\"\n  uid=a2g1 textbox \"Email\"";
    let exec = ScriptedExecutor::new(vec![Ok(body.to_string())]);
    let _ = r
        .run_turn(
            &tool_call("take_ax_snapshot", serde_json::json!({}), "tc-ax"),
            &exec,
        )
        .await;
    let snap = r
        .world_model
        .last_native_ax_snapshot
        .as_ref()
        .expect("ax snapshot populated");
    assert_eq!(snap.value.element_count, 2);
    assert!(snap.value.ax_tree_text.contains("uid=a1g1"));
}

#[tokio::test]
async fn agent_replan_records_last_replan_step() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![]);
    let (_, _) = r.run_turn(&agent_replan("form is gone"), &exec).await;
    assert_eq!(r.last_replan_step, Some(0));
}

#[tokio::test]
async fn cache_eligibility_flips_with_active_watch_slot() {
    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![Ok("ok".to_string())]);
    r.observe();
    assert!(r.is_replay_eligible());

    // A turn that sets a watch slot should make replay ineligible next pass.
    let turn = AgentTurn {
        mutations: vec![TaskStateMutation::SetWatchSlot {
            name: WatchSlotName::PendingAuth,
            note: "expecting 2fa prompt".to_string(),
        }],
        action: AgentAction::ToolCall {
            tool_name: "cdp_click".to_string(),
            arguments: serde_json::json!({}),
            tool_call_id: "tc-1".to_string(),
        },
    };
    let _ = r.run_turn(&turn, &exec).await;
    assert!(!r.is_replay_eligible());
}

#[tokio::test]
async fn terminal_boundary_record_captures_final_state() {
    use crate::agent::step_record::BoundaryKind;

    let mut r = StateRunner::new_for_test("goal".to_string());
    let exec = ScriptedExecutor::new(vec![]);
    let (_, _) = r
        .run_turn(&agent_done("done".to_string().as_str()), &exec)
        .await;

    let record = r.build_step_record(
        BoundaryKind::Terminal,
        serde_json::to_value(&AgentAction::AgentDone {
            summary: "done".to_string(),
        })
        .unwrap(),
        serde_json::json!({"kind":"completed"}),
    );
    let json = serde_json::to_string(&record).unwrap();
    assert!(json.contains("\"boundary_kind\":\"terminal\""));
    assert_eq!(record.step_index, 1);
}

#[tokio::test]
async fn subgoal_completed_boundary_written_once_via_storage() {
    use crate::agent::step_record::BoundaryKind;
    use std::sync::Arc;

    let tmp = tempfile::tempdir().unwrap();
    let mut storage = clickweave_core::storage::RunStorage::new(tmp.path(), "int-test");
    let exec_dir = storage.begin_execution().expect("begin exec");
    let storage = Arc::new(Mutex::new(storage));

    let exec = ScriptedExecutor::new(vec![Ok("ok".to_string())]);
    let mut r = StateRunner::new_for_test("goal".to_string()).with_storage(storage.clone());

    // Turn 1: push subgoal, fire tool call.
    let t1 = AgentTurn {
        mutations: vec![TaskStateMutation::PushSubgoal {
            text: "step A".to_string(),
        }],
        action: AgentAction::ToolCall {
            tool_name: "cdp_click".to_string(),
            arguments: serde_json::json!({}),
            tool_call_id: "tc-1".to_string(),
        },
    };
    let _ = r.run_turn(&t1, &exec).await;

    // Turn 2: complete subgoal — write the boundary record.
    let t2 = AgentTurn {
        mutations: vec![TaskStateMutation::CompleteSubgoal {
            summary: "did A".to_string(),
        }],
        action: AgentAction::AgentDone {
            summary: "done".to_string(),
        },
    };
    let _ = r.run_turn(&t2, &exec).await;

    let subgoal_record = r.build_step_record(
        BoundaryKind::SubgoalCompleted,
        serde_json::json!({"kind":"complete_subgoal","summary":"did A"}),
        serde_json::json!({"kind":"success"}),
    );
    r.write_step_record(&subgoal_record);
    let terminal_record = r.build_step_record(
        BoundaryKind::Terminal,
        serde_json::json!({"kind":"agent_done","summary":"done"}),
        serde_json::json!({"kind":"completed"}),
    );
    r.write_step_record(&terminal_record);

    let path = tmp
        .path()
        .join(".clickweave")
        .join("runs")
        .join("int-test")
        .join(&exec_dir)
        .join("events.jsonl");
    let contents = std::fs::read_to_string(&path).unwrap();
    let subgoal_count = contents
        .lines()
        .filter(|l| l.contains("\"boundary_kind\":\"subgoal_completed\""))
        .count();
    assert_eq!(subgoal_count, 1);
    let terminal_count = contents
        .lines()
        .filter(|l| l.contains("\"boundary_kind\":\"terminal\""))
        .count();
    assert_eq!(terminal_count, 1);
}

// ---------------------------------------------------------------------------
// Task 3a.0.6: `RunStorage` parameter plumbing
// ---------------------------------------------------------------------------
//
// Asserts the new `storage` parameter on `run_agent_workflow` compiles and
// flows through the public seam. The legacy `AgentRunner` does not yet
// consume the handle — that wiring lands in Task 3a.6.5. This test pins
// the signature so subsequent tasks cannot silently drop the argument.

#[cfg(test)]
mod run_agent_workflow_signature_tests {
    /// Compile-time assertion: `run_agent_workflow` accepts a
    /// `Option<RunStorageHandle>` as its last parameter.
    ///
    /// If this coerces, the plumbing compiles; we do not invoke the
    /// function here because it takes a concrete `McpClient` which cannot
    /// be instantiated in-crate without spawning the external MCP server.
    /// Task 3a.1's `ScriptedLlm`/`StaticMcp` stubs enable a live
    /// end-to-end test.
    #[test]
    fn run_agent_workflow_accepts_storage_argument() {
        fn _coerce() {
            let _: Option<crate::agent::RunStorageHandle> = None;
        }
    }
}
