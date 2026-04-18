//! Tests for the degraded-trace-persistence emission path in
//! [`WorkflowExecutor::record_event`].
//!
//! A `RunStorage` with `persistent=true` and no `begin_execution()` call
//! makes every `append_execution_event` fail via the "begin_execution()
//! must be called" guard — a deterministic failure stream for testing
//! the streak threshold without patching the storage layer. Successful
//! writes are simulated by temporarily flipping `persistent` to `false`
//! and routing through a NodeRun-less `run_dir` path — but since
//! `append_execution_event` errors before the `persistent` check, we
//! instead simulate success via a NodeRun whose `append_event` checks
//! `persistent` first.

use super::helpers::*;
use crate::executor::{ExecutorEvent, TRACE_WRITE_FAILURE_THRESHOLD};
use clickweave_core::{NodeRun, TraceLevel};
use serde_json::Value;
use uuid::Uuid;

/// Drain the event channel and return every event emitted so far.
fn drain(rx: &mut tokio::sync::mpsc::Receiver<ExecutorEvent>) -> Vec<ExecutorEvent> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        out.push(event);
    }
    out
}

fn count_errors(events: &[ExecutorEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, ExecutorEvent::Error(_)))
        .count()
}

/// Build a minimal `NodeRun` suitable for feeding `record_event`. The
/// node isn't registered with the storage — the storage either succeeds
/// on the `persistent=false` early-return or is irrelevant to the
/// behaviour under test.
fn dummy_run() -> NodeRun {
    NodeRun {
        run_id: Uuid::new_v4(),
        node_id: Uuid::new_v4(),
        node_name: "probe".to_string(),
        execution_dir: String::new(),
        started_at: 0,
        ended_at: None,
        status: clickweave_core::RunStatus::Ok,
        trace_level: TraceLevel::Full,
        events: Vec::new(),
        artifacts: Vec::new(),
        observed_summary: None,
    }
}

#[tokio::test]
async fn record_event_emits_error_once_after_threshold_is_reached() {
    // `make_test_executor_with_event_tx()` builds a persistent
    // `RunStorage` but never calls `begin_execution`, so every
    // `append_execution_event` will fail with the "begin_execution() must
    // be called" context error. That gives us a deterministic failure
    // stream without patching the storage.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut exec = make_test_executor_with_event_tx(tx);

    // First N-1 failures: counted but no emission yet.
    for _ in 0..(TRACE_WRITE_FAILURE_THRESHOLD - 1) {
        exec.record_event(None, "probe", Value::Null);
    }
    assert_eq!(
        count_errors(&drain(&mut rx)),
        0,
        "no error event emitted before the threshold",
    );

    // Nth failure crosses the threshold and emits exactly one error.
    exec.record_event(None, "probe", Value::Null);
    let events = drain(&mut rx);
    assert_eq!(
        count_errors(&events),
        1,
        "exactly one Error event on threshold crossing",
    );
    // The error message must reference the degraded-trace semantics so the
    // UI can branch on it.
    let msg = events
        .iter()
        .find_map(|e| match e {
            ExecutorEvent::Error(m) => Some(m.clone()),
            _ => None,
        })
        .expect("error event present");
    assert!(
        msg.to_lowercase().contains("trace"),
        "error must mention trace: {msg}"
    );

    // Further failures after the threshold: still counted, still nothing
    // emitted (one-shot per streak).
    exec.record_event(None, "probe", Value::Null);
    exec.record_event(None, "probe", Value::Null);
    assert_eq!(
        count_errors(&drain(&mut rx)),
        0,
        "no re-emission during a sustained streak",
    );
}

#[tokio::test]
async fn successful_write_clears_streak_and_rearms_emission() {
    // Use `append_event(Some(&run))` for simulated successes: that path
    // short-circuits on `!self.persistent` before any disk I/O, so
    // flipping `set_persistent(false)` yields a clean Ok(). Failures use
    // `append_execution_event` which hits the `begin_execution` guard
    // independent of the persistence flag.
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let mut exec = make_test_executor_with_event_tx(tx);
    let run = dummy_run();

    // Build the streak up to emission.
    for _ in 0..TRACE_WRITE_FAILURE_THRESHOLD {
        exec.record_event(None, "probe", Value::Null);
    }
    let streak_events = drain(&mut rx);
    assert_eq!(
        count_errors(&streak_events),
        1,
        "first streak emits exactly once",
    );

    // Flip storage to non-persistent so the NodeRun path returns Ok; this
    // clears the streak.
    exec.storage.set_persistent(false);
    exec.record_event(Some(&run), "probe", Value::Null);
    assert_eq!(
        count_errors(&drain(&mut rx)),
        0,
        "successful write must not emit",
    );

    // Flip back to persistent (the execution-event path still errors)
    // and let the streak rebuild — emission must re-arm.
    exec.storage.set_persistent(true);
    for _ in 0..TRACE_WRITE_FAILURE_THRESHOLD {
        exec.record_event(None, "probe", Value::Null);
    }
    assert_eq!(
        count_errors(&drain(&mut rx)),
        1,
        "second streak must re-emit exactly once",
    );
}
