// Test modules split out of runner/mod.rs to keep production code navigable.

use super::*;

#[cfg(test)]
mod datetime_oracle_executor_tests {
    use super::*;
    use crate::executor::Mcp;
    use clickweave_mcp::ToolCallResult;
    use serde_json::{Value, json};

    struct PanicMcp;

    impl Mcp for PanicMcp {
        async fn call_tool(
            &self,
            _name: &str,
            _arguments: Option<Value>,
        ) -> anyhow::Result<ToolCallResult> {
            panic!("date/time oracle must be answered by the harness before MCP dispatch");
        }

        fn has_tool(&self, _name: &str) -> bool {
            false
        }

        fn tools_as_openai(&self) -> Vec<Value> {
            Vec::new()
        }

        async fn refresh_server_tool_list(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn get_current_datetime_is_intercepted_before_mcp() {
        let executor = McpToolExecutor { mcp: &PanicMcp };

        let body = executor
            .call_tool(crate::agent::time_oracle::TOOL_NAME, &json!({}))
            .await
            .expect("oracle response");
        let value: Value = serde_json::from_str(&body).expect("oracle JSON");

        assert_eq!(value["kind"], "current_datetime");
        assert_eq!(value["source"], "system_clock");
        assert!(
            value["utc_datetime"]
                .as_str()
                .is_some_and(|s| s.ends_with('Z'))
        );
        assert!(value["unix_millis"].as_i64().is_some());
        assert!(value["timezone"]["offset"].as_str().is_some());
    }
}

#[cfg(test)]
mod builder_tests {
    use super::*;
    use clickweave_llm::{ChatBackend, ChatOptions, ChatResponse, Message};
    use serde_json::Value;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    /// Minimal stub implementing `ChatBackend` so we can confirm the
    /// blanket `DynChatBackend` impl lets us stash one behind `Arc<dyn>`.
    #[derive(Default)]
    struct YesVlmStub;
    impl ChatBackend for YesVlmStub {
        fn model_name(&self) -> &str {
            "yes-vlm"
        }
        async fn chat_with_options(
            &self,
            _messages: &[Message],
            _tools: Option<&[Value]>,
            _options: &ChatOptions,
        ) -> anyhow::Result<ChatResponse> {
            Ok(ChatResponse {
                id: "t".into(),
                choices: vec![clickweave_llm::Choice {
                    index: 0,
                    message: Message::assistant("YES"),
                    finish_reason: Some("stop".into()),
                }],
                usage: None,
            })
        }
    }

    #[test]
    fn with_events_stores_sender() {
        let (tx, _rx) = mpsc::channel::<RunnerOutput>(8);
        let r = StateRunner::new_for_test("g".to_string()).with_events(tx);
        assert!(r.event_tx.is_some());
    }

    #[test]
    fn with_approval_stores_gate() {
        let (tx, _rx) = mpsc::channel::<(ApprovalRequest, oneshot::Sender<bool>)>(8);
        let r = StateRunner::new_for_test("g".to_string()).with_approval(tx);
        assert!(r.approval_gate.is_some());
    }

    #[test]
    fn with_vision_stores_backend_as_arc_dyn() {
        let vlm: Arc<dyn DynChatBackend> = Arc::new(YesVlmStub);
        let r = StateRunner::new_for_test("g".to_string()).with_vision(vlm);
        assert!(r.vision.is_some());
    }

    #[test]
    fn with_permissions_replaces_default_policy() {
        let policy = PermissionPolicy::default();
        let r = StateRunner::new_for_test("g".to_string()).with_permissions(policy);
        // Confirm the field is populated — the default policy is Copy-
        // like and doesn't diverge from the constructor default, so the
        // guarantee here is "no panic, no drop".
        let _ = &r.permissions;
    }

    #[test]
    fn with_verification_artifacts_dir_stores_path() {
        let r = StateRunner::new_for_test("g".to_string())
            .with_verification_artifacts_dir(PathBuf::from("/tmp/artifacts"));
        assert_eq!(
            r.verification_artifacts_dir.as_deref(),
            Some(std::path::Path::new("/tmp/artifacts"))
        );
    }
}

#[cfg(test)]
mod observe_tests {
    use super::*;

    #[test]
    fn observe_applies_pending_events_and_infers_phase() {
        let mut runner = StateRunner::new_for_test("goal".to_string());
        runner.queue_invalidation(InvalidationEvent::FocusChanging {
            tool: "launch_app".to_string(),
        });
        runner.observe();
        assert_eq!(
            runner.task_state.phase,
            crate::agent::phase::Phase::Exploring
        );
    }
}

#[cfg(test)]
mod turn_application_tests {
    use super::*;
    use crate::agent::task_state::TaskStateMutation;

    #[test]
    fn mutations_apply_in_order_before_action() {
        let mut r = StateRunner::new_for_test("g".to_string());
        let turn = AgentTurn {
            mutations: vec![
                TaskStateMutation::PushSubgoal {
                    text: "a".to_string(),
                },
                TaskStateMutation::PushSubgoal {
                    text: "b".to_string(),
                },
            ],
            action: AgentAction::AgentDone {
                summary: "done".to_string(),
            },
        };
        let warnings = r.apply_mutations(&turn.mutations);
        assert!(warnings.is_empty());
        assert_eq!(r.task_state.subgoal_stack.len(), 2);
        assert_eq!(r.task_state.subgoal_stack[1].text, "b");
    }

    #[test]
    fn invalid_mutation_produces_warning_but_others_still_apply() {
        let mut r = StateRunner::new_for_test("g".to_string());
        let muts = vec![
            TaskStateMutation::PushSubgoal {
                text: "a".to_string(),
            },
            TaskStateMutation::RefuteHypothesis { index: 99 },
            TaskStateMutation::PushSubgoal {
                text: "b".to_string(),
            },
        ];
        let warnings = r.apply_mutations(&muts);
        assert_eq!(warnings.len(), 1);
        assert_eq!(r.task_state.subgoal_stack.len(), 2);
    }
}

#[cfg(test)]
mod continuity_tests {
    use super::*;

    #[test]
    fn take_ax_snapshot_success_populates_last_native_ax_snapshot() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.step_index = 5;
        let body = "uid=a1g3 button \"OK\"\n  uid=a2g3 textbox";
        r.update_continuity_after_tool_success("take_ax_snapshot", body);
        let ax = r.world_model.last_native_ax_snapshot.as_ref().unwrap();
        assert_eq!(ax.value.captured_at_step, 5);
        assert!(ax.value.element_count >= 2);
        assert!(ax.value.ax_tree_text.contains("uid=a1g3"));
    }

    #[test]
    fn take_screenshot_success_populates_last_screenshot_ref() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.step_index = 4;
        let body = r#"{"screenshot_id":"ss-abc","width":1440,"height":900}"#;
        r.update_continuity_after_tool_success("take_screenshot", body);
        let s = r.world_model.last_screenshot.as_ref().unwrap();
        assert_eq!(s.value.screenshot_id, "ss-abc");
        assert_eq!(s.value.captured_at_step, 4);
    }

    #[test]
    fn non_snapshot_tool_does_not_touch_continuity() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.update_continuity_after_tool_success("cdp_click", "ok");
        assert!(r.world_model.last_native_ax_snapshot.is_none());
        assert!(r.world_model.last_screenshot.is_none());
    }
}

#[cfg(test)]
mod ax_enrichment_tests {
    use super::*;
    use clickweave_core::{
        AxClickParams, AxSelectParams, AxSetValueParams, AxTarget, McpToolCallParams, NodeType,
    };

    fn runner_with_snapshot(body: &str) -> StateRunner {
        use crate::agent::world_model::{AxSnapshotData, Fresh, FreshnessSource};
        let mut r = StateRunner::new_for_test("g".to_string());
        r.world_model.last_native_ax_snapshot = Some(Fresh {
            value: AxSnapshotData {
                snapshot_id: "a1g1".to_string(),
                element_count: 3,
                captured_at_step: 0,
                ax_tree_text: body.to_string(),
            },
            written_at: 0,
            source: FreshnessSource::DirectObservation,
            ttl_steps: None,
        });
        r
    }

    #[test]
    fn enrich_ax_click_resolved_uid_to_descriptor() {
        let r = runner_with_snapshot("uid=a5g2 AXButton \"Continue\"\n");
        let mut nt = NodeType::AxClick(AxClickParams {
            target: AxTarget::ResolvedUid("a5g2".into()),
            ..Default::default()
        });
        r.enrich_ax_descriptor(&mut nt);
        match nt {
            NodeType::AxClick(p) => assert_eq!(
                p.target,
                AxTarget::Descriptor {
                    role: "AXButton".into(),
                    name: "Continue".into(),
                    parent_name: None,
                }
            ),
            _ => panic!("expected AxClick"),
        }
    }

    #[test]
    fn upgrade_preserves_parent_name_for_outline_rows() {
        // NSOutlineView rows often share (role, name) across sections, so
        // the parent anchor is what makes the descriptor unambiguous.
        let snapshot = concat!(
            "uid=a1g1 AXOutline \"Sidebar\"\n",
            "  uid=a2g1 AXGroup \"Network\"\n",
            "    uid=a3g1 AXRow \"Wi-Fi\"\n",
        );
        let r = runner_with_snapshot(snapshot);
        let mut nt = NodeType::AxSelect(AxSelectParams {
            target: AxTarget::ResolvedUid("a3g1".into()),
            ..Default::default()
        });
        r.enrich_ax_descriptor(&mut nt);
        match nt {
            NodeType::AxSelect(p) => assert_eq!(
                p.target,
                AxTarget::Descriptor {
                    role: "AXRow".into(),
                    name: "Wi-Fi".into(),
                    parent_name: Some("Network".into()),
                }
            ),
            _ => panic!("expected AxSelect"),
        }
    }

    #[test]
    fn enrich_preserves_value_on_ax_set_value() {
        let r = runner_with_snapshot("uid=a10g1 AXTextField \"Email\"\n");
        let mut nt = NodeType::AxSetValue(AxSetValueParams {
            target: AxTarget::ResolvedUid("a10g1".into()),
            value: "preserved".into(),
            ..Default::default()
        });
        r.enrich_ax_descriptor(&mut nt);
        match nt {
            NodeType::AxSetValue(p) => {
                assert_eq!(p.value, "preserved");
                assert_eq!(
                    p.target,
                    AxTarget::Descriptor {
                        role: "AXTextField".into(),
                        name: "Email".into(),
                        parent_name: None,
                    }
                );
            }
            _ => panic!("expected AxSetValue"),
        }
    }

    #[test]
    fn enrich_is_noop_when_uid_not_in_snapshot() {
        let r = runner_with_snapshot("uid=a1g1 AXButton \"Other\"\n");
        let mut nt = NodeType::AxClick(AxClickParams {
            target: AxTarget::ResolvedUid("a99g9".into()),
            ..Default::default()
        });
        r.enrich_ax_descriptor(&mut nt);
        match nt {
            NodeType::AxClick(p) => assert_eq!(p.target, AxTarget::ResolvedUid("a99g9".into())),
            _ => panic!("expected AxClick"),
        }
    }

    #[test]
    fn enrich_is_noop_for_non_ax_nodes() {
        let r = runner_with_snapshot("uid=a1g1 AXButton \"X\"\n");
        let mut nt = NodeType::McpToolCall(McpToolCallParams {
            tool_name: "click".into(),
            arguments: serde_json::json!({}),
        });
        r.enrich_ax_descriptor(&mut nt);
        assert!(matches!(nt, NodeType::McpToolCall(_)));
    }

    #[test]
    fn enrich_is_noop_when_no_snapshot_captured() {
        let r = StateRunner::new_for_test("g".to_string());
        let mut nt = NodeType::AxClick(AxClickParams {
            target: AxTarget::ResolvedUid("a5g2".into()),
            ..Default::default()
        });
        r.enrich_ax_descriptor(&mut nt);
        match nt {
            NodeType::AxClick(p) => assert_eq!(p.target, AxTarget::ResolvedUid("a5g2".into())),
            _ => panic!("expected AxClick"),
        }
    }
}

#[cfg(test)]
mod storage_persistence_tests {
    use super::*;
    use crate::agent::step_record::{BoundaryKind, StepRecord, WorldModelSnapshot};
    use std::sync::{Arc, Mutex};

    fn sample_record(step_index: usize, boundary: BoundaryKind) -> StepRecord {
        StepRecord {
            step_index,
            boundary_kind: boundary,
            world_model_snapshot: WorldModelSnapshot::from_world_model(&WorldModel::default()),
            task_state_snapshot: TaskState::new("goal".to_string()),
            action_taken: serde_json::json!({"kind":"agent_done","summary":"done"}),
            outcome: serde_json::json!({"kind":"completed"}),
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn write_step_record_is_noop_when_no_storage_attached() {
        // No storage means no panic, no events file, nothing persisted.
        let r = StateRunner::new_for_test("g".to_string());
        r.write_step_record(&sample_record(0, BoundaryKind::Terminal));
    }

    #[test]
    fn write_step_record_appends_to_events_jsonl_when_storage_attached() {
        let tmp = tempfile::tempdir().unwrap();
        let mut storage = clickweave_core::storage::RunStorage::new(tmp.path(), "test-workflow");
        let exec_dir = storage.begin_execution().expect("begin_execution");
        let storage = Arc::new(Mutex::new(storage));

        let r = StateRunner::new_for_test("g".to_string()).with_storage(storage.clone());
        r.write_step_record(&sample_record(1, BoundaryKind::SubgoalCompleted));
        r.write_step_record(&sample_record(2, BoundaryKind::Terminal));

        let events_path = tmp
            .path()
            .join(".clickweave")
            .join("runs")
            .join("test-workflow")
            .join(&exec_dir)
            .join("events.jsonl");
        let contents = std::fs::read_to_string(&events_path)
            .unwrap_or_else(|e| panic!("read {:?} failed: {}", events_path, e));
        let subgoal: Vec<_> = contents
            .lines()
            .filter(|l| l.contains("\"boundary_kind\":\"subgoal_completed\""))
            .collect();
        assert_eq!(subgoal.len(), 1);
        let terminal: Vec<_> = contents
            .lines()
            .filter(|l| l.contains("\"boundary_kind\":\"terminal\""))
            .collect();
        assert_eq!(terminal.len(), 1);
    }
}

#[cfg(test)]
mod agent_turn_parsing_tests {
    use super::*;

    #[test]
    fn parses_tool_call_with_no_mutations() {
        let json = r#"{
            "mutations": [],
            "action": {"kind":"tool_call","tool_name":"cdp_click","arguments":{"uid":"d5"},"tool_call_id":"tc-1"}
        }"#;
        let turn: AgentTurn = serde_json::from_str(json).unwrap();
        assert!(turn.mutations.is_empty());
        match turn.action {
            AgentAction::ToolCall { tool_name, .. } => assert_eq!(tool_name, "cdp_click"),
            _ => panic!("expected tool_call"),
        }
    }

    #[test]
    fn parses_agent_done() {
        let json = r#"{
            "mutations": [],
            "action": {"kind":"agent_done","summary":"completed login"}
        }"#;
        let turn: AgentTurn = serde_json::from_str(json).unwrap();
        match turn.action {
            AgentAction::AgentDone { summary } => assert_eq!(summary, "completed login"),
            _ => panic!("expected agent_done"),
        }
    }

    #[test]
    fn parses_mutations_then_action() {
        let json = r#"{
            "mutations": [
                {"kind":"push_subgoal","text":"open login"},
                {"kind":"record_hypothesis","text":"form has 2 fields"}
            ],
            "action": {"kind":"tool_call","tool_name":"cdp_find_elements","arguments":{},"tool_call_id":"tc-2"}
        }"#;
        let turn: AgentTurn = serde_json::from_str(json).unwrap();
        assert_eq!(turn.mutations.len(), 2);
    }

    #[test]
    fn rejects_missing_action() {
        let json = r#"{"mutations": []}"#;
        let result = serde_json::from_str::<AgentTurn>(json);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unknown_mutation_kind() {
        let json = r#"{
            "mutations": [{"kind":"set_phase","phase":"executing"}],
            "action": {"kind":"agent_done","summary":""}
        }"#;
        let result = serde_json::from_str::<AgentTurn>(json);
        assert!(result.is_err(), "set_phase is not a valid mutation (D5)");
    }

    #[test]
    fn rejects_malformed_json() {
        // The design's error-path table says a malformed AgentTurn
        // triggers one repair retry; the parser must surface the error
        // clearly rather than returning a default.
        let json = r#"{"mutations": [], "action":"#; // truncated
        let result = serde_json::from_str::<AgentTurn>(json);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_tool_call_without_tool_name() {
        let json = r#"{
            "mutations": [],
            "action": {"kind":"tool_call","arguments":{},"tool_call_id":"tc-1"}
        }"#;
        let result = serde_json::from_str::<AgentTurn>(json);
        assert!(result.is_err(), "tool_call must require tool_name");
    }

    #[test]
    fn accepts_tool_call_with_empty_arguments_object() {
        // Empty arguments is valid — some tools take no args (e.g. take_ax_snapshot).
        let json = r#"{
            "mutations": [],
            "action": {"kind":"tool_call","tool_name":"take_ax_snapshot","arguments":{},"tool_call_id":"tc-1"}
        }"#;
        let turn: AgentTurn = serde_json::from_str(json).unwrap();
        assert!(matches!(turn.action, AgentAction::ToolCall { .. }));
    }
}

#[cfg(test)]
mod parse_agent_turn_tool_calls_tests {
    //! Tests for the live `parse_agent_turn(&Message)` parser that
    //! consumes OpenAI-shaped `tool_calls`. Distinct from the JSON
    //! envelope tests above, which exercise the `serde::Deserialize`
    //! path for `AgentTurn`.

    use super::*;
    use crate::agent::task_state::WatchSlotName;
    use clickweave_llm::{CallType, FunctionCall, Message, ToolCall};
    use serde_json::json;

    fn tc(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            call_type: CallType::Function,
            function: FunctionCall {
                name: name.to_string(),
                arguments: args,
            },
        }
    }

    #[test]
    fn maps_mcp_tool_call_to_tool_call_action_with_no_mutations() {
        let msg = Message::assistant_tool_calls(vec![tc("tc1", "cdp_click", json!({"uid": "d5"}))]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert!(turn.mutations.is_empty());
        match turn.action {
            AgentAction::ToolCall { tool_name, .. } => assert_eq!(tool_name, "cdp_click"),
            _ => panic!("expected tool_call"),
        }
    }

    #[test]
    fn maps_agent_done_pseudo_tool_to_agent_done_action() {
        let msg = Message::assistant_tool_calls(vec![tc(
            "tc1",
            "agent_done",
            json!({"summary": "logged in"}),
        )]);
        let turn = parse_agent_turn(&msg).unwrap();
        match turn.action {
            AgentAction::AgentDone { summary } => assert_eq!(summary, "logged in"),
            _ => panic!("expected agent_done"),
        }
    }

    #[test]
    fn maps_invoke_skill_pseudo_tool_to_invoke_skill_action() {
        let msg = Message::assistant_tool_calls(vec![tc(
            "tc1",
            "invoke_skill",
            json!({
                "skill_id": "open_settings",
                "version": 2,
                "parameters": {"app": "Notes"}
            }),
        )]);
        let turn = parse_agent_turn(&msg).unwrap();
        match turn.action {
            AgentAction::InvokeSkill {
                skill_id,
                version,
                parameters,
            } => {
                assert_eq!(skill_id, "open_settings");
                assert_eq!(version, 2);
                assert_eq!(parameters, json!({"app": "Notes"}));
            }
            other => panic!("expected invoke_skill, got {:?}", other),
        }
    }

    #[test]
    fn maps_get_current_datetime_to_tool_call_action() {
        let msg = Message::assistant_tool_calls(vec![tc("tc1", "get_current_datetime", json!({}))]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert!(turn.mutations.is_empty());
        match turn.action {
            AgentAction::ToolCall {
                tool_name,
                arguments,
                tool_call_id,
            } => {
                assert_eq!(tool_name, "get_current_datetime");
                assert_eq!(arguments, json!({}));
                assert_eq!(tool_call_id, "tc1");
            }
            other => panic!("expected get_current_datetime tool call, got {:?}", other),
        }
    }

    #[test]
    fn invoke_skill_missing_required_fields_replans() {
        // Missing `version` — the parser cannot fabricate a sensible
        // default, so degrades to a replan instead of dispatching a
        // skill that won't resolve.
        let msg = Message::assistant_tool_calls(vec![tc(
            "tc1",
            "invoke_skill",
            json!({"skill_id": "open_settings"}),
        )]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert!(matches!(turn.action, AgentAction::AgentReplan { .. }));
    }

    #[test]
    fn invoke_skill_version_overflow_replans_instead_of_wrapping() {
        let msg = Message::assistant_tool_calls(vec![tc(
            "tc1",
            "invoke_skill",
            json!({
                "skill_id": "open_settings",
                "version": u64::from(u32::MAX) + 1,
                "parameters": {}
            }),
        )]);
        let turn = parse_agent_turn(&msg).unwrap();
        match turn.action {
            AgentAction::AgentReplan { reason } => {
                assert!(reason.contains("out of range"));
            }
            other => panic!("expected replan for overflow, got {:?}", other),
        }
    }

    #[test]
    fn collects_mutations_then_takes_first_action_call() {
        let msg = Message::assistant_tool_calls(vec![
            tc("m1", "push_subgoal", json!({"text": "open login"})),
            tc(
                "m2",
                "record_hypothesis",
                json!({"text": "form has 2 fields"}),
            ),
            tc("a1", "cdp_find_elements", json!({})),
            // Extra action calls after the first action are dropped.
            tc("a2", "cdp_click", json!({"uid": "d2"})),
        ]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert_eq!(turn.mutations.len(), 2);
        assert!(matches!(
            turn.mutations[0],
            TaskStateMutation::PushSubgoal { .. }
        ));
        assert!(matches!(
            turn.mutations[1],
            TaskStateMutation::RecordHypothesis { .. }
        ));
        match turn.action {
            AgentAction::ToolCall { tool_name, .. } => assert_eq!(tool_name, "cdp_find_elements"),
            _ => panic!("expected first action to win"),
        }
    }

    #[test]
    fn mutations_after_action_are_still_collected() {
        // Apply order is `apply_mutations` -> action; tool-call array
        // ordering is irrelevant. A mutation emitted after the action
        // is still picked up so the parser is robust to LLM sloppiness.
        let msg = Message::assistant_tool_calls(vec![
            tc("a1", "agent_done", json!({"summary": "done"})),
            tc("m1", "push_subgoal", json!({"text": "noted"})),
        ]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert_eq!(turn.mutations.len(), 1);
        assert!(matches!(turn.action, AgentAction::AgentDone { .. }));
    }

    #[test]
    fn only_mutations_synthesizes_agent_replan() {
        // The LLM emitted state mutations but no action — surface as a
        // replan so the next turn re-observes instead of aborting.
        let msg = Message::assistant_tool_calls(vec![tc(
            "m1",
            "push_subgoal",
            json!({"text": "explore"}),
        )]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert_eq!(turn.mutations.len(), 1);
        match turn.action {
            AgentAction::AgentReplan { reason } => {
                assert!(reason.starts_with(NO_ACTION_MUTATION_ONLY_PREFIX));
                assert!(reason.contains("no MCP/environment action ran"));
            }
            other => panic!("expected mutation-only replan, got {:?}", other),
        }
    }

    #[test]
    fn malformed_mutation_is_dropped_without_aborting_turn() {
        // `set_watch_slot` requires both `name` and `note`; a missing
        // field drops just that mutation while letting subsequent
        // mutations and the action through.
        let msg = Message::assistant_tool_calls(vec![
            tc("m_bad", "set_watch_slot", json!({"name": "pending_modal"})),
            tc(
                "m_good",
                "set_watch_slot",
                json!({"name": "pending_auth", "note": "captcha shown"}),
            ),
            tc("a1", "agent_replan", json!({"reason": "auth required"})),
        ]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert_eq!(turn.mutations.len(), 1);
        match &turn.mutations[0] {
            TaskStateMutation::SetWatchSlot { name, .. } => {
                assert_eq!(*name, WatchSlotName::PendingAuth)
            }
            _ => panic!("expected set_watch_slot for pending_auth"),
        }
        assert!(matches!(turn.action, AgentAction::AgentReplan { .. }));
    }

    #[test]
    fn refute_hypothesis_parses_index() {
        let msg = Message::assistant_tool_calls(vec![
            tc("m1", "refute_hypothesis", json!({"index": 3})),
            tc("a1", "agent_replan", json!({"reason": "wrong"})),
        ]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert!(matches!(
            turn.mutations[0],
            TaskStateMutation::RefuteHypothesis { index: 3 }
        ));
    }

    #[test]
    fn unknown_watch_slot_name_drops_mutation() {
        let msg = Message::assistant_tool_calls(vec![
            tc(
                "m1",
                "set_watch_slot",
                json!({"name": "made_up_slot", "note": "x"}),
            ),
            tc("a1", "agent_replan", json!({"reason": "ok"})),
        ]);
        let turn = parse_agent_turn(&msg).unwrap();
        assert!(turn.mutations.is_empty());
    }

    #[test]
    fn empty_tool_calls_array_falls_back_to_text_replan() {
        // `assistant_tool_calls(vec![])` with no content emits a replan
        // with the no-call sentinel reason, mirroring text-only output.
        let msg = Message::assistant_tool_calls(vec![]);
        let turn = parse_agent_turn(&msg).unwrap();
        match turn.action {
            AgentAction::AgentReplan { reason } => {
                assert!(reason.contains("no tool call") || reason.is_empty());
            }
            _ => panic!("expected agent_replan fallback"),
        }
    }
}

#[cfg(test)]
mod unverified_side_effect_guard_tests {
    use super::*;
    use crate::agent::permissions::ToolAnnotations;
    use crate::agent::task_state::TaskStateMutation;
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn detects_obvious_side_effectful_eval_but_allows_read_only_eval() {
        let annotations = HashMap::new();

        assert!(is_unverified_side_effect_action(
            "cdp_evaluate_script",
            &json!({"function": "() => document.querySelector('button')?.click()"}),
            &annotations,
        ));
        assert!(!is_unverified_side_effect_action(
            "cdp_evaluate_script",
            &json!({"function": "() => Array.from(document.querySelectorAll('button')).map((b) => b.textContent)"}),
            &annotations,
        ));
    }

    #[test]
    fn uses_open_world_destructive_annotations_for_future_tools() {
        let mut annotations = HashMap::new();
        annotations.insert(
            "future_action".to_string(),
            ToolAnnotations {
                destructive_hint: Some(true),
                open_world_hint: Some(true),
                ..ToolAnnotations::default()
            },
        );

        assert!(is_unverified_side_effect_action(
            "future_action",
            &json!({}),
            &annotations,
        ));
    }

    #[test]
    fn guard_completion_after_unverified_side_effect_strips_complete_and_blocks_done() {
        let mut turn = AgentTurn {
            mutations: vec![
                TaskStateMutation::PushSubgoal {
                    text: "find target".to_string(),
                },
                TaskStateMutation::CompleteSubgoal {
                    summary: "target open".to_string(),
                },
            ],
            action: AgentAction::AgentDone {
                summary: "done".to_string(),
            },
        };

        assert!(guard_completion_after_unverified_side_effect(
            Some("[UNVERIFIED SIDE EFFECT] previous result"),
            &mut turn,
        ));
        assert_eq!(turn.mutations.len(), 1);
        assert!(matches!(
            turn.mutations[0],
            TaskStateMutation::PushSubgoal { .. }
        ));
        assert!(matches!(turn.action, AgentAction::AgentReplan { .. }));
    }
}

#[cfg(test)]
mod no_progress_guard_tests {
    use super::*;
    use crate::agent::world_model::{CdpPageState, Fresh, FreshnessSource, OcrMatch};
    use clickweave_core::cdp::CdpFindElementMatch;
    use serde_json::json;

    fn sig(
        tool_name: &str,
        arguments: serde_json::Value,
        context: &str,
    ) -> ActionProgressSignature {
        ActionProgressSignature {
            tool_name: tool_name.to_string(),
            arguments,
            context_signature: context.to_string(),
        }
    }

    #[test]
    fn detects_two_action_cycle_in_same_stable_context() {
        let recent = VecDeque::from(vec![
            sig(
                "cdp_fill",
                json!({"uid": "d1", "value": "synthetic"}),
                "ctx",
            ),
            sig("cdp_click", json!({"uid": "d2"}), "ctx"),
            sig(
                "cdp_fill",
                json!({"uid": "d1", "value": "synthetic"}),
                "ctx",
            ),
            sig("cdp_click", json!({"uid": "d2"}), "ctx"),
        ]);

        assert_eq!(
            detect_repeated_action_cycle(&recent),
            Some(vec!["cdp_fill".to_string(), "cdp_click".to_string()])
        );
    }

    #[test]
    fn detects_three_action_cycle_in_same_stable_context() {
        let recent = VecDeque::from(vec![
            sig(
                "cdp_fill",
                json!({"uid": "d-search", "value": "synthetic"}),
                "ctx",
            ),
            sig("cdp_click", json!({"uid": "d-filter"}), "ctx"),
            sig("cdp_click", json!({"uid": "d-cancel"}), "ctx"),
            sig(
                "cdp_fill",
                json!({"uid": "d-search", "value": "synthetic"}),
                "ctx",
            ),
            sig("cdp_click", json!({"uid": "d-filter"}), "ctx"),
            sig("cdp_click", json!({"uid": "d-cancel"}), "ctx"),
        ]);

        assert_eq!(
            detect_repeated_action_cycle(&recent),
            Some(vec![
                "cdp_fill".to_string(),
                "cdp_click".to_string(),
                "cdp_click".to_string(),
            ])
        );
    }

    #[test]
    fn ignores_same_pair_after_context_progress() {
        let recent = VecDeque::from(vec![
            sig(
                "cdp_fill",
                json!({"uid": "d1", "value": "synthetic"}),
                "ctx-a",
            ),
            sig("cdp_click", json!({"uid": "d2"}), "ctx-a"),
            sig(
                "cdp_fill",
                json!({"uid": "d1", "value": "synthetic"}),
                "ctx-b",
            ),
            sig("cdp_click", json!({"uid": "d2"}), "ctx-b"),
        ]);

        assert_eq!(detect_repeated_action_cycle(&recent), None);
    }

    #[test]
    fn stable_context_falls_back_to_page_fingerprint_without_elements() {
        let mut wm = WorldModel::default();
        wm.cdp_page = Some(Fresh {
            value: CdpPageState {
                url: "app://synthetic/page".to_string(),
                page_fingerprint: "count=1;hash=a".to_string(),
                element_inventory: Vec::new(),
            },
            written_at: 1,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        let before = stable_no_progress_context_signature(&wm);

        wm.cdp_page.as_mut().unwrap().value.page_fingerprint = "count=2;hash=b".to_string();
        let after = stable_no_progress_context_signature(&wm);

        assert_ne!(
            before, after,
            "CDP element-surface progress must reset no-progress tracking"
        );
    }

    fn cdp(uid: &str, role: &str, label: &str, tag: &str) -> ObservedElement {
        ObservedElement::Cdp(CdpFindElementMatch {
            uid: uid.to_string(),
            role: role.to_string(),
            label: label.to_string(),
            tag: tag.to_string(),
            disabled: false,
            parent_role: None,
            parent_name: None,
            ..Default::default()
        })
    }

    fn wm_with_cdp_elements(page_fingerprint: &str, elements: Vec<ObservedElement>) -> WorldModel {
        let mut wm = WorldModel::default();
        wm.cdp_page = Some(Fresh {
            value: CdpPageState {
                url: "app://synthetic/page".to_string(),
                page_fingerprint: page_fingerprint.to_string(),
                element_inventory: Vec::new(),
            },
            written_at: 1,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        wm.elements = Some(Fresh {
            value: elements,
            written_at: 1,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        wm
    }

    #[test]
    fn stable_context_ignores_cdp_order_and_uid_churn_when_elements_exist() {
        let before = wm_with_cdp_elements(
            "count=2;hash=uid-a",
            vec![
                cdp("d1", "textbox", "Search synthetic channels", "input"),
                cdp("d2", "button", "Cancel search", "button"),
            ],
        );
        let after = wm_with_cdp_elements(
            "count=2;hash=uid-b",
            vec![
                cdp("d9", "button", "Cancel search", "button"),
                cdp("d8", "textbox", "Search synthetic channels", "input"),
            ],
        );

        assert_eq!(
            stable_no_progress_context_signature(&before),
            stable_no_progress_context_signature(&after),
            "element order, uid churn, and derived page-fingerprint churn must not look like progress"
        );
    }

    #[test]
    fn stable_context_changes_when_semantic_element_surface_changes() {
        let before = wm_with_cdp_elements(
            "count=1;hash=a",
            vec![cdp("d1", "button", "Open synthetic item", "button")],
        );
        let after = wm_with_cdp_elements(
            "count=1;hash=b",
            vec![cdp("d1", "button", "Synthetic item open", "button")],
        );

        assert_ne!(
            stable_no_progress_context_signature(&before),
            stable_no_progress_context_signature(&after),
            "semantic element changes must still reset no-progress tracking"
        );
    }

    #[test]
    fn stable_context_changes_when_cdp_visible_text_changes() {
        let mut before_el = cdp("d1", "button", "Chat with Ljuba Isakovic", "button");
        if let ObservedElement::Cdp(el) = &mut before_el {
            el.visible_text = "Note to Self Tue Photo".to_string();
        }
        let mut after_el = before_el.clone();
        if let ObservedElement::Cdp(el) = &mut after_el {
            el.visible_text = "Note to Self Wed New message".to_string();
        }

        let before = wm_with_cdp_elements("count=1;hash=a", vec![before_el]);
        let after = wm_with_cdp_elements("count=1;hash=b", vec![after_el]);

        assert_ne!(
            stable_no_progress_context_signature(&before),
            stable_no_progress_context_signature(&after),
            "visible text changes must reset no-progress tracking even when the accessibility label is unchanged"
        );
    }

    #[test]
    fn stable_context_ignores_ocr_confidence_jitter() {
        let mut before = WorldModel::default();
        before.elements = Some(Fresh {
            value: vec![ObservedElement::Ocr(OcrMatch {
                text: "Synthetic status".to_string(),
                x: 101,
                y: 202,
                width: 98,
                height: 19,
                confidence: 0.91,
            })],
            written_at: 1,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        let mut after = before.clone();
        if let Some(elements) = after.elements.as_mut()
            && let Some(ObservedElement::Ocr(match_)) = elements.value.first_mut()
        {
            match_.x = 104;
            match_.y = 206;
            match_.confidence = 0.73;
        }

        assert_eq!(
            stable_no_progress_context_signature(&before),
            stable_no_progress_context_signature(&after),
            "small OCR coordinate jitter and confidence changes must not reset the guard"
        );
    }

    #[test]
    fn stale_cdp_uid_errors_are_recognized_and_wrapped() {
        assert!(is_stale_cdp_uid_error(
            "cdp_fill",
            "No node with given id found"
        ));
        assert!(!is_stale_cdp_uid_error(
            "ax_click",
            "No node with given id found"
        ));

        let nudge = build_stale_cdp_uid_nudge("No node with given id found");
        assert!(nudge.starts_with(STALE_CDP_UID_PREFIX));
        assert!(nudge.contains("Rediscover the target"));
        assert!(!nudge.contains("cdp_evaluate_script"));
    }

    #[test]
    fn recovery_nudges_do_not_recommend_eval_script_for_discovery() {
        let repeated = build_no_progress_nudge("cdp_click", 2, "clicked");
        let cycle = build_action_cycle_nudge("cdp_find_elements -> cdp_click", "clicked");
        let post_text = build_post_text_submit_nudge(3, r#"{"matches":[]}"#);

        assert!(repeated.contains("cdp_find_elements"));
        assert!(cycle.contains("cdp_get_element_context"));
        assert!(post_text.contains("cdp_press_key"));
        assert!(!repeated.contains("cdp_evaluate_script"));
        assert!(!cycle.contains("cdp_evaluate_script"));
        assert!(!post_text.contains("cdp_evaluate_script"));
    }

    #[test]
    fn post_text_send_search_helpers_detect_empty_send_searches() {
        assert!(is_send_submit_cdp_search(
            &serde_json::json!({"query":"Send", "role":"button"})
        ));
        assert!(is_send_submit_cdp_search(
            &serde_json::json!({"query":"send button"})
        ));
        assert!(is_send_submit_cdp_search(
            &serde_json::json!({"query":"Submit"})
        ));
        assert!(!is_send_submit_cdp_search(
            &serde_json::json!({"query":"Message", "role":"textbox"})
        ));

        assert_eq!(
            cdp_find_elements_has_matches(r#"{"matches":[],"inventory":[]}"#),
            Some(false)
        );
        assert_eq!(
            cdp_find_elements_has_matches(
                r#"{"matches":[{"uid":"d1","role":"button","label":"Send"}]}"#
            ),
            Some(true)
        );
    }
}

#[cfg(test)]
mod invalidation_wiring_tests {
    //! Direct tests for `queue_invalidations_for_tool_success` and
    //! `queue_snapshot_stale_if_aged` — both fire pending events that
    //! `observe()` drains.

    use super::*;
    use crate::agent::world_model::{
        AxSnapshotData, Fresh, FreshnessSource, InvalidationEvent, ScreenshotRef, SnapshotKind,
    };
    use serde_json::json;

    fn runner() -> StateRunner {
        StateRunner::new_for_test("test goal".to_string())
    }

    #[test]
    fn focus_window_queues_focus_changing() {
        let mut r = runner();
        r.queue_invalidations_for_tool_success("focus_window", &json!({"app_name": "Safari"}));
        assert!(matches!(
            r.pending_events.as_slice(),
            [InvalidationEvent::FocusChanging { tool }] if tool == "focus_window"
        ));
    }

    #[test]
    fn launch_app_queues_focus_and_lifecycle() {
        let mut r = runner();
        r.queue_invalidations_for_tool_success("launch_app", &json!({"app_name": "Mail"}));
        assert_eq!(r.pending_events.len(), 2);
        assert!(matches!(
            r.pending_events[0],
            InvalidationEvent::FocusChanging { .. }
        ));
        assert!(matches!(
            r.pending_events[1],
            InvalidationEvent::AppLifecycle { .. }
        ));
    }

    #[test]
    fn quit_app_queues_focus_and_lifecycle() {
        let mut r = runner();
        r.queue_invalidations_for_tool_success("quit_app", &json!({"app_name": "Mail"}));
        assert_eq!(r.pending_events.len(), 2);
    }

    #[test]
    fn cdp_navigate_queues_navigation_with_url() {
        let mut r = runner();
        r.queue_invalidations_for_tool_success(
            "cdp_navigate",
            &json!({"url": "https://example.com/login"}),
        );
        match r.pending_events.as_slice() {
            [InvalidationEvent::CdpNavigation { new_url }] => {
                assert_eq!(new_url, "https://example.com/login");
            }
            _ => panic!("expected CdpNavigation event"),
        }
    }

    #[test]
    fn cdp_select_page_queues_navigation_even_without_url() {
        let mut r = runner();
        r.queue_invalidations_for_tool_success("cdp_select_page", &json!({"page_index": 1}));
        assert!(matches!(
            r.pending_events.as_slice(),
            [InvalidationEvent::CdpNavigation { new_url }] if new_url.is_empty()
        ));
    }

    #[test]
    fn unrelated_tool_queues_nothing() {
        let mut r = runner();
        r.queue_invalidations_for_tool_success("cdp_click", &json!({"uid": "d1"}));
        assert!(r.pending_events.is_empty());
    }

    #[test]
    fn snapshot_stale_fires_only_for_aged_ax_field() {
        let mut r = runner();
        r.world_model.last_native_ax_snapshot = Some(Fresh {
            value: AxSnapshotData {
                snapshot_id: "ax-0".into(),
                element_count: 0,
                captured_at_step: 0,
                ax_tree_text: String::new(),
            },
            written_at: 0,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        r.step_index = 5; // age = 5, TTL = 2 → should fire.
        r.queue_snapshot_stale_if_aged();
        assert!(matches!(
            r.pending_events.as_slice(),
            [InvalidationEvent::SnapshotStale {
                kind: SnapshotKind::NativeAx,
                age_steps: 5,
            }]
        ));
    }

    #[test]
    fn snapshot_stale_no_op_when_within_ttl() {
        let mut r = runner();
        r.world_model.last_screenshot = Some(Fresh {
            value: ScreenshotRef {
                screenshot_id: "ss-0".into(),
                captured_at_step: 0,
            },
            written_at: 3,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(8),
        });
        r.step_index = 5; // age = 2, TTL = 8 → no event.
        r.queue_snapshot_stale_if_aged();
        assert!(r.pending_events.is_empty());
    }

    #[test]
    fn stale_ax_does_not_invalidate_fresh_screenshot() {
        // The bug being prevented: AX captured at step 0 (TTL 2) and
        // a screenshot captured at step 4 (TTL 4). At step 5, AX is
        // stale (age 5 > TTL 2) but the screenshot is fresh
        // (age 1 < TTL 4). A single `SnapshotStale { age_steps = 5 }`
        // event would have dragged the screenshot down too; the new
        // shape queues per-kind so apply only clears AX.
        let mut r = runner();
        r.world_model.last_native_ax_snapshot = Some(Fresh {
            value: AxSnapshotData {
                snapshot_id: "ax-0".into(),
                element_count: 0,
                captured_at_step: 0,
                ax_tree_text: String::new(),
            },
            written_at: 0,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        r.world_model.last_screenshot = Some(Fresh {
            value: ScreenshotRef {
                screenshot_id: "ss-1".into(),
                captured_at_step: 4,
            },
            written_at: 4,
            source: FreshnessSource::DirectObservation,
            ttl_steps: Some(4),
        });
        r.step_index = 5;
        r.queue_snapshot_stale_if_aged();
        let queued = std::mem::take(&mut r.pending_events);
        r.world_model.apply_events(queued);
        assert!(
            r.world_model.last_native_ax_snapshot.is_none(),
            "stale AX must be cleared"
        );
        assert!(
            r.world_model.last_screenshot.is_some(),
            "fresh screenshot must survive AX going stale"
        );
    }
}

#[cfg(test)]
mod source_agnostic_elements_tests {
    //! `update_continuity_after_tool_success` mirrors AX and OCR
    //! results into the source-agnostic `world_model.elements` field
    //! so the renderer can print them uniformly.

    use super::*;
    use crate::agent::world_model::ObservedElement;

    fn runner() -> StateRunner {
        StateRunner::new_for_test("test goal".to_string())
    }

    #[test]
    fn take_ax_snapshot_populates_elements_with_ax_variants() {
        let mut r = runner();
        let body = "uid=a1g3 button \"Login\"\n  uid=a2g3 textbox \"Email\"\n";
        r.update_continuity_after_tool_success("take_ax_snapshot", body);
        let els = r.world_model.elements.as_ref().expect("elements populated");
        assert!(!els.value.is_empty(), "expected parsed AX elements");
        assert!(
            els.value
                .iter()
                .all(|e| matches!(e, ObservedElement::Ax(_))),
            "all elements must be Ax-variant"
        );
    }

    #[test]
    fn take_ax_snapshot_with_empty_body_does_not_overwrite_elements() {
        let mut r = runner();
        // Pre-populate a CDP elements surface; an empty AX snapshot
        // should not clobber it (no `Ax` elements parsed).
        let cdp_match = clickweave_core::cdp::CdpFindElementMatch {
            uid: "d1".into(),
            role: "button".into(),
            label: "OK".into(),
            tag: "button".into(),
            disabled: false,
            parent_role: None,
            parent_name: None,
            ..Default::default()
        };
        r.world_model.elements = Some(crate::agent::world_model::Fresh {
            value: vec![ObservedElement::Cdp(cdp_match.clone())],
            written_at: 0,
            source: crate::agent::world_model::FreshnessSource::DirectObservation,
            ttl_steps: Some(2),
        });
        r.update_continuity_after_tool_success("take_ax_snapshot", "");
        let els = r.world_model.elements.as_ref().unwrap();
        assert!(matches!(els.value.first(), Some(ObservedElement::Cdp(_))));
    }
}

#[cfg(test)]
mod resolve_cdp_target_tests {
    //! Ported verbatim from the legacy `resolve_cdp_target_tests`
    //! for Task 3a.7.d. The legacy tests targeted
    //! `AgentRunner::<B>::resolve_cdp_target`; here they call
    //! `StateRunner::resolve_cdp_target` directly (no backend type
    //! parameter on the new runner's associated fn).
    use super::*;
    use crate::executor::Mcp;
    use clickweave_mcp::ToolCallResult;

    /// MCP stub that panics on any call. Every test in this module
    /// exercises paths (structured response, arguments-only) that must
    /// not reach MCP — the panic proves those paths don't regress to
    /// making extra round-trips.
    struct UnusedMcp;

    impl Mcp for UnusedMcp {
        async fn call_tool(
            &self,
            _name: &str,
            _arguments: Option<Value>,
        ) -> anyhow::Result<ToolCallResult> {
            panic!("resolve_cdp_target reached MCP on a fast-path case");
        }
        fn has_tool(&self, _name: &str) -> bool {
            false
        }
        fn tools_as_openai(&self) -> Vec<Value> {
            Vec::new()
        }
        async fn refresh_server_tool_list(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    async fn resolve(arguments: Value, result_text: &str) -> Option<(String, Option<String>)> {
        StateRunner::resolve_cdp_target(&arguments, result_text, &UnusedMcp).await
    }

    #[tokio::test]
    async fn structured_response_wins_over_pid_argument() {
        let arguments = serde_json::json!({ "pid": 16024 });
        let result_text = serde_json::json!({
            "app_name": "Signal",
            "pid": 16024,
            "bundle_id": "org.whispersystems.signal-desktop",
            "kind": "ElectronApp",
        })
        .to_string();
        let resolved = resolve(arguments, &result_text).await;
        assert_eq!(
            resolved,
            Some(("Signal".to_string(), Some("ElectronApp".to_string())))
        );
    }

    #[tokio::test]
    async fn plain_text_response_falls_back_to_arguments_app_name() {
        let arguments = serde_json::json!({ "app_name": "Signal" });
        let resolved = resolve(arguments, "Window focused successfully").await;
        assert_eq!(resolved, Some(("Signal".to_string(), None)));
    }

    #[tokio::test]
    async fn empty_app_name_in_structured_response_is_ignored() {
        let arguments = serde_json::json!({ "app_name": "Chrome" });
        let result_text = serde_json::json!({ "app_name": "", "pid": 0 }).to_string();
        let resolved = resolve(arguments, &result_text).await;
        assert_eq!(resolved, Some(("Chrome".to_string(), None)));
    }

    /// MCP stub that returns a fixed multi-text-block `list_apps` response.
    /// Pins the contract that the `pid → list_apps` CDP resolution path
    /// parses only the first text block: regression guard for a past bug
    /// where joining blocks with `\n` broke serde_json parsing whenever a
    /// server returned a JSON payload plus trailing prose.
    struct MultiBlockListAppsMcp;

    impl Mcp for MultiBlockListAppsMcp {
        async fn call_tool(
            &self,
            name: &str,
            _arguments: Option<Value>,
        ) -> anyhow::Result<ToolCallResult> {
            assert_eq!(name, "list_apps");
            Ok(ToolCallResult {
                content: vec![
                    clickweave_mcp::ToolContent::Text {
                        text: r#"[{"name":"Signal","pid":16024}]"#.to_string(),
                    },
                    clickweave_mcp::ToolContent::Text {
                        text: "(rendered from cached process table)".to_string(),
                    },
                ],
                is_error: None,
            })
        }
        fn has_tool(&self, name: &str) -> bool {
            name == "list_apps"
        }
        fn tools_as_openai(&self) -> Vec<Value> {
            Vec::new()
        }
        async fn refresh_server_tool_list(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn pid_resolves_to_app_name_even_with_trailing_prose_block() {
        let arguments = serde_json::json!({ "pid": 16024 });
        let resolved = StateRunner::resolve_cdp_target(
            &arguments,
            "Window focused successfully",
            &MultiBlockListAppsMcp,
        )
        .await;
        assert_eq!(resolved, Some(("Signal".to_string(), None)));
    }
}

#[cfg(test)]
mod focus_skip_tests {
    //! Ported verbatim from the focus_window skip guard section of the
    //! legacy runner's observation-union tests for Task 3a.7.d. Exercises
    //! `StateRunner::should_skip_focus_window` and its two sister
    //! predicates (`is_synthetic_focus_skip`, `mcp_has_toolset`) against
    //! the same matrix of kind / toolset / CDP-liveness / policy cases
    //! the legacy `AgentRunner` suite pinned.
    use super::*;
    use clickweave_mcp::ToolCallResult;

    /// Minimal `Mcp` stub used to exercise the focus_window skip guard.
    /// Only `has_tool` is consulted by
    /// [`StateRunner::should_skip_focus_window`] — `call_tool` /
    /// `tools_as_openai` / `refresh_server_tool_list` are never reached
    /// in these unit tests but must exist to satisfy the trait bound.
    struct ToolsetStub {
        tools: Vec<String>,
    }

    impl ToolsetStub {
        fn with(tools: &[&str]) -> Self {
            Self {
                tools: tools.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl crate::executor::Mcp for ToolsetStub {
        async fn call_tool(
            &self,
            _name: &str,
            _arguments: Option<Value>,
        ) -> anyhow::Result<ToolCallResult> {
            unimplemented!("focus_window skip guard does not dispatch tools")
        }

        fn has_tool(&self, name: &str) -> bool {
            self.tools.iter().any(|t| t == name)
        }

        fn tools_as_openai(&self) -> Vec<Value> {
            Vec::new()
        }

        async fn refresh_server_tool_list(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Fresh runner pre-seeded with one app/kind hint for guard tests.
    fn runner_with_kind(app_name: &str, kind: &str) -> StateRunner {
        let mut runner = StateRunner::new_for_test("test-goal".to_string());
        runner.record_app_kind(app_name, kind);
        runner
    }

    const FULL_AX_TOOLSET: &[&str] = &["take_ax_snapshot", "ax_click", "ax_set_value", "ax_select"];

    #[test]
    fn mcp_has_toolset_requires_every_member() {
        // Missing even one member blocks the guard. The guard only fires
        // when the full macOS AX dispatch toolset is present; on Windows
        // and on older MCP servers the set is incomplete and
        // focus_window still matters.
        let mcp_full = ToolsetStub::with(FULL_AX_TOOLSET);
        assert!(mcp_has_toolset(&mcp_full, FULL_AX_TOOLSET));

        for (i, missing) in FULL_AX_TOOLSET.iter().enumerate() {
            let partial: Vec<&str> = FULL_AX_TOOLSET
                .iter()
                .enumerate()
                .filter_map(|(j, t)| (j != i).then_some(*t))
                .collect();
            let mcp = ToolsetStub::with(&partial);
            assert!(
                !mcp_has_toolset(&mcp, FULL_AX_TOOLSET),
                "toolset without {} must not count as full AX toolset",
                missing,
            );
        }
    }

    #[test]
    fn should_skip_focus_window_fires_for_known_native_with_full_ax_toolset() {
        // Baseline happy path: MCP exposes the full AX toolset AND we've
        // already seen that the target is Native — suppress focus_window
        // to keep the user's foreground undisturbed.
        let runner = runner_with_kind("Calculator", "Native");
        let mcp = ToolsetStub::with(FULL_AX_TOOLSET);
        let args = serde_json::json!({"app_name": "Calculator"});
        assert_eq!(
            runner.should_skip_focus_window(&args, &mcp),
            Some(FocusSkipReason::AxAvailable),
        );
    }

    #[test]
    fn should_skip_focus_window_defers_for_electron_or_chrome_without_live_cdp() {
        // Broader contract (see `should_skip_focus_window`): Electron /
        // Chrome apps DO qualify for the skip, but only after CDP is
        // live for that exact app. When no CDP session is bound yet,
        // the first `focus_window` call often precedes `cdp_connect`
        // and may be needed to bring the window front so the debug
        // port is discoverable. Without CDP live, the guard must defer
        // regardless of which dispatch toolset the MCP server exposes.
        //
        // NOTE: this test previously asserted that Electron / Chrome
        // apps were NEVER skipped. That narrower contract was relaxed
        // when CDP dispatch became the dominant path for these apps.
        // The test now covers the pre-CDP-connect half of the broader
        // contract; the post-CDP-connect half is covered by
        // `should_skip_focus_window_fires_for_electron_with_live_cdp`.
        // AX + CDP toolsets both present — the only thing missing is
        // the live CDP session, which is the point.
        let mcp = ToolsetStub::with(&[
            "take_ax_snapshot",
            "ax_click",
            "ax_set_value",
            "ax_select",
            "cdp_find_elements",
            "cdp_click",
        ]);
        for kind in ["ElectronApp", "ChromeBrowser"] {
            let runner = runner_with_kind("VSCode", kind);
            let args = serde_json::json!({"app_name": "VSCode"});
            assert!(
                runner.should_skip_focus_window(&args, &mcp).is_none(),
                "focus_window must NOT be skipped for kind={} without a live CDP session",
                kind,
            );
        }
    }

    /// Seed a runner with a kind hint AND an active CDP session bound
    /// to the same app — the on-the-wire state the agent reaches after
    /// `launch_app` + successful `cdp_connect`. Delegates to
    /// [`StateRunner::seed_cdp_live_for_test`] so the "post-`on_cdp_connected`
    /// state shape" has a single source of truth.
    fn runner_with_kind_and_cdp(app_name: &str, kind: &str) -> StateRunner {
        let mut runner = StateRunner::new_for_test("test-goal".to_string());
        runner.seed_cdp_live_for_test(app_name, kind);
        runner
    }

    const FULL_CDP_TOOLSET: &[&str] = &["cdp_find_elements", "cdp_click"];

    #[test]
    fn should_skip_focus_window_fires_for_electron_with_live_cdp() {
        // CDP dispatch operates on backgrounded windows without stealing
        // focus, so once a session is live for the exact app, the real
        // `focus_window` is redundant and the guard must fire.
        let runner = runner_with_kind_and_cdp("Signal", "ElectronApp");
        let mcp = ToolsetStub::with(FULL_CDP_TOOLSET);
        let args = serde_json::json!({"app_name": "Signal"});
        assert_eq!(
            runner.should_skip_focus_window(&args, &mcp),
            Some(FocusSkipReason::CdpLive),
        );
    }

    #[test]
    fn should_skip_focus_window_fires_for_chrome_browser_with_live_cdp() {
        // Same contract as the Electron path — ChromeBrowser targets
        // go through CDP and must be suppressed when a session is live.
        let runner = runner_with_kind_and_cdp("Google Chrome", "ChromeBrowser");
        let mcp = ToolsetStub::with(FULL_CDP_TOOLSET);
        let args = serde_json::json!({"app_name": "Google Chrome"});
        assert_eq!(
            runner.should_skip_focus_window(&args, &mcp),
            Some(FocusSkipReason::CdpLive),
        );
    }

    #[test]
    fn should_skip_focus_window_defers_for_electron_when_cdp_not_connected() {
        // Kind hint + full CDP toolset but NO live session — the first
        // focus_window often precedes cdp_connect and may itself be
        // what brings the window front so the debug port is findable.
        // The guard must defer here.
        let runner = runner_with_kind("Signal", "ElectronApp");
        let mcp = ToolsetStub::with(FULL_CDP_TOOLSET);
        let args = serde_json::json!({"app_name": "Signal"});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn should_skip_focus_window_defers_for_electron_when_cdp_tools_missing() {
        // CDP is live but the MCP server does not advertise the CDP
        // dispatch toolset (older server, stripped build). Without
        // cdp_find_elements / cdp_click the agent cannot drive the
        // target via CDP, so coordinate-based tools — which DO need
        // focus — are the likely fallback. The guard must defer.
        let runner = runner_with_kind_and_cdp("Signal", "ElectronApp");
        // Only cdp_find_elements, missing cdp_click.
        let mcp = ToolsetStub::with(&["cdp_find_elements"]);
        let args = serde_json::json!({"app_name": "Signal"});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn should_skip_focus_window_defers_when_cdp_bound_to_other_app() {
        // A live CDP session bound to a different app must not authorize
        // a skip for this one — the name scope of `is_connected_to` is
        // load-bearing.
        let mut runner = StateRunner::new_for_test("test-goal".to_string());
        runner.record_app_kind("Signal", "ElectronApp");
        runner.cdp_state.set_connected("Slack", 0);
        let mcp = ToolsetStub::with(FULL_CDP_TOOLSET);
        let args = serde_json::json!({"app_name": "Signal"});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn should_skip_focus_window_defers_when_kind_unknown() {
        // First-ever focus: no prior probe / structured response, so we
        // can't classify the app. The task is explicit about erring on
        // the side of executing focus_window normally in this case —
        // breaking Electron / Windows workflows is strictly worse than
        // a single preserved focus-steal on the first call.
        let runner = StateRunner::new_for_test("test-goal".to_string());
        let mcp = ToolsetStub::with(FULL_AX_TOOLSET);
        let args = serde_json::json!({"app_name": "MysteryApp"});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn should_skip_focus_window_defers_when_ax_toolset_incomplete() {
        // Windows / older MCP servers surface only a partial toolset.
        // Without ax_click / ax_set_value / ax_select, the agent cannot
        // drive the target via AX and `focus_window` is still required.
        let runner = runner_with_kind("Calculator", "Native");
        // Only take_ax_snapshot — no dispatch primitives.
        let mcp = ToolsetStub::with(&["take_ax_snapshot"]);
        let args = serde_json::json!({"app_name": "Calculator"});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn should_skip_focus_window_requires_app_name_in_args() {
        // window_id / pid-only focus_window variants are ambiguous; we
        // can't map them to a recorded kind, so the guard must not
        // fire. resolve_cdp_target's list_apps / list_windows path
        // still runs the real tool, which is the correct behavior.
        let runner = runner_with_kind("Calculator", "Native");
        let mcp = ToolsetStub::with(FULL_AX_TOOLSET);
        let args = serde_json::json!({"window_id": 42});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn is_synthetic_focus_skip_matches_only_the_sentinels() {
        // Post-step bookkeeping gates CDP auto-connect and workflow-node
        // creation on this predicate — it must be tight enough that a
        // real focus_window success never masquerades as a skip, yet
        // match every FocusSkipReason variant so none of the runner's
        // suppressions leak into the workflow graph.
        for reason in FocusSkipReason::ALL {
            assert!(
                StateRunner::is_synthetic_focus_skip("focus_window", reason.llm_message()),
                "focus_window + {:?} message must register as synthetic skip",
                reason,
            );
            assert!(
                !StateRunner::is_synthetic_focus_skip("launch_app", reason.llm_message()),
                "non-focus_window tool with {:?} message must not register",
                reason,
            );
        }
        // Different result text — a real MCP success must not be
        // treated as skipped.
        assert!(!StateRunner::is_synthetic_focus_skip(
            "focus_window",
            "Window focused successfully",
        ));
    }

    #[test]
    fn should_skip_focus_window_respects_allow_focus_window_policy() {
        // Policy takes precedence over every kind / toolset branch: when
        // `allow_focus_window == false`, the predicate must return the
        // policy sentinel even for cases that would otherwise defer
        // (unknown kind, missing toolset, missing app_name, CDP-not-live).
        // The returned skip text is the LLM-facing nudge toward AX / CDP
        // dispatch primitives.
        let mut runner = StateRunner::new(
            "test-goal".to_string(),
            AgentConfig {
                allow_focus_window: false,
                ..Default::default()
            },
        );
        let mcp_empty = ToolsetStub::with(&[]);

        // 1. Unknown app kind, empty toolset — would normally defer.
        let args_named = serde_json::json!({"app_name": "MysteryApp"});
        assert_eq!(
            runner.should_skip_focus_window(&args_named, &mcp_empty),
            Some(FocusSkipReason::PolicyDisabled),
        );

        // 2. Missing app_name (window_id / pid-only form) — the kind /
        // toolset branches always defer here, but policy overrides.
        let args_windowed = serde_json::json!({"window_id": 42});
        assert_eq!(
            runner.should_skip_focus_window(&args_windowed, &mcp_empty),
            Some(FocusSkipReason::PolicyDisabled),
        );

        // 3. Electron kind hint but no live CDP session — normally
        // defers because the first focus_window often precedes
        // cdp_connect. Policy overrides.
        runner.record_app_kind("Signal", "ElectronApp");
        let args_electron = serde_json::json!({"app_name": "Signal"});
        assert_eq!(
            runner.should_skip_focus_window(&args_electron, &mcp_empty),
            Some(FocusSkipReason::PolicyDisabled),
        );

        // 4. `new_for_test` opts allow_focus_window back in so the
        //    unit tests in this module exercise the kind/toolset
        //    branches without per-test opt-in; an unseeded fixture
        //    runner must defer on unknown kind.
        let test_default_runner = StateRunner::new_for_test("test-goal".to_string());
        assert!(
            test_default_runner
                .should_skip_focus_window(&args_named, &mcp_empty)
                .is_none(),
        );
    }

    #[test]
    fn default_config_disables_focus_window_via_policy() {
        // Pins the production-default contract: `AgentConfig::default()`
        // must suppress every focus_window unconditionally. `new_for_test`
        // overrides this for the rest of the suite (see above).
        let runner = StateRunner::new("test-goal".to_string(), AgentConfig::default());
        let mcp = ToolsetStub::with(&[]);
        let args = serde_json::json!({"app_name": "AnyApp"});
        assert_eq!(
            runner.should_skip_focus_window(&args, &mcp),
            Some(FocusSkipReason::PolicyDisabled),
            "AgentConfig::default() must suppress focus_window unconditionally",
        );
    }

    #[test]
    fn record_app_kind_overwrites_previous_value_for_same_app() {
        // Apps can transition between kinds across runs (e.g. a Chrome
        // profile that used to be launched plain and is now launched
        // with --remote-debugging-port). The latest hint must win so
        // the guard reflects the current lifecycle, not history.
        let mut runner = StateRunner::new_for_test("test-goal".to_string());
        runner.record_app_kind("Calculator", "Native");
        runner.record_app_kind("Calculator", "ElectronApp");
        let mcp = ToolsetStub::with(FULL_AX_TOOLSET);
        let args = serde_json::json!({"app_name": "Calculator"});
        // Electron now — guard must NOT fire.
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn should_skip_focus_window_fires_cdp_attachable_for_electron_pre_connect() {
        // Pre-CDP-connect contract: kind is Electron / Chrome and the
        // server advertises `cdp_connect`. The post-tool hook will
        // auto-connect on its own — the real focus_window is
        // unnecessary and would only steal foreground in the meantime.
        for kind in ["ElectronApp", "ChromeBrowser"] {
            let runner = runner_with_kind("VSCode", kind);
            let mcp = ToolsetStub::with(&["cdp_connect"]);
            let args = serde_json::json!({"app_name": "VSCode"});
            assert_eq!(
                runner.should_skip_focus_window(&args, &mcp),
                Some(FocusSkipReason::CdpAttachable),
                "kind={kind} with cdp_connect advertised must trigger CdpAttachable",
            );
        }
    }

    #[test]
    fn should_skip_focus_window_defers_for_electron_when_cdp_connect_missing() {
        // CDP-attachable arm requires the server to actually advertise
        // `cdp_connect`. Without it the post-tool hook cannot fire, so
        // the first focus_window may itself be needed to bring the
        // window front and the classifier must defer.
        let runner = runner_with_kind("VSCode", "ElectronApp");
        // FULL_CDP_TOOLSET does NOT include cdp_connect by design —
        // it is the dispatch toolset, not the lifecycle one.
        let mcp = ToolsetStub::with(FULL_CDP_TOOLSET);
        let args = serde_json::json!({"app_name": "VSCode"});
        assert!(runner.should_skip_focus_window(&args, &mcp).is_none());
    }

    #[test]
    fn cdp_live_takes_precedence_over_cdp_attachable_for_same_app() {
        // When the session is live AND the server advertises
        // `cdp_connect`, the more specific `CdpLive` arm must fire —
        // the agent has the dispatch toolset, not just the connect
        // primitive. Order matters in the match: CdpLive first.
        let runner = runner_with_kind_and_cdp("Signal", "ElectronApp");
        // Both CDP dispatch AND cdp_connect advertised.
        let mcp = ToolsetStub::with(&["cdp_find_elements", "cdp_click", "cdp_connect"]);
        let args = serde_json::json!({"app_name": "Signal"});
        assert_eq!(
            runner.should_skip_focus_window(&args, &mcp),
            Some(FocusSkipReason::CdpLive),
        );
    }
}

/// Coordinate-primitive guard: defense-in-depth check that a wrong-family
/// dispatch (`click` / `type_text` / `press_key` / `move_mouse` / `scroll`
/// / `drag`) is rejected at the harness layer when a structured surface
/// (`cdp_page` for CDP-backed apps, `take_ax_snapshot` + AX dispatch for
/// Native) is wired for the focused app. Sits behind the per-turn
/// `<tools_in_scope>` filter — these tests pin the predicate alone; the
/// dispatch-site behaviour (synthetic StepOutcome::Error, StepFailed
/// event, recovery_strategy interaction) is covered by the integration
/// suite.
#[cfg(test)]
mod coordinate_primitive_guard_tests {
    use super::*;
    use crate::agent::world_model::{AppKind, CdpPageState, FocusedApp, Fresh, FreshnessSource};
    use clickweave_mcp::ToolCallResult;

    struct ToolsetStub {
        tools: Vec<String>,
    }

    impl ToolsetStub {
        fn with(tools: &[&str]) -> Self {
            Self {
                tools: tools.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl crate::executor::Mcp for ToolsetStub {
        async fn call_tool(
            &self,
            _name: &str,
            _arguments: Option<Value>,
        ) -> anyhow::Result<ToolCallResult> {
            unimplemented!("coordinate guard predicate does not dispatch tools")
        }
        fn has_tool(&self, name: &str) -> bool {
            self.tools.iter().any(|t| t == name)
        }
        fn tools_as_openai(&self) -> Vec<Value> {
            Vec::new()
        }
        async fn refresh_server_tool_list(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    const AX_TOOLSET: &[&str] = &["take_ax_snapshot", "ax_click", "ax_set_value", "ax_select"];

    fn focused(name: &str, kind: AppKind) -> Fresh<FocusedApp> {
        Fresh {
            value: FocusedApp {
                name: name.to_string(),
                kind,
                pid: 1,
            },
            written_at: 0,
            source: FreshnessSource::DirectObservation,
            ttl_steps: None,
        }
    }

    fn cdp_page(url: &str) -> Fresh<CdpPageState> {
        Fresh {
            value: CdpPageState {
                url: url.to_string(),
                page_fingerprint: "fp".to_string(),
                element_inventory: Vec::new(),
            },
            written_at: 0,
            source: FreshnessSource::DirectObservation,
            ttl_steps: None,
        }
    }

    #[test]
    fn blocks_click_when_cdp_page_live_and_focus_is_electron() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.world_model.focused_app = Some(focused("Signal", AppKind::ElectronApp));
        runner.world_model.cdp_page = Some(cdp_page("https://signal/"));
        let mcp = ToolsetStub::with(&[]);
        let blocked = runner.coordinate_primitive_blocked("click", &mcp);
        assert!(blocked.is_some(), "click must be blocked under live CDP");
        let msg = blocked.unwrap();
        assert!(msg.contains("cdp_page"));
        assert!(msg.contains("cdp_click"));
        assert!(!msg.contains("cdp_evaluate_script"));
    }

    #[test]
    fn blocks_each_coordinate_primitive_under_cdp() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.world_model.focused_app = Some(focused("Signal", AppKind::ElectronApp));
        runner.world_model.cdp_page = Some(cdp_page("https://signal/"));
        let mcp = ToolsetStub::with(&[]);
        for tool in [
            "click",
            "type_text",
            "press_key",
            "move_mouse",
            "scroll",
            "drag",
        ] {
            assert!(
                runner.coordinate_primitive_blocked(tool, &mcp).is_some(),
                "{tool} must be blocked when CDP is wired",
            );
        }
    }

    #[test]
    fn does_not_block_observation_or_structured_tools_under_cdp() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.world_model.focused_app = Some(focused("Signal", AppKind::ElectronApp));
        runner.world_model.cdp_page = Some(cdp_page("https://signal/"));
        let mcp = ToolsetStub::with(&[]);
        for tool in [
            "find_text",
            "find_image",
            "element_at_point",
            "cdp_click",
            "ax_click",
            "take_screenshot",
        ] {
            assert!(
                runner.coordinate_primitive_blocked(tool, &mcp).is_none(),
                "{tool} must NOT be blocked — only coordinate primitives are",
            );
        }
    }

    #[test]
    fn blocks_click_when_focus_is_native_and_ax_dispatch_wired() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.world_model.focused_app = Some(focused("Calculator", AppKind::Native));
        let mcp = ToolsetStub::with(AX_TOOLSET);
        let blocked = runner.coordinate_primitive_blocked("click", &mcp);
        assert!(blocked.is_some(), "click must be blocked under AX dispatch");
        let msg = blocked.unwrap();
        assert!(msg.contains("Native"));
        assert!(msg.contains("ax_click"));
    }

    #[test]
    fn defers_when_focus_is_native_but_ax_toolset_partial() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.world_model.focused_app = Some(focused("Calculator", AppKind::Native));
        // Missing ax_set_value — partial toolset means agent cannot
        // drive via AX, so coordinate primitives remain a valid path.
        let mcp = ToolsetStub::with(&["take_ax_snapshot", "ax_click"]);
        assert!(runner.coordinate_primitive_blocked("click", &mcp).is_none());
    }

    #[test]
    fn defers_when_no_focused_app() {
        let runner = StateRunner::new_for_test("g".to_string());
        // No focused_app set — caller has not yet observed which surface
        // is wired, so we cannot tell which family the agent should be
        // using and must fall through.
        let mcp = ToolsetStub::with(AX_TOOLSET);
        assert!(runner.coordinate_primitive_blocked("click", &mcp).is_none());
    }

    #[test]
    fn defers_for_electron_focus_without_cdp_page() {
        // Electron is focused but no cdp_page yet (auto-connect hasn't
        // attached). Coordinate primitives are not yet redundant — the
        // agent may need them to bring the window front. Guard defers.
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.world_model.focused_app = Some(focused("Signal", AppKind::ElectronApp));
        let mcp = ToolsetStub::with(&["cdp_connect"]);
        assert!(runner.coordinate_primitive_blocked("click", &mcp).is_none());
    }

    #[test]
    fn is_coordinate_primitive_includes_actions_excludes_observations() {
        for name in [
            "click",
            "type_text",
            "press_key",
            "move_mouse",
            "scroll",
            "drag",
        ] {
            assert!(is_coordinate_primitive(name), "{name} is a coord primitive");
        }
        for name in [
            "find_text",
            "find_image",
            "element_at_point",
            "take_screenshot",
            "ax_click",
            "cdp_click",
            "launch_app",
        ] {
            assert!(
                !is_coordinate_primitive(name),
                "{name} must NOT be classified as a coordinate primitive",
            );
        }
    }
}

/// CDP auto-connect status field (`world_model.cdp_connect_status`).
/// The runner sets this whenever `auto_connect_cdp` exhausts retries
/// and clears it on success or focus change. Without the field, the
/// LLM cannot tell "auto-connect hasn't fired yet" (no cdp_page, no
/// status) from "auto-connect tried and failed permanently" (no
/// cdp_page, status present).
#[cfg(test)]
mod cdp_connect_status_tests {
    use super::*;

    #[test]
    fn record_cdp_connect_failure_writes_fresh_status() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        assert!(runner.world_model.cdp_connect_status.is_none());
        runner.record_cdp_connect_failure("probe_app failed for X: y".to_string());
        let status = runner
            .world_model
            .cdp_connect_status
            .as_ref()
            .expect("status set");
        assert_eq!(status.value, "probe_app failed for X: y");
        assert_eq!(status.written_at, runner.step_index);
    }

    #[test]
    fn second_failure_overwrites_first() {
        let mut runner = StateRunner::new_for_test("g".to_string());
        runner.record_cdp_connect_failure("first".to_string());
        runner.record_cdp_connect_failure("second".to_string());
        assert_eq!(
            runner
                .world_model
                .cdp_connect_status
                .as_ref()
                .unwrap()
                .value,
            "second",
        );
    }
}

/// D24/D29 run-start retrieval gate + step_index ownership tests.
/// The gate (`episodic_run_start_retrieved`) replaces the drift-prone
/// `step_index == 0` proxy; the helper (`advance_recorded_step_index`)
/// is the single owner of `step_index` updates so the counter matches
/// `state.steps.len()` across all recording paths (synthetic skip,
/// policy deny, approval reject, normal LLM turn).
#[cfg(test)]
mod retrieval_gate_tests {
    use super::*;
    use crate::agent::episodic::{EpisodeScope, EpisodicContext, SqliteEpisodicStore};
    use crate::agent::phase::Phase;
    use tempfile::TempDir;

    fn enabled_runner_with_store() -> (StateRunner, TempDir) {
        let dir = TempDir::new().unwrap();
        let wl_path = dir.path().join("episodic.sqlite");
        let ctx = EpisodicContext {
            enabled: true,
            workflow_local_path: wl_path.clone(),
            global_path: None,
            workflow_hash: "gate-test-workflow".into(),
        };
        let runner =
            StateRunner::new_with_episodic("goal".to_string(), AgentConfig::default(), ctx);
        // Sanity: store opened.
        assert!(
            runner.episodic_store.is_some(),
            "test setup expects an episodic store",
        );
        // The `wl_path` is referenced indirectly through the runner's
        // store; pre-open one to confirm SQLite WAL mode took.
        let _verify = SqliteEpisodicStore::new(&wl_path, EpisodeScope::WorkflowLocal).unwrap();
        (runner, dir)
    }

    #[tokio::test]
    async fn run_start_retrieval_consumes_gate_on_first_call() {
        let (mut r, _dir) = enabled_runner_with_store();
        assert!(!r.episodic_run_start_retrieved);

        // First call: run-start trigger fires (zero hits, but the
        // gate-consumed semantic still applies).
        let hits = r.try_retrieve_episodic(Phase::Exploring).await;
        assert!(
            hits.is_empty(),
            "fresh store has no episodes yet — retrieval should be empty",
        );
        assert!(
            r.episodic_run_start_retrieved,
            "first call must mark the run-start slot consumed regardless of hit count",
        );

        // Second call with no Recovering transition: must skip
        // entirely. Previously `step_index == 0` would have re-fired
        // RunStart on policy-deny early-continue paths.
        // Force `step_index` back to 0 to prove the gate (not the
        // counter) is what blocks re-fire.
        r.step_index = 0;
        let hits2 = r.try_retrieve_episodic(Phase::Exploring).await;
        assert!(
            hits2.is_empty(),
            "second call without Recovering transition must be a no-op",
        );
    }

    #[tokio::test]
    async fn recovering_entry_still_fires_after_run_start_consumed() {
        let (mut r, _dir) = enabled_runner_with_store();

        // Consume the run-start slot.
        let _ = r.try_retrieve_episodic(Phase::Exploring).await;
        assert!(r.episodic_run_start_retrieved);

        // Transition into Recovering. Retrieval should fire on the
        // edge (returns empty here because no episodes exist yet, but
        // the call should still execute the trigger branch — verified
        // by the side effect of capturing a `recovering_snapshot`).
        r.task_state.phase = Phase::Recovering;
        let _ = r.try_retrieve_episodic(Phase::Exploring).await;
        assert!(
            r.recovering_snapshot.is_some(),
            "Recovering entry must capture a snapshot for the eventual write",
        );
    }

    #[tokio::test]
    async fn advance_recorded_step_index_increments_counter() {
        let mut r = StateRunner::new_for_test("g".to_string());
        assert_eq!(r.step_index, 0);
        r.advance_recorded_step_index();
        assert_eq!(r.step_index, 1);
        r.advance_recorded_step_index();
        assert_eq!(r.step_index, 2);
    }

    #[tokio::test]
    async fn record_policy_deny_failure_sets_stable_kind() {
        // Policy-deny branches funnel through this helper, and the snapshot derived from
        // `last_failed_*` populates `FailureSignature` on the
        // eventual write. The `error_kind` must be the stable
        // snake_case `policy_denied`, not a free-form string.
        let mut r = StateRunner::new_for_test("g".to_string());
        assert!(r.last_failed_tool_name.is_none());
        assert!(r.last_failed_error_kind.is_none());

        r.record_policy_deny_failure("cdp_click");
        assert_eq!(r.last_failed_tool_name.as_deref(), Some("cdp_click"));
        assert_eq!(
            r.last_failed_error_kind.as_deref(),
            Some("policy_denied"),
            "policy-deny error_kind must be the stable snake_case string used by both branches",
        );
    }

    #[tokio::test]
    async fn clear_last_failure_tracking_drops_both_fields() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.record_policy_deny_failure("ax_click");
        r.clear_last_failure_tracking();
        assert!(
            r.last_failed_tool_name.is_none(),
            "tool_name must be cleared after success",
        );
        assert!(
            r.last_failed_error_kind.is_none(),
            "error_kind must be cleared after success",
        );
    }

    #[tokio::test]
    async fn run_turn_no_longer_advances_step_index_directly() {
        // Under the new ownership rule, `run_turn` does not bump the
        // counter — that's the helper's job, called by sites that push
        // an `AgentStep`. `agent_done` is terminal with no step push,
        // so `step_index` must stay 0 after the turn.
        use async_trait::async_trait;
        use std::sync::Mutex;

        struct EmptyExec(Mutex<Vec<Result<String, String>>>);
        #[async_trait]
        impl ToolExecutor for EmptyExec {
            async fn call_tool(&self, _: &str, _: &serde_json::Value) -> Result<String, String> {
                let mut q = self.0.lock().unwrap();
                q.pop().unwrap_or_else(|| Err("no result".into()))
            }
        }

        let mut r = StateRunner::new_for_test("g".to_string());
        let exec = EmptyExec(Mutex::new(vec![]));
        let done = AgentTurn {
            mutations: vec![],
            action: AgentAction::AgentDone {
                summary: "done".into(),
            },
        };
        let _ = r.run_turn(&done, &exec).await;
        assert_eq!(
            r.step_index, 0,
            "run_turn must not advance step_index — only `advance_recorded_step_index` does",
        );
    }
}

#[cfg(test)]
mod skills_apply_mutations_tests {
    //! Spec 3 Phase 3 unit tests for the runner-side scratch fields
    //! populated by `apply_mutations`.

    use super::*;
    use crate::agent::world_model::{AppKind, FocusedApp, Fresh, FreshnessSource};

    fn focused_app(name: &str) -> Fresh<FocusedApp> {
        Fresh {
            value: FocusedApp {
                name: name.to_string(),
                kind: AppKind::Native,
                pid: 1,
            },
            written_at: 0,
            source: FreshnessSource::DirectObservation,
            ttl_steps: None,
        }
    }

    #[test]
    fn push_subgoal_records_id_and_push_idx() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.apply_mutations(&[TaskStateMutation::PushSubgoal {
            text: "open chat".into(),
        }]);
        assert_eq!(r.last_pushed_subgoal_ids.len(), 1);
        assert_eq!(r.push_idx_stack.len(), 1);
        assert_eq!(r.push_idx_stack[0], 0); // recorded_steps was empty
        assert_eq!(r.push_signature_stack.len(), 1);
        assert_eq!(r.produced_node_ids_stack.len(), 1);
        assert!(r.produced_node_ids_stack[0].is_empty());
    }

    #[test]
    fn complete_subgoal_drains_push_idx_into_extraction_queue() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.apply_mutations(&[TaskStateMutation::PushSubgoal {
            text: "open chat".into(),
        }]);
        r.apply_mutations(&[TaskStateMutation::CompleteSubgoal {
            summary: "done".into(),
        }]);
        assert!(r.push_idx_stack.is_empty(), "push_idx popped on complete");
        assert!(
            r.push_signature_stack.is_empty(),
            "push signature popped on complete"
        );
        assert_eq!(
            r.completed_subgoal_extraction_queue.len(),
            1,
            "extraction queue carries the completed milestone",
        );
    }

    #[test]
    fn complete_subgoal_carries_push_time_signature() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.world_model.focused_app = Some(focused_app("Finder"));
        let push_sig = crate::agent::skills::signature::compute_subgoal_signature(
            "open inbox",
            &r.world_model,
        );

        r.apply_mutations(&[TaskStateMutation::PushSubgoal {
            text: "open inbox".into(),
        }]);
        r.world_model.focused_app = Some(focused_app("Mail"));
        let completion_sig = crate::agent::skills::signature::compute_subgoal_signature(
            "open inbox",
            &r.world_model,
        );
        r.apply_mutations(&[TaskStateMutation::CompleteSubgoal {
            summary: "done".into(),
        }]);

        let (_, _, queued_sig, _) = r
            .completed_subgoal_extraction_queue
            .first()
            .expect("queued extraction");
        assert_eq!(queued_sig, &push_sig);
        assert_ne!(queued_sig, &completion_sig);
    }

    #[test]
    fn last_pushed_subgoal_ids_cleared_each_batch() {
        let mut r = StateRunner::new_for_test("g".to_string());
        r.apply_mutations(&[TaskStateMutation::PushSubgoal {
            text: "first".into(),
        }]);
        r.apply_mutations(&[]); // empty batch — should still clear
        assert!(r.last_pushed_subgoal_ids.is_empty());
    }

    #[test]
    fn nested_subgoals_queue_produced_nodes_per_frame() {
        let mut r = StateRunner::new_for_test("g".to_string());
        let outer_node = uuid::Uuid::new_v4();
        let inner_node = uuid::Uuid::new_v4();
        let after_inner_node = uuid::Uuid::new_v4();

        r.apply_mutations(&[TaskStateMutation::PushSubgoal {
            text: "outer".into(),
        }]);
        r.record_produced_node_id(outer_node);

        r.apply_mutations(&[TaskStateMutation::PushSubgoal {
            text: "inner".into(),
        }]);
        r.record_produced_node_id(inner_node);

        r.apply_mutations(&[TaskStateMutation::CompleteSubgoal {
            summary: "inner done".into(),
        }]);
        r.record_produced_node_id(after_inner_node);

        r.apply_mutations(&[TaskStateMutation::CompleteSubgoal {
            summary: "outer done".into(),
        }]);

        assert!(r.produced_node_ids_stack.is_empty());
        assert_eq!(r.completed_subgoal_extraction_queue.len(), 2);
        assert_eq!(
            r.completed_subgoal_extraction_queue[0].3,
            vec![inner_node],
            "inner frame only records nodes produced after the inner push",
        );
        assert_eq!(
            r.completed_subgoal_extraction_queue[1].3,
            vec![outer_node, inner_node, after_inner_node],
            "outer frame records every node produced while it was active",
        );
    }

    #[test]
    fn complete_with_empty_stack_records_warning_not_panic() {
        let mut r = StateRunner::new_for_test("g".to_string());
        let warnings = r.apply_mutations(&[TaskStateMutation::CompleteSubgoal {
            summary: "done".into(),
        }]);
        assert!(!warnings.is_empty());
    }
}

#[cfg(test)]
mod dispatch_skill_tests {
    //! Phase 4 lookup-and-validate coverage for `StateRunner::dispatch_skill`.
    //! The per-step expansion (Task 4.3+) is deferred; these tests pin
    //! the foundation so the resume seam stays stable.

    use super::*;
    use crate::agent::skills::types::{
        ApplicabilityHints, ApplicabilitySignature, ExpectedWorldModelDelta, OutcomePredicate,
        ParameterSlot, ProvenanceEntry, Skill, SkillState, SkillStats, SubgoalSignature,
    };
    use crate::agent::skills::{ActionSketchStep, SkillIndex, SkillScope};
    use chrono::Utc;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    fn make_skill(id: &str, version: u32, state: SkillState, schema: Vec<ParameterSlot>) -> Skill {
        let now = Utc::now();
        Skill {
            id: id.to_string(),
            version,
            state,
            scope: SkillScope::ProjectLocal,
            name: format!("Skill {id}"),
            description: "test skill".to_string(),
            tags: vec![],
            subgoal_text: "open the file".to_string(),
            subgoal_signature: SubgoalSignature("sg".to_string()),
            applicability: ApplicabilityHints {
                apps: vec![],
                hosts: vec![],
                signature: ApplicabilitySignature("app".to_string()),
            },
            parameter_schema: schema,
            action_sketch: vec![ActionSketchStep::ToolCall {
                tool: "noop".to_string(),
                args: serde_json::json!({}),
                captures_pre: vec![],
                captures: vec![],
                expected_world_model_delta: ExpectedWorldModelDelta::default(),
            }],
            outputs: vec![],
            outcome_predicate: OutcomePredicate::SubgoalCompleted {
                post_state_world_model_signature: None,
            },
            provenance: vec![ProvenanceEntry {
                run_id: uuid::Uuid::new_v4().to_string(),
                step_index: 0,
                completed_at: now,
                workflow_hash: "h".to_string(),
            }],
            stats: SkillStats {
                occurrence_count: 1,
                success_rate: 0.5,
                last_seen_at: Some(now),
                last_invoked_at: None,
            },
            edited_by_user: false,
            created_at: now,
            updated_at: now,
            produced_node_ids: vec![],
            body: "# Test\n".to_string(),
        }
    }

    fn tool_step(tool: &str) -> ActionSketchStep {
        ActionSketchStep::ToolCall {
            tool: tool.to_string(),
            args: serde_json::json!({}),
            captures_pre: vec![],
            captures: vec![],
            expected_world_model_delta: ExpectedWorldModelDelta::default(),
        }
    }

    fn slot(name: &str, type_tag: &str, default: Option<serde_json::Value>) -> ParameterSlot {
        ParameterSlot {
            name: name.to_string(),
            type_tag: type_tag.to_string(),
            description: None,
            default,
            enum_values: None,
        }
    }

    fn fresh_runner_with_skill(
        skill: Option<Skill>,
    ) -> (StateRunner, mpsc::Receiver<RunnerOutput>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let mut runner = StateRunner::new_for_test_with_skills(
            "test goal".to_string(),
            tmp.path().to_path_buf(),
        );
        let embedder = Arc::new(crate::agent::episodic::HashedShingleEmbedder::default());
        let mut index = SkillIndex::empty(embedder);
        if let Some(s) = skill {
            index.upsert(s);
        }
        runner.skill_index = Arc::new(parking_lot::RwLock::new(index));
        let (tx, rx) = mpsc::channel(16);
        runner.event_tx = Some(tx);
        (runner, rx, tmp)
    }

    #[test]
    fn single_step_bridge_rejects_multi_step_skill_before_partial_dispatch() {
        let mut skill = make_skill("multi", 1, SkillState::Confirmed, vec![]);
        skill.action_sketch = vec![tool_step("first"), tool_step("second")];
        let frame = SkillFrame::new(Arc::new(skill), serde_json::json!({}));

        match StateRunner::skill_frame_to_single_step_action(&frame) {
            AgentAction::AgentReplan { reason } => {
                assert!(
                    reason.contains("2 replay steps"),
                    "reason should explain unsupported multi-step replay: {reason}"
                );
            }
            other => panic!("expected fail-closed replan, got {:?}", other),
        }
    }

    #[test]
    fn single_step_bridge_dispatches_exactly_one_tool_step() {
        let skill = make_skill("single", 3, SkillState::Confirmed, vec![]);
        let frame = SkillFrame::new(Arc::new(skill), serde_json::json!({}));

        match StateRunner::skill_frame_to_single_step_action(&frame) {
            AgentAction::ToolCall {
                tool_name,
                tool_call_id,
                ..
            } => {
                assert_eq!(tool_name, "noop");
                assert_eq!(tool_call_id, "skill-single-v3-step-0");
            }
            other => panic!("expected single-step tool call, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn unknown_id_yields_replan_naming_the_id() {
        let (mut runner, _rx, _tmp) = fresh_runner_with_skill(None);
        let err = runner
            .dispatch_skill("never_extracted", 1, serde_json::json!({}))
            .await
            .expect_err("missing skill must fail");
        assert!(err.contains("never_extracted"), "reason: {err}");
    }

    #[tokio::test]
    async fn draft_state_is_rejected() {
        let skill = make_skill("draftish", 1, SkillState::Draft, vec![]);
        let (mut runner, _rx, _tmp) = fresh_runner_with_skill(Some(skill));
        let err = runner
            .dispatch_skill("draftish", 1, serde_json::json!({}))
            .await
            .expect_err("draft must not invoke");
        assert!(err.contains("draft"), "reason: {err}");
    }

    #[tokio::test]
    async fn invalid_parameters_yield_replan() {
        let skill = make_skill(
            "needs_count",
            1,
            SkillState::Confirmed,
            vec![slot("count", "integer", None)],
        );
        let (mut runner, _rx, _tmp) = fresh_runner_with_skill(Some(skill));
        let err = runner
            .dispatch_skill("needs_count", 1, serde_json::json!({}))
            .await
            .expect_err("missing required field must fail");
        assert!(err.contains("count"), "reason: {err}");
    }

    #[tokio::test]
    async fn confirmed_emits_invoked_event_and_marks_invoked() {
        let skill = make_skill(
            "confirm_ok",
            2,
            SkillState::Confirmed,
            vec![slot("name", "string", None)],
        );
        let (mut runner, mut rx, _tmp) = fresh_runner_with_skill(Some(skill));
        let frame = runner
            .dispatch_skill("confirm_ok", 2, serde_json::json!({"name": "x"}))
            .await
            .expect("confirmed skill should resolve");
        assert_eq!(frame.skill.id, "confirm_ok");
        assert_eq!(frame.skill.version, 2);
        assert_eq!(frame.next_step, 0);

        let stamped = runner
            .skill_index
            .read()
            .get("confirm_ok", 2)
            .unwrap()
            .stats
            .last_invoked_at;
        assert!(stamped.is_some());

        let event = rx
            .try_recv()
            .expect("SkillInvoked must be emitted")
            .into_event()
            .expect("SkillInvoked must be a durable event");
        match event {
            AgentEvent::SkillInvoked {
                skill_id,
                version,
                parameter_count,
                ..
            } => {
                assert_eq!(skill_id, "confirm_ok");
                assert_eq!(version, 2);
                assert_eq!(parameter_count, 1);
            }
            other => panic!("expected SkillInvoked, got {:?}", other),
        }
    }
}
