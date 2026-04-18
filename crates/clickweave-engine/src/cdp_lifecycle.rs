//! Shared CDP lifecycle primitives used by both the deterministic executor
//! and the agent runner.
//!
//! The CDP "probe → quit → poll for exit → force-quit → relaunch with
//! `--remote-debugging-port` → connect with retries" state machine was
//! previously duplicated across `executor::deterministic::cdp` and
//! `agent::loop_runner`. A fix in one path silently diverged from the other
//! — most visibly, the agent lost the selected-tab tracking that the
//! executor maintains via `cdp_selected_pages`.
//!
//! This module owns:
//! - [`CdpState`]: the bookkeeping shared by both callers (connected app
//!   identity and last-observed page URL per app instance).
//! - Free async functions for each lifecycle step, taking an [`Mcp`] handle
//!   and — where required — a `&mut CdpState`. The caller is still
//!   responsible for any side channels it wants to surface around each
//!   step (UI events, decision-cache updates, chrome-profile-specific
//!   relaunch paths) because those differ between agent and executor.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use crate::executor::Mcp;
use clickweave_mcp::{ToolCallResult, ToolContent};

// ── Retry / timing constants ────────────────────────────────────────────
// Previously lived in `executor/deterministic/cdp.rs`. Kept `pub(crate)`
// so existing re-export sites (and new call sites in the agent loop) can
// continue to reference them from a single source of truth.

/// Maximum attempts for `cdp_connect` before giving up.
pub(crate) const CDP_CONNECT_MAX_ATTEMPTS: u32 = 10;
/// Delay between `cdp_connect` retry attempts.
pub(crate) const CDP_CONNECT_RETRY_INTERVAL: Duration = Duration::from_secs(1);
/// Maximum poll iterations waiting for a graceful app quit.
/// Paired with `APP_QUIT_POLL_INTERVAL`, this gives a ~10s graceful window.
pub(crate) const APP_QUIT_MAX_ATTEMPTS: u32 = 20;
/// Poll interval while waiting for `list_apps` to report the app has exited.
pub(crate) const APP_QUIT_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Sleep after a force-kill to let the OS reap the process.
pub(crate) const APP_FORCE_QUIT_GRACE: Duration = Duration::from_secs(2);
/// Sleep after relaunching with `--remote-debugging-port` before connecting.
pub(crate) const APP_RELAUNCH_WARMUP: Duration = Duration::from_secs(3);
/// Timeout for `poll_cdp_ready` after a fresh relaunch.
pub(crate) const CDP_READY_TIMEOUT_AFTER_RELAUNCH_SECS: u64 = 30;
/// Timeout for `poll_cdp_ready` when reusing an existing debug port.
pub(crate) const CDP_READY_TIMEOUT_REUSE_SECS: u64 = 5;

/// In-memory bookkeeping for a single active CDP lifecycle owner.
///
/// Both [`crate::executor::WorkflowExecutor`] and the agent runner hold
/// one of these. The lifecycle free functions take `&mut CdpState` when
/// they need to mutate connection identity or the selected-page map.
#[derive(Debug, Default, Clone)]
pub(crate) struct CdpState {
    /// The `(app_name, pid)` the current CDP session is bound to, if any.
    ///
    /// PID is tracked alongside the name so that two instances of the
    /// same-name app (default Chrome vs. profile-scoped Chrome running
    /// side-by-side) are treated as distinct targets. PID=0 is the
    /// "not yet known" placeholder used immediately after
    /// `launch_app` / `focus_window`; [`Self::upgrade_pid`] promotes it
    /// to the real PID once it becomes available.
    pub(crate) connected_app: Option<(String, i32)>,

    /// Per-app-instance last-observed page URL, used to restore the selected
    /// tab across a CDP disconnect/reconnect cycle. Keyed by
    /// `(app_name, pid)` for the same reason as `connected_app`: two
    /// same-name instances must not clobber each other's remembered tab.
    /// In-memory only; session-specific URLs are worse than a default
    /// first-page fallback when stale.
    pub(crate) selected_pages: HashMap<(String, i32), String>,
}

impl CdpState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Return whether an active CDP session matches `(app_name, pid)`.
    ///
    /// Two entries with matching names and known PIDs must have identical
    /// PIDs. When either side carries the `0` placeholder, the name match
    /// is treated as sufficient — see [`Self::upgrade_pid`] for the
    /// promotion flow.
    pub(crate) fn is_connected_to(&self, app_name: &str, pid: i32) -> bool {
        match &self.connected_app {
            Some((name, stored_pid)) => {
                if name != app_name {
                    return false;
                }
                if *stored_pid != 0 && pid != 0 {
                    return *stored_pid == pid;
                }
                true
            }
            None => false,
        }
    }

    /// Clear connection state and any remembered tabs for `app_name`.
    ///
    /// Called when the app is (being) quit: any URL remembered for any
    /// instance of that name is now stale, and the current connection —
    /// if bound to this app — must be forgotten.
    pub(crate) fn mark_app_quit(&mut self, app_name: &str) {
        if self
            .connected_app
            .as_ref()
            .is_some_and(|(name, _)| name == app_name)
        {
            self.connected_app = None;
        }
        self.selected_pages.retain(|(name, _), _| name != app_name);
    }

    /// Promote a placeholder `(app_name, 0)` entry to `(app_name, pid)`.
    ///
    /// Applies to both `connected_app` and `selected_pages`. Entries
    /// keyed by other apps, or by the same app but with a different
    /// known PID, are left alone.
    pub(crate) fn upgrade_pid(&mut self, app_name: &str, pid: i32) {
        if let Some((name, stored_pid)) = self.connected_app.as_mut()
            && name == app_name
            && *stored_pid == 0
        {
            *stored_pid = pid;
        }

        if let Some(url) = self.selected_pages.remove(&(app_name.to_string(), 0)) {
            self.selected_pages.insert((app_name.to_string(), pid), url);
        }
    }

    /// Mark `(app_name, pid)` as the active CDP session target.
    pub(crate) fn set_connected(&mut self, app_name: &str, pid: i32) {
        self.connected_app = Some((app_name.to_string(), pid));
    }

    /// Record the URL currently selected for `(app_name, pid)`.
    pub(crate) fn record_selected_page(&mut self, app_name: &str, pid: i32, url: String) {
        self.selected_pages.insert((app_name.to_string(), pid), url);
    }

    /// Look up the remembered URL for `(app_name, pid)`.
    pub(crate) fn remembered_url(&self, app_name: &str, pid: i32) -> Option<&str> {
        self.selected_pages
            .get(&(app_name.to_string(), pid))
            .map(String::as_str)
    }
}

/// Pull the first text block out of an MCP tool result.
///
/// Mirrors [`crate::executor::WorkflowExecutor::extract_result_text`] but
/// lives here as a free function so lifecycle primitives don't need a
/// generic executor type parameter.
pub(crate) fn extract_text(result: &ToolCallResult) -> String {
    result
        .content
        .iter()
        .find_map(|c| match c {
            ToolContent::Text { text } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Outcome of a graceful-quit poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuitOutcome {
    /// `list_apps` confirmed the app was gone within the poll window.
    Graceful,
    /// The poll window elapsed without confirmation — caller should
    /// proceed to force-quit.
    TimedOut,
}

/// Issue `quit_app` and poll `list_apps` until the app is gone or the
/// graceful window elapses.
///
/// A `Transport` failure from `quit_app` is logged and treated the same
/// as a successful dispatch — the poll loop decides the outcome. Polling
/// stops early when `list_apps` reports an empty `[]`.
///
/// Clears the corresponding entries from [`CdpState`] on return (both
/// outcomes): an app that is quitting or has just been force-killed
/// carries no valid connection, regardless of whether the graceful
/// window succeeded.
pub(crate) async fn quit_and_wait(
    mcp: &(impl Mcp + ?Sized),
    app_name: &str,
    state: &mut CdpState,
) -> QuitOutcome {
    let quit_args = serde_json::json!({ "app_name": app_name });
    if let Err(e) = mcp.call_tool("quit_app", Some(quit_args)).await {
        tracing::debug!(
            app_name = app_name,
            error = %e,
            "quit_app dispatch failed (continuing with poll)",
        );
    }

    let outcome = poll_until_quit(mcp, app_name).await;
    state.mark_app_quit(app_name);
    outcome
}

/// Poll `list_apps` until the named app is gone or the attempts budget
/// is exhausted. Does **not** mutate [`CdpState`]; callers typically
/// reach for [`quit_and_wait`] instead, which combines the dispatch,
/// poll, and bookkeeping.
pub(crate) async fn poll_until_quit(mcp: &(impl Mcp + ?Sized), app_name: &str) -> QuitOutcome {
    let poll_args = serde_json::json!({ "app_name": app_name, "user_apps_only": true });
    for _ in 0..APP_QUIT_MAX_ATTEMPTS {
        tokio::time::sleep(APP_QUIT_POLL_INTERVAL).await;
        if let Ok(r) = mcp.call_tool("list_apps", Some(poll_args.clone())).await {
            let text = extract_text(&r);
            if text.trim() == "[]" {
                return QuitOutcome::Graceful;
            }
        }
    }
    QuitOutcome::TimedOut
}

/// Best-effort force-quit: fires `quit_app` with `force: true` through a
/// best-effort wrapper (failure is logged, not propagated) and sleeps
/// for the OS reap grace window.
pub(crate) async fn force_quit(mcp: &(impl Mcp + ?Sized), app_name: &str) {
    let force_args = serde_json::json!({ "app_name": app_name, "force": true });
    if let Err(e) = mcp.call_tool("quit_app", Some(force_args)).await {
        tracing::debug!(
            app_name = app_name,
            error = %e,
            "force quit_app failed (continuing)",
        );
    }
    tokio::time::sleep(APP_FORCE_QUIT_GRACE).await;
}

/// Dispatch `launch_app` with `--remote-debugging-port=<port>`.
///
/// Returns `Ok(())` on a successful launch, `Err(msg)` when MCP refused
/// the call or returned an error payload. Does **not** mutate
/// [`CdpState`] — the caller sets `connected_app` only after the
/// subsequent `cdp_connect` succeeds.
pub(crate) async fn launch_with_debug_port(
    mcp: &(impl Mcp + ?Sized),
    app_name: &str,
    port: u16,
) -> Result<(), String> {
    let launch_args = serde_json::json!({
        "app_name": app_name,
        "args": [format!("--remote-debugging-port={}", port)],
    });
    match mcp.call_tool("launch_app", Some(launch_args)).await {
        Ok(r) if r.is_error != Some(true) => Ok(()),
        Ok(r) => Err(format!("launch_app error: {}", extract_text(&r))),
        Err(e) => Err(format!("launch_app dispatch failed: {}", e)),
    }
}

/// Attempt `cdp_connect` up to [`CDP_CONNECT_MAX_ATTEMPTS`] times,
/// sleeping [`CDP_CONNECT_RETRY_INTERVAL`] between attempts.
///
/// Returns the last error text (empty when the first call transport-failed
/// before any payload). `Ok(())` means the server accepted the connect;
/// the caller usually pairs this with [`poll_cdp_ready`] to confirm pages
/// have become visible.
pub(crate) async fn connect_with_retries(
    mcp: &(impl Mcp + ?Sized),
    port: u16,
) -> Result<(), String> {
    let connect_args = serde_json::json!({ "port": port });
    let mut last_err = String::new();
    for attempt in 0..CDP_CONNECT_MAX_ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(CDP_CONNECT_RETRY_INTERVAL).await;
        }
        match mcp
            .call_tool("cdp_connect", Some(connect_args.clone()))
            .await
        {
            Ok(r) if r.is_error != Some(true) => return Ok(()),
            Ok(r) => {
                last_err = extract_text(&r);
                tracing::debug!(
                    port = port,
                    attempt = attempt + 1,
                    error = %last_err,
                    "cdp_connect attempt returned error payload",
                );
            }
            Err(e) => {
                last_err = e.to_string();
                tracing::debug!(
                    port = port,
                    attempt = attempt + 1,
                    error = %last_err,
                    "cdp_connect dispatch failed",
                );
            }
        }
    }
    Err(last_err)
}

/// Poll `cdp_list_pages` until it returns at least one page, or the
/// deadline lapses.
///
/// Returns `Ok(())` on success, `Err(reason)` with the last-known
/// diagnostic otherwise. A successful page-line is one that looks like
/// `^\s*\[\d+]` in its trimmed form — the same heuristic the executor
/// previously used inline.
pub(crate) async fn poll_cdp_ready(
    mcp: &(impl Mcp + ?Sized),
    app_name: &str,
    timeout_secs: u64,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match mcp
            .call_tool("cdp_list_pages", Some(serde_json::json!({})))
            .await
        {
            Ok(result) if result.is_error != Some(true) => {
                let text = extract_text(&result);
                if text.lines().any(|l| {
                    let t = l.trim_start();
                    t.starts_with('[') && t.contains(']')
                }) {
                    tracing::debug!(
                        app_name = app_name,
                        text = %text.trim(),
                        "CDP pages visible",
                    );
                    return Ok(());
                }
                tracing::debug!(
                    app_name = app_name,
                    "cdp_list_pages returned but no pages yet",
                );
            }
            Ok(result) => {
                tracing::debug!(
                    app_name = app_name,
                    error = %extract_text(&result),
                    "cdp_list_pages returned error payload",
                );
            }
            Err(e) => {
                tracing::debug!(
                    app_name = app_name,
                    error = %e,
                    "cdp_list_pages dispatch failed",
                );
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(format!(
                "Timed out waiting for CDP to be ready for '{}' ({}s)",
                app_name, timeout_secs
            ));
        }
        tokio::time::sleep(CDP_CONNECT_RETRY_INTERVAL).await;
    }
}

/// Wait [`APP_RELAUNCH_WARMUP`] for an app launched with the debug port
/// to reach a state where `cdp_connect` can succeed. Factored into its
/// own helper so lifecycle callers can express the sequence as
/// quit → relaunch → warmup → connect.
pub(crate) async fn warmup_after_relaunch() {
    tokio::time::sleep(APP_RELAUNCH_WARMUP).await;
}

/// Pick the MCP payload for `cdp_connect` for the given port.
#[inline]
pub(crate) fn connect_args(port: u16) -> Value {
    serde_json::json!({ "port": port })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_mcp::{ToolCallResult, ToolContent};
    use std::sync::Mutex;

    /// Minimal MCP stub that serves queued responses FIFO and records
    /// the call history. Kept out of the shared `executor::tests::helpers`
    /// fixture because this module lives above the executor in the crate
    /// hierarchy and importing test helpers from a descendant module is
    /// awkward; a focused stub keeps these tests hermetic.
    struct QueueMcp {
        queue: Mutex<Vec<Result<ToolCallResult, String>>>,
        calls: Mutex<Vec<(String, Option<Value>)>>,
    }

    impl QueueMcp {
        fn new() -> Self {
            Self {
                queue: Mutex::new(Vec::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn push_text(&self, text: &str) {
            self.queue.lock().unwrap().push(Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: text.to_string(),
                }],
                is_error: None,
            }));
        }

        fn push_error_payload(&self, text: &str) {
            self.queue.lock().unwrap().push(Ok(ToolCallResult {
                content: vec![ToolContent::Text {
                    text: text.to_string(),
                }],
                is_error: Some(true),
            }));
        }

        fn push_transport_error(&self, message: &str) {
            self.queue.lock().unwrap().push(Err(message.to_string()));
        }

        fn take_calls(&self) -> Vec<(String, Option<Value>)> {
            std::mem::take(&mut *self.calls.lock().unwrap())
        }
    }

    impl Mcp for QueueMcp {
        async fn call_tool(
            &self,
            name: &str,
            arguments: Option<Value>,
        ) -> anyhow::Result<ToolCallResult> {
            self.calls
                .lock()
                .unwrap()
                .push((name.to_string(), arguments.clone()));
            let mut q = self.queue.lock().unwrap();
            if q.is_empty() {
                panic!("QueueMcp: no queued response for '{}'", name);
            }
            match q.remove(0) {
                Ok(r) => Ok(r),
                Err(m) => Err(anyhow::anyhow!(m)),
            }
        }

        fn has_tool(&self, _name: &str) -> bool {
            true
        }

        fn tools_as_openai(&self) -> Vec<Value> {
            Vec::new()
        }

        async fn refresh_server_tool_list(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn upgrade_pid_promotes_placeholder_for_matching_name_only() {
        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 0));
        state
            .selected_pages
            .insert(("Chrome".to_string(), 0), "https://a.example.com/".into());
        // An unrelated entry that must survive untouched.
        state
            .selected_pages
            .insert(("Slack".to_string(), 0), "slack-url".into());
        // A different-PID entry for the same app must not be overwritten.
        state
            .selected_pages
            .insert(("Chrome".to_string(), 9999), "keep-me".into());

        state.upgrade_pid("Chrome", 1234);

        assert_eq!(state.connected_app, Some(("Chrome".to_string(), 1234)));
        assert!(
            !state
                .selected_pages
                .contains_key(&("Chrome".to_string(), 0))
        );
        assert_eq!(
            state
                .selected_pages
                .get(&("Chrome".to_string(), 1234))
                .map(String::as_str),
            Some("https://a.example.com/")
        );
        assert_eq!(
            state
                .selected_pages
                .get(&("Slack".to_string(), 0))
                .map(String::as_str),
            Some("slack-url")
        );
        assert_eq!(
            state
                .selected_pages
                .get(&("Chrome".to_string(), 9999))
                .map(String::as_str),
            Some("keep-me")
        );
    }

    #[test]
    fn upgrade_pid_leaves_other_apps_untouched() {
        let mut state = CdpState::new();
        state.connected_app = Some(("Safari".to_string(), 0));
        state.upgrade_pid("Chrome", 1234);
        assert_eq!(state.connected_app, Some(("Safari".to_string(), 0)));
    }

    #[test]
    fn upgrade_pid_does_not_downgrade_known_pid() {
        // A non-zero stored PID is authoritative — upgrading it with a
        // different value would be a name-collision bug the caller
        // should see, not silently overwrite.
        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 4242));
        state.upgrade_pid("Chrome", 9999);
        assert_eq!(state.connected_app, Some(("Chrome".to_string(), 4242)));
    }

    #[test]
    fn mark_app_quit_clears_connection_and_all_matching_pages() {
        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 4242));
        state
            .selected_pages
            .insert(("Chrome".to_string(), 4242), "a".into());
        state
            .selected_pages
            .insert(("Chrome".to_string(), 9999), "b".into());
        state
            .selected_pages
            .insert(("Safari".to_string(), 1), "c".into());

        state.mark_app_quit("Chrome");

        assert!(state.connected_app.is_none());
        assert!(
            !state
                .selected_pages
                .keys()
                .any(|(name, _)| name == "Chrome")
        );
        assert_eq!(
            state
                .selected_pages
                .get(&("Safari".to_string(), 1))
                .map(String::as_str),
            Some("c"),
        );
    }

    #[test]
    fn mark_app_quit_keeps_connection_bound_to_other_app() {
        let mut state = CdpState::new();
        state.connected_app = Some(("Safari".to_string(), 1));
        state.mark_app_quit("Chrome");
        assert_eq!(state.connected_app, Some(("Safari".to_string(), 1)));
    }

    #[test]
    fn is_connected_to_matches_name_with_placeholder_pid() {
        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 0));
        assert!(state.is_connected_to("Chrome", 4242));
        assert!(state.is_connected_to("Chrome", 0));
        assert!(!state.is_connected_to("Safari", 4242));
    }

    #[test]
    fn is_connected_to_rejects_mismatched_known_pids() {
        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 4242));
        assert!(!state.is_connected_to("Chrome", 9999));
        assert!(state.is_connected_to("Chrome", 4242));
    }

    #[tokio::test]
    async fn quit_and_wait_happy_path_returns_graceful() {
        let mcp = QueueMcp::new();
        // quit_app succeeds; first list_apps poll reports empty.
        mcp.push_text("Quit dispatched");
        mcp.push_text("[]");

        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 4242));
        state
            .selected_pages
            .insert(("Chrome".to_string(), 4242), "url".into());

        let outcome = quit_and_wait(&mcp, "Chrome", &mut state).await;

        assert_eq!(outcome, QuitOutcome::Graceful);
        assert!(state.connected_app.is_none());
        assert!(state.selected_pages.is_empty());
        let calls = mcp.take_calls();
        assert_eq!(calls[0].0, "quit_app");
        assert_eq!(calls[1].0, "list_apps");
    }

    #[tokio::test]
    async fn quit_and_wait_tolerates_quit_app_transport_error() {
        // The quit_app dispatch fails — the app was already dead, or the
        // server returned an error. The poll loop must still run and the
        // state bookkeeping must still clear.
        let mcp = QueueMcp::new();
        mcp.push_transport_error("no such app");
        mcp.push_text("[]");

        let mut state = CdpState::new();
        state.connected_app = Some(("Chrome".to_string(), 4242));

        let outcome = quit_and_wait(&mcp, "Chrome", &mut state).await;

        assert_eq!(outcome, QuitOutcome::Graceful);
        assert!(state.connected_app.is_none());
    }

    #[tokio::test]
    async fn launch_with_debug_port_propagates_error_payload() {
        let mcp = QueueMcp::new();
        mcp.push_error_payload("no launch binary");

        let err = launch_with_debug_port(&mcp, "NoSuchApp", 9222)
            .await
            .expect_err("error payload must surface");
        assert!(err.contains("no launch binary"));
    }

    #[tokio::test]
    async fn launch_with_debug_port_sends_remote_debugging_flag() {
        let mcp = QueueMcp::new();
        mcp.push_text("launched");

        launch_with_debug_port(&mcp, "Chrome", 9222)
            .await
            .expect("ok path");

        let calls = mcp.take_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "launch_app");
        let args = calls[0].1.as_ref().unwrap();
        assert_eq!(args["app_name"], "Chrome");
        assert_eq!(args["args"][0], "--remote-debugging-port=9222");
    }

    #[tokio::test]
    async fn connect_with_retries_exhausts_budget_and_returns_last_error() {
        // This reproduces the "connect_and_poll times out after relaunch"
        // scenario: every cdp_connect attempt returns an error payload.
        let mcp = QueueMcp::new();
        for _ in 0..CDP_CONNECT_MAX_ATTEMPTS {
            mcp.push_error_payload("connection refused");
        }

        // Use `tokio::time::pause()` so the test doesn't wait out all the
        // real retry intervals.
        tokio::time::pause();
        let result = connect_with_retries(&mcp, 9222).await;
        let err = result.expect_err("exhausted budget must return Err");
        assert!(err.contains("connection refused"));

        let calls = mcp.take_calls();
        assert_eq!(calls.len(), CDP_CONNECT_MAX_ATTEMPTS as usize);
        for (name, args) in &calls {
            assert_eq!(name, "cdp_connect");
            assert_eq!(args.as_ref().unwrap()["port"], 9222);
        }
    }

    #[tokio::test]
    async fn connect_with_retries_succeeds_on_late_attempt() {
        let mcp = QueueMcp::new();
        mcp.push_error_payload("not ready");
        mcp.push_error_payload("still not ready");
        mcp.push_text("connected");

        tokio::time::pause();
        connect_with_retries(&mcp, 9222)
            .await
            .expect("late success");
        assert_eq!(mcp.take_calls().len(), 3);
    }

    #[tokio::test]
    async fn poll_cdp_ready_times_out_when_no_pages() {
        // Relaunch connect timeout scenario: server is reachable but
        // list_pages never returns a bracketed page line.
        let mcp = QueueMcp::new();
        // Seed enough "no pages yet" responses that the deadline trips.
        for _ in 0..4 {
            mcp.push_text("no pages yet");
        }

        tokio::time::pause();
        // 2s timeout means at least one poll + deadline check.
        let err = poll_cdp_ready(&mcp, "Chrome", 2)
            .await
            .expect_err("timeout must return Err");
        assert!(err.contains("Timed out"));
        assert!(err.contains("Chrome"));
    }
}
