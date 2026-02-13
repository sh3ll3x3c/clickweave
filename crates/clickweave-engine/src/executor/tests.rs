use super::*;
use clickweave_core::storage::RunStorage;
use clickweave_core::{
    FocusMethod, FocusWindowParams, NodeType, ScreenshotMode, TakeScreenshotParams, Workflow,
};
use clickweave_llm::{ChatBackend, ChatResponse, Content, ContentPart, Message};
use serde_json::Value;
use std::path::PathBuf;

/// A stub ChatBackend that never expects to be called.
/// Useful for tests that only exercise cache mechanics without LLM interaction.
struct StubBackend;

impl ChatBackend for StubBackend {
    fn model_name(&self) -> &str {
        "stub"
    }

    async fn chat(
        &self,
        _messages: Vec<Message>,
        _tools: Option<Vec<Value>>,
    ) -> anyhow::Result<ChatResponse> {
        panic!("StubBackend::chat should not be called in this test");
    }
}

/// Helper to create a `WorkflowExecutor<StubBackend>` with minimal setup.
fn make_test_executor() -> WorkflowExecutor<StubBackend> {
    let (tx, _rx) = tokio::sync::mpsc::channel(16);
    let workflow = Workflow::default();
    let temp_dir = std::env::temp_dir().join("clickweave_test_executor");
    let storage = RunStorage::new_app_data(&temp_dir, &workflow.name, workflow.id);
    WorkflowExecutor::with_backends(
        workflow,
        StubBackend,
        None,
        "stub-mcp".to_string(),
        None,
        tx,
        storage,
    )
}

/// Check that a list of messages contains no image content parts.
fn assert_no_images(messages: &[Message]) {
    for (i, msg) in messages.iter().enumerate() {
        if let Some(Content::Parts(parts)) = &msg.content {
            for part in parts {
                if matches!(part, ContentPart::ImageUrl { .. }) {
                    panic!(
                        "Message[{}] (role={}) contains image content — agent should never receive images when VLM is configured",
                        i, msg.role
                    );
                }
            }
        }
    }
}

impl<C: ChatBackend> WorkflowExecutor<C> {
    pub fn with_backends(
        workflow: Workflow,
        agent: C,
        vlm: Option<C>,
        mcp_command: String,
        project_path: Option<PathBuf>,
        event_tx: Sender<ExecutorEvent>,
        storage: RunStorage,
    ) -> Self {
        Self {
            workflow,
            agent,
            vlm,
            mcp_command,
            project_path,
            event_tx,
            storage,
            app_cache: RwLock::new(HashMap::new()),
            focused_app: RwLock::new(None),
        }
    }
}

#[test]
fn assert_no_images_passes_for_text_only() {
    let messages = vec![
        Message::system("system prompt"),
        Message::user("hello"),
        Message::assistant("world"),
        Message::user("VLM_IMAGE_SUMMARY:\n{\"summary\": \"a screen\"}"),
    ];
    assert_no_images(&messages);
}

#[test]
#[should_panic(expected = "contains image content")]
fn assert_no_images_catches_image_parts() {
    let messages = vec![Message::user_with_images(
        "Here are images",
        vec![("base64".to_string(), "image/png".to_string())],
    )];
    assert_no_images(&messages);
}

#[test]
fn vlm_summary_replaces_images_in_message_flow() {
    use clickweave_llm::workflow_system_prompt;

    // Simulate the message flow when VLM is configured:
    // After tool results, we should append a text VLM_IMAGE_SUMMARY
    // instead of images.
    let mut messages = vec![
        Message::system(workflow_system_prompt()),
        Message::user("Click the login button"),
    ];

    // Simulate: agent made a tool call, got a result with images
    messages.push(Message::tool_result("call_1", "screenshot taken"));

    // VLM analyzed the images and produced a summary
    let vlm_summary = r#"{"summary": "Login page with username/password fields"}"#;
    messages.push(Message::user(format!(
        "VLM_IMAGE_SUMMARY:\n{}",
        vlm_summary
    )));

    // Verify: no images in the agent messages
    assert_no_images(&messages);

    // Verify: the VLM summary is present as plain text
    let last = messages.last().unwrap();
    assert!(matches!(&last.content, Some(Content::Text(t)) if t.contains("VLM_IMAGE_SUMMARY")));
}

// ---------------------------------------------------------------------------
// App cache tests
// ---------------------------------------------------------------------------

#[test]
fn evict_app_cache_removes_entry() {
    let exec = make_test_executor();

    // Insert a resolved app into the cache
    exec.app_cache.write().unwrap().insert(
        "chrome".to_string(),
        ResolvedApp {
            name: "Google Chrome".to_string(),
            pid: 1234,
        },
    );
    assert!(exec.app_cache.read().unwrap().contains_key("chrome"));

    // Evict it
    exec.evict_app_cache("chrome");
    assert!(
        !exec.app_cache.read().unwrap().contains_key("chrome"),
        "cache entry should be removed after eviction"
    );
}

#[test]
fn evict_app_cache_noop_for_missing_key() {
    let exec = make_test_executor();

    // Evicting a key that was never cached should not panic
    exec.evict_app_cache("nonexistent");
    assert!(exec.app_cache.read().unwrap().is_empty());
}

#[test]
fn evict_app_cache_leaves_other_entries() {
    let exec = make_test_executor();

    exec.app_cache.write().unwrap().insert(
        "chrome".to_string(),
        ResolvedApp {
            name: "Google Chrome".to_string(),
            pid: 1234,
        },
    );
    exec.app_cache.write().unwrap().insert(
        "firefox".to_string(),
        ResolvedApp {
            name: "Firefox".to_string(),
            pid: 5678,
        },
    );

    exec.evict_app_cache("chrome");

    assert!(
        !exec.app_cache.read().unwrap().contains_key("chrome"),
        "evicted entry should be gone"
    );
    assert!(
        exec.app_cache.read().unwrap().contains_key("firefox"),
        "other entries should remain"
    );
}

#[test]
fn evict_app_cache_for_focus_window_node() {
    let exec = make_test_executor();
    exec.app_cache.write().unwrap().insert(
        "chrome".to_string(),
        ResolvedApp {
            name: "Google Chrome".to_string(),
            pid: 1234,
        },
    );

    let node = NodeType::FocusWindow(FocusWindowParams {
        method: FocusMethod::AppName,
        value: Some("chrome".to_string()),
        bring_to_front: true,
    });
    exec.evict_app_cache_for_node(&node);
    assert!(!exec.app_cache.read().unwrap().contains_key("chrome"));
}

#[test]
fn evict_app_cache_for_screenshot_node() {
    let exec = make_test_executor();
    exec.app_cache.write().unwrap().insert(
        "safari".to_string(),
        ResolvedApp {
            name: "Safari".to_string(),
            pid: 999,
        },
    );

    let node = NodeType::TakeScreenshot(TakeScreenshotParams {
        mode: ScreenshotMode::Window,
        target: Some("safari".to_string()),
        include_ocr: true,
    });
    exec.evict_app_cache_for_node(&node);
    assert!(!exec.app_cache.read().unwrap().contains_key("safari"));
}

#[test]
fn evict_app_cache_for_unrelated_node_is_noop() {
    let exec = make_test_executor();
    exec.app_cache.write().unwrap().insert(
        "chrome".to_string(),
        ResolvedApp {
            name: "Google Chrome".to_string(),
            pid: 1234,
        },
    );

    let node = NodeType::Click(clickweave_core::ClickParams::default());
    exec.evict_app_cache_for_node(&node);
    assert!(exec.app_cache.read().unwrap().contains_key("chrome"));
}
