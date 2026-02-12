use super::*;
use clickweave_core::Workflow;
use clickweave_core::storage::RunStorage;
use clickweave_llm::{ChatBackend, Content, ContentPart, Message};
use std::path::PathBuf;

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
    ) -> Self {
        let storage = project_path
            .as_ref()
            .map(|p| RunStorage::new(p, workflow.id));
        Self {
            workflow,
            agent,
            vlm,
            mcp_command,
            project_path,
            event_tx,
            storage,
            app_cache: RefCell::new(HashMap::new()),
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
