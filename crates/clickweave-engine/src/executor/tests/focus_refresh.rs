use super::helpers::*;
use clickweave_core::AppKind;

#[tokio::test]
async fn refresh_focused_pid_upgrades_placeholder_pid() {
    let mut exec = make_test_executor();
    *exec.write_focused_app() = Some(("Notes".to_string(), AppKind::Native, 0));

    let mcp = StubToolProvider::new();
    mcp.push_text_response(r#"[{"name": "Notes", "pid": 4242}]"#);

    exec.refresh_focused_pid(&mcp).await;

    let (name, _kind, pid) = exec
        .read_focused_app()
        .clone()
        .expect("focused_app should still be set");
    assert_eq!(name, "Notes");
    assert_eq!(pid, 4242);

    let calls = mcp.take_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "list_apps");
}

#[tokio::test]
async fn refresh_focused_pid_no_op_when_pid_already_known() {
    let mut exec = make_test_executor();
    *exec.write_focused_app() = Some(("Editor".to_string(), AppKind::Native, 777));

    let mcp = StubToolProvider::new();
    // No queued responses — a call to list_apps would panic.
    exec.refresh_focused_pid(&mcp).await;

    let (_name, _kind, pid) = exec.read_focused_app().clone().unwrap();
    assert_eq!(pid, 777);
    assert!(mcp.take_calls().is_empty());
}

#[tokio::test]
async fn refresh_focused_pid_no_op_when_focused_app_empty() {
    let mut exec = make_test_executor();
    *exec.write_focused_app() = None;

    let mcp = StubToolProvider::new();
    exec.refresh_focused_pid(&mcp).await;

    assert!(exec.read_focused_app().is_none());
    assert!(mcp.take_calls().is_empty());
}

#[tokio::test]
async fn refresh_focused_pid_also_updates_cdp_connected_app_pid() {
    let mut exec = make_test_executor();
    *exec.write_focused_app() = Some(("Chrome".to_string(), AppKind::Native, 0));
    exec.cdp_state_mut().connected_app = Some(("Chrome".to_string(), 0));

    let mcp = StubToolProvider::new();
    mcp.push_text_response(r#"[{"name": "Chrome", "pid": 9001}]"#);

    exec.refresh_focused_pid(&mcp).await;

    let (_name, _kind, pid) = exec.read_focused_app().clone().unwrap();
    assert_eq!(pid, 9001);
    assert_eq!(exec.cdp_state().connected_app.as_ref().unwrap().1, 9001);
}

#[tokio::test]
async fn refresh_focused_pid_leaves_placeholder_when_lookup_fails() {
    let mut exec = make_test_executor();
    *exec.write_focused_app() = Some(("Ghost".to_string(), AppKind::Native, 0));

    let mcp = StubToolProvider::new();
    mcp.push_text_response("[]");

    exec.refresh_focused_pid(&mcp).await;

    // Placeholder should remain — better than losing focus tracking.
    let (name, _kind, pid) = exec.read_focused_app().clone().unwrap();
    assert_eq!(name, "Ghost");
    assert_eq!(pid, 0);
    // Verify the lookup was attempted (so a future early-return refactor
    // can't spuriously pass this test).
    assert_eq!(mcp.take_calls().len(), 1);
}
