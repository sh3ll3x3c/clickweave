use clickweave_host::{AgentEvent, RunnerOutput, TerminalReason};

/// Map a `TerminalReason` to a CLI exit code.
///
/// Exit code table (decision 10):
///  0 = Completed
///  2 = MaxStepsReached
///  3 = MaxErrorsReached
///  4 = ConsecutiveDestructiveCap
///  5 = LoopDetected
///  6 = ApprovalUnavailable
///  7 = CompletionDisagreement
///  1 = setup/transport error (used by command wiring, not here)
///
/// `DisagreementConfirmed` / `DisagreementCancelled` never arise in the CLI.
pub fn exit_code_for(reason: &TerminalReason) -> i32 {
    match reason {
        TerminalReason::Completed { .. } => 0,
        TerminalReason::MaxStepsReached { .. } => 2,
        TerminalReason::MaxErrorsReached { .. } => 3,
        TerminalReason::ConsecutiveDestructiveCap { .. } => 4,
        TerminalReason::LoopDetected { .. } => 5,
        TerminalReason::ApprovalUnavailable => 6,
        TerminalReason::CompletionDisagreement { .. } => 7,
        // Tauri-only; should not arise in CLI.
        TerminalReason::DisagreementConfirmed { .. } => 0,
        TerminalReason::DisagreementCancelled { .. } => 7,
    }
}

/// Render a single `RunnerOutput` event to stderr in human-readable format.
///
/// The drain loop calls this for each event. Returns the `AgentEvent` if the
/// item carried one (so the caller can check for a terminal reason), `None`
/// for `DrainBarrier` and `SkillProposalNeeded`.
pub fn render_human(output: &RunnerOutput) -> Option<&AgentEvent> {
    match output {
        RunnerOutput::Event(event) => {
            render_event_human(event);
            Some(event)
        }
        RunnerOutput::DrainBarrier { .. } | RunnerOutput::SkillProposalNeeded { .. } => None,
    }
}

fn render_event_human(event: &AgentEvent) {
    use std::io::Write;
    let stderr = std::io::stderr();
    let mut out = stderr.lock();
    match event {
        AgentEvent::StepCompleted {
            step_index,
            tool_name,
            summary,
        } => {
            let _ = writeln!(out, "[step {step_index}] {tool_name}: {summary}");
        }
        AgentEvent::StepFailed {
            step_index,
            tool_name,
            error,
        } => {
            let _ = writeln!(out, "[step {step_index}] FAILED {tool_name}: {error}");
        }
        AgentEvent::GoalComplete { summary } => {
            let _ = writeln!(out, "Goal completed: {summary}");
        }
        AgentEvent::Error { message } => {
            let _ = writeln!(out, "Error: {message}");
        }
        AgentEvent::Warning { message } => {
            let _ = writeln!(out, "Warning: {message}");
        }
        AgentEvent::SubAction { tool_name, summary } => {
            let _ = writeln!(out, "  [{tool_name}] {summary}");
        }
        AgentEvent::CdpConnected { app_name, port } => {
            let _ = writeln!(out, "CDP connected: {app_name} (port {port})");
        }
        AgentEvent::CompletionDisagreement {
            vlm_reasoning,
            agent_summary,
            ..
        } => {
            let _ = writeln!(out, "Completion disagreement:");
            let _ = writeln!(out, "  Agent summary: {agent_summary}");
            let _ = writeln!(out, "  VLM reasoning: {vlm_reasoning}");
        }
        AgentEvent::ConsecutiveDestructiveCapHit {
            recent_tool_names,
            cap,
        } => {
            let _ = writeln!(
                out,
                "Destructive cap ({cap}) hit: {}",
                recent_tool_names.join(", ")
            );
        }
        // Informational events — render a minimal line.
        AgentEvent::TaskStateChanged { .. } => {}
        AgentEvent::WorldModelChanged { .. } => {}
        AgentEvent::BoundaryRecordWritten {
            boundary_kind,
            step_index,
            ..
        } => {
            let _ = writeln!(out, "  [boundary:{boundary_kind:?}] step {step_index}");
        }
        AgentEvent::EpisodesRetrieved { count, .. } => {
            let _ = writeln!(out, "  Retrieved {count} episode(s)");
        }
        AgentEvent::EpisodeWritten { outcome, .. } => {
            let _ = writeln!(out, "  Episode written: {outcome}");
        }
        AgentEvent::EpisodePromoted {
            promoted_episode_ids,
            ..
        } => {
            let _ = writeln!(out, "  {} episode(s) promoted", promoted_episode_ids.len());
        }
        AgentEvent::SkillInvoked {
            skill_id, version, ..
        } => {
            let _ = writeln!(out, "  Invoked skill {skill_id} v{version}");
        }
        AgentEvent::SkillExtracted { skill_id, .. } => {
            let _ = writeln!(out, "  Skill extracted: {skill_id}");
        }
        AgentEvent::SkillConfirmed { skill_id, .. } => {
            let _ = writeln!(out, "  Skill confirmed: {skill_id}");
        }
        AgentEvent::CompletionDisagreementResolved { action, .. } => {
            let _ = writeln!(out, "  Disagreement resolved: {action:?}");
        }
    }
}

/// Emit a single `AgentEvent` as NDJSON to stdout.
///
/// `CompletionDisagreement.screenshot_b64` is redacted to a byte-count
/// placeholder unless `include_screenshots` is true.
pub fn render_json(event: &AgentEvent, include_screenshots: bool) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let value = if include_screenshots {
        serde_json::to_value(event).unwrap_or(serde_json::Value::Null)
    } else {
        redact_screenshot(event)
    };

    let line = serde_json::to_string(&value).unwrap_or_default();
    let _ = writeln!(out, "{line}");
}

/// Serialize the event to JSON, replacing `screenshot_b64` in
/// `CompletionDisagreement` with a `"<screenshot: N bytes>"` placeholder.
fn redact_screenshot(event: &AgentEvent) -> serde_json::Value {
    let mut value = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);

    if let AgentEvent::CompletionDisagreement { screenshot_b64, .. } = event
        && let serde_json::Value::Object(ref mut map) = value
    {
        let byte_count = screenshot_b64.len();
        map.insert(
            "screenshot_b64".to_string(),
            serde_json::Value::String(format!("<screenshot: {byte_count} bytes>")),
        );
    }

    value
}

/// Print the terminal summary to stdout (human mode).
pub fn print_terminal_summary(reason: &TerminalReason) {
    use std::io::Write;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match reason {
        TerminalReason::Completed { summary } => {
            let _ = writeln!(out, "Completed: {summary}");
        }
        TerminalReason::MaxStepsReached { steps_executed } => {
            let _ = writeln!(out, "Stopped: max steps reached ({steps_executed} steps)");
        }
        TerminalReason::MaxErrorsReached { consecutive_errors } => {
            let _ = writeln!(
                out,
                "Stopped: max consecutive errors reached ({consecutive_errors})"
            );
        }
        TerminalReason::ApprovalUnavailable => {
            let _ = writeln!(
                out,
                "Stopped: approval unavailable (no TTY, no --yes/--policy)"
            );
        }
        TerminalReason::CompletionDisagreement {
            agent_summary,
            vlm_reasoning,
        } => {
            let _ = writeln!(out, "Stopped: completion verification disagreed");
            let _ = writeln!(out, "  Agent summary: {agent_summary}");
            let _ = writeln!(out, "  VLM reasoning: {vlm_reasoning}");
        }
        TerminalReason::ConsecutiveDestructiveCap {
            recent_tool_names,
            cap,
        } => {
            let _ = writeln!(
                out,
                "Stopped: consecutive destructive cap ({cap}) — {}",
                recent_tool_names.join(", ")
            );
        }
        TerminalReason::LoopDetected { tool_name, error } => {
            let _ = writeln!(
                out,
                "Stopped: loop detected ({tool_name} kept failing with: {error})"
            );
        }
        TerminalReason::DisagreementConfirmed { agent_summary } => {
            let _ = writeln!(out, "Completed (user override): {agent_summary}");
        }
        TerminalReason::DisagreementCancelled { vlm_reasoning, .. } => {
            let _ = writeln!(out, "Cancelled after VLM disagreement: {vlm_reasoning}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_completed() {
        assert_eq!(
            exit_code_for(&TerminalReason::Completed {
                summary: "done".to_string()
            }),
            0
        );
    }

    #[test]
    fn exit_code_max_steps() {
        assert_eq!(
            exit_code_for(&TerminalReason::MaxStepsReached { steps_executed: 30 }),
            2
        );
    }

    #[test]
    fn exit_code_max_errors() {
        assert_eq!(
            exit_code_for(&TerminalReason::MaxErrorsReached {
                consecutive_errors: 3
            }),
            3
        );
    }

    #[test]
    fn exit_code_consecutive_destructive() {
        assert_eq!(
            exit_code_for(&TerminalReason::ConsecutiveDestructiveCap {
                recent_tool_names: vec!["a".to_string()],
                cap: 3,
            }),
            4
        );
    }

    #[test]
    fn exit_code_loop_detected() {
        assert_eq!(
            exit_code_for(&TerminalReason::LoopDetected {
                tool_name: "t".to_string(),
                error: "e".to_string(),
            }),
            5
        );
    }

    #[test]
    fn exit_code_approval_unavailable() {
        assert_eq!(exit_code_for(&TerminalReason::ApprovalUnavailable), 6);
    }

    #[test]
    fn exit_code_completion_disagreement() {
        assert_eq!(
            exit_code_for(&TerminalReason::CompletionDisagreement {
                agent_summary: "a".to_string(),
                vlm_reasoning: "v".to_string(),
            }),
            7
        );
    }

    #[test]
    fn redact_screenshot_replaces_blob_with_placeholder() {
        let event = AgentEvent::CompletionDisagreement {
            screenshot_b64: "AAAA".to_string(),
            vlm_reasoning: "vlm".to_string(),
            agent_summary: "done".to_string(),
        };
        let value = redact_screenshot(&event);
        let b64 = value
            .as_object()
            .unwrap()
            .get("screenshot_b64")
            .unwrap()
            .as_str()
            .unwrap();
        assert!(
            b64.starts_with("<screenshot:"),
            "screenshot_b64 should be replaced with a placeholder, got: {b64}"
        );
        // Must contain byte count.
        assert!(
            b64.contains("4 bytes"),
            "placeholder should encode byte count, got: {b64}"
        );
    }

    #[test]
    fn redact_screenshot_preserves_other_fields() {
        let event = AgentEvent::CompletionDisagreement {
            screenshot_b64: "data".to_string(),
            vlm_reasoning: "reason".to_string(),
            agent_summary: "sum".to_string(),
        };
        let value = redact_screenshot(&event);
        let map = value.as_object().unwrap();
        assert_eq!(
            map.get("vlm_reasoning").and_then(|v| v.as_str()),
            Some("reason")
        );
        assert_eq!(
            map.get("agent_summary").and_then(|v| v.as_str()),
            Some("sum")
        );
    }

    #[test]
    fn non_disagreement_event_not_redacted() {
        let event = AgentEvent::GoalComplete {
            summary: "done".to_string(),
        };
        let value = redact_screenshot(&event);
        // Should serialise normally, no screenshot_b64 key.
        assert!(value.as_object().unwrap().get("screenshot_b64").is_none());
    }
}
