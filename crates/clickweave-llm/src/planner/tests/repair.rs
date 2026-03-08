use super::helpers::MockBackend;
use crate::Message;
use crate::planner::repair::chat_with_repair;

#[tokio::test]
async fn chat_with_repair_succeeds_on_first_try() {
    let backend = MockBackend::single(r#"{"result": "ok"}"#);
    let messages = vec![Message::user("generate JSON")];

    let result = chat_with_repair(&backend, "Test", messages, |content| {
        let v: serde_json::Value = serde_json::from_str(content)?;
        Ok(v)
    })
    .await;

    assert!(result.is_ok());
    let v = result.unwrap();
    assert_eq!(v["result"], "ok");
    assert_eq!(backend.call_count(), 1);
}

#[tokio::test]
async fn chat_with_repair_retries_on_parse_error_then_succeeds() {
    // First response is invalid JSON, second is valid
    let backend = MockBackend::new(vec!["not valid json", r#"{"fixed": true}"#]);
    let messages = vec![Message::user("generate JSON")];

    let result = chat_with_repair(&backend, "Test", messages, |content| {
        let v: serde_json::Value = serde_json::from_str(content)?;
        Ok(v)
    })
    .await;

    assert!(result.is_ok());
    let v = result.unwrap();
    assert_eq!(v["fixed"], true);
    assert_eq!(backend.call_count(), 2);
}

#[tokio::test]
async fn chat_with_repair_fails_after_max_attempts() {
    // Both responses are invalid JSON — should fail after 2 attempts
    // (MAX_REPAIR_ATTEMPTS = 1, so initial + 1 retry = 2 calls)
    let backend = MockBackend::new(vec!["bad json 1", "bad json 2"]);
    let messages = vec![Message::user("generate JSON")];

    let result = chat_with_repair(&backend, "Test", messages, |content| {
        let _v: serde_json::Value = serde_json::from_str(content)?;
        Ok(())
    })
    .await;

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    // The error should be from the final parse failure
    assert!(
        err_msg.contains("expected"),
        "Error should be a JSON parse error, got: {}",
        err_msg
    );
    assert_eq!(backend.call_count(), 2);
}

#[tokio::test]
async fn chat_with_repair_process_error_triggers_retry() {
    // First response is valid JSON but fails the process closure,
    // second response succeeds
    let backend = MockBackend::new(vec![r#"{"status": "draft"}"#, r#"{"status": "final"}"#]);
    let messages = vec![Message::user("generate JSON")];

    let result = chat_with_repair(&backend, "Test", messages, |content| {
        let v: serde_json::Value = serde_json::from_str(content)?;
        if v["status"] == "draft" {
            anyhow::bail!("status must be final");
        }
        Ok(v)
    })
    .await;

    assert!(result.is_ok());
    let v = result.unwrap();
    assert_eq!(v["status"], "final");
    assert_eq!(backend.call_count(), 2);
}

#[tokio::test]
async fn chat_with_repair_process_error_on_all_attempts() {
    // Both responses fail the process closure
    let backend = MockBackend::new(vec![r#"{"status": "draft"}"#, r#"{"status": "draft"}"#]);
    let messages = vec![Message::user("generate JSON")];

    let result: anyhow::Result<serde_json::Value> =
        chat_with_repair(&backend, "Test", messages, |content| {
            let v: serde_json::Value = serde_json::from_str(content)?;
            if v["status"] == "draft" {
                anyhow::bail!("status must be final");
            }
            Ok(v)
        })
        .await;

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("status must be final")
    );
    assert_eq!(backend.call_count(), 2);
}
