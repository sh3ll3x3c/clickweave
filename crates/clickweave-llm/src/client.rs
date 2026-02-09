use crate::types::*;
use anyhow::{Context, Result};
use serde_json::Value;
use std::future::Future;
use tracing::{debug, info};

/// Seam for LLM interaction, allowing mock backends in tests.
pub trait ChatBackend: Send + Sync {
    fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> impl Future<Output = Result<ChatResponse>> + Send;

    fn model_name(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            // LM Studio default
            base_url: "http://localhost:1234/v1".to_string(),
            api_key: None,
            model: "local-model".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(4096),
        }
    }
}

pub struct LlmClient {
    config: LlmConfig,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    pub fn config(&self) -> &LlmConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut LlmConfig {
        &mut self.config
    }
}

impl ChatBackend for LlmClient {
    fn model_name(&self) -> &str {
        &self.config.model
    }

    async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<ChatResponse> {
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages,
            tools,
            tool_choice: None,
            temperature: self.config.temperature,
            max_tokens: self.config.max_tokens,
        };

        debug!("LLM request to {}: {:?}", url, request.messages.len());

        let mut req_builder = self.http.post(&url).json(&request);

        if let Some(api_key) = &self.config.api_key {
            req_builder = req_builder.bearer_auth(api_key);
        }

        let response = req_builder
            .send()
            .await
            .context("Failed to send request to LLM")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("LLM request failed ({}): {}", status, error_text);
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .context("Failed to parse LLM response")?;

        info!(
            "LLM response: finish_reason={:?}, tool_calls={:?}",
            chat_response
                .choices
                .first()
                .and_then(|c| c.finish_reason.as_ref()),
            chat_response.choices.first().and_then(|c| c
                .message
                .tool_calls
                .as_ref()
                .map(|tc| tc.len()))
        );

        Ok(chat_response)
    }
}

/// System prompt for the orchestrator (text-only, no images).
pub fn workflow_system_prompt() -> String {
    r#"You are a UI automation assistant executing an AI Step node within a workflow.

You have access to MCP tools for native UI interaction:
- take_screenshot: capture the screen, a window, or a region (optionally with OCR)
- find_text: locate text on screen using OCR
- find_image: template-match an image on screen
- click: click at coordinates or on an element
- type_text: type text at the cursor
- scroll: scroll at a position
- list_windows / focus_window: manage windows

For each step, you will receive:
- A prompt describing the objective
- Optional button_text: specific text to find and click
- Optional template_image: path to an image to locate on screen

Image outputs from tools are analyzed by a separate vision model. You will receive
their analysis as a VLM_IMAGE_SUMMARY message containing a JSON object with:
- summary: what is visible on screen
- visible_text: key labels, buttons, headings
- alerts: errors, popups, permission prompts
- notes_for_orchestrator: non-prescriptive hints

Use find_text / find_image for precise coordinate targeting. Do not guess coordinates.

Strategy:
1. Start by taking a screenshot to observe the current state
2. Use find_text or find_image to locate targets precisely
3. Perform the required input actions (click, type, scroll)
4. Verify the result with another screenshot if needed

When you have completed the step's objective, respond with a JSON object:
{"step_complete": true, "summary": "Brief description of what was done"}

If you cannot complete the step:
{"step_complete": false, "error": "Description of the problem"}

Be precise with coordinates. Always verify actions when the outcome matters."#
        .to_string()
}

/// System prompt for the VLM (vision model).
pub fn vlm_system_prompt() -> String {
    r#"You are a visual analyst for UI automation. You receive screenshots and images from tool results and produce structured descriptions for an orchestrator model that cannot see images.

Output ONLY a JSON object with these fields:
{
  "summary": "1-3 sentences describing what is visible on screen",
  "visible_text": ["key labels", "button text", "dialog headings"],
  "alerts": ["any errors", "popups", "permission prompts"],
  "notes_for_orchestrator": "Non-prescriptive hints, e.g. 'There is a modal blocking the UI' or 'The search field is focused'"
}

Rules:
- Be factual and concise. Describe what you see, not what to do.
- Include coordinates only if they are clearly visible (e.g. OCR overlay).
- Do NOT suggest actions or next steps — the orchestrator decides.
- If nothing notable is on screen, keep fields empty but still return valid JSON."#
        .to_string()
}

/// Build the user prompt for a VLM image analysis call.
pub fn build_vlm_prompt(step_goal: &str, tool_name: &str) -> String {
    format!(
        "The orchestrator is working on: \"{}\"\n\
         The following image(s) were returned by the \"{}\" tool.\n\
         Analyze the image(s) and produce the JSON summary.",
        step_goal, tool_name
    )
}

/// Call the VLM to analyze images and return a text summary.
pub async fn analyze_images(
    vlm: &(impl ChatBackend + ?Sized),
    step_goal: &str,
    tool_name: &str,
    images: Vec<(String, String)>,
) -> Result<String> {
    let messages = vec![
        Message::system(vlm_system_prompt()),
        Message::user_with_images(build_vlm_prompt(step_goal, tool_name), images),
    ];

    let response = vlm.chat(messages, None).await?;

    let text = response
        .choices
        .first()
        .and_then(|c| c.message.content_text())
        .unwrap_or("")
        .to_string();

    Ok(text)
}

/// Build user message for a workflow step
pub fn build_step_prompt(
    prompt: &str,
    button_text: Option<&str>,
    image_path: Option<&str>,
) -> String {
    let mut parts = vec![prompt.to_string()];

    if let Some(text) = button_text {
        parts.push(format!("\nButton to find: \"{}\"", text));
    }

    if let Some(path) = image_path {
        parts.push(format!("\nImage to find: {}", path));
    }

    parts.join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Mock backend that records calls and returns a canned response.
    struct MockBackend {
        response_text: String,
        calls: Mutex<Vec<Vec<Message>>>,
    }

    impl MockBackend {
        fn new(response_text: &str) -> Self {
            Self {
                response_text: response_text.to_string(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }

        fn last_messages(&self) -> Vec<Message> {
            self.calls
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl ChatBackend for MockBackend {
        fn model_name(&self) -> &str {
            "mock-model"
        }

        async fn chat(
            &self,
            messages: Vec<Message>,
            _tools: Option<Vec<Value>>,
        ) -> Result<ChatResponse> {
            self.calls.lock().unwrap().push(messages);
            Ok(ChatResponse {
                id: "mock".to_string(),
                choices: vec![Choice {
                    index: 0,
                    message: Message::assistant(&self.response_text),
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
            })
        }
    }

    #[test]
    fn vlm_system_prompt_requests_json() {
        let prompt = vlm_system_prompt();
        assert!(
            prompt.contains("JSON"),
            "VLM prompt should request JSON output"
        );
        assert!(
            prompt.contains("summary"),
            "VLM prompt should mention summary field"
        );
        assert!(
            prompt.contains("visible_text"),
            "VLM prompt should mention visible_text field"
        );
        assert!(
            prompt.contains("alerts"),
            "VLM prompt should mention alerts field"
        );
        assert!(
            prompt.contains("notes_for_orchestrator"),
            "VLM prompt should mention notes_for_orchestrator field"
        );
    }

    #[test]
    fn vlm_prompt_is_non_prescriptive() {
        let prompt = vlm_system_prompt();
        assert!(
            prompt.contains("Do NOT suggest actions"),
            "VLM prompt should forbid suggesting actions"
        );
    }

    #[test]
    fn orchestrator_prompt_mentions_vlm_summary() {
        let prompt = workflow_system_prompt();
        assert!(
            prompt.contains("VLM_IMAGE_SUMMARY"),
            "Orchestrator prompt should describe VLM summary format"
        );
    }

    #[test]
    fn build_vlm_prompt_includes_context() {
        let prompt = build_vlm_prompt("click the login button", "take_screenshot");
        assert!(prompt.contains("click the login button"));
        assert!(prompt.contains("take_screenshot"));
    }

    #[tokio::test]
    async fn analyze_images_returns_vlm_text() {
        let mock = MockBackend::new(r#"{"summary": "a login screen"}"#);
        let result = analyze_images(
            &mock,
            "click the login button",
            "take_screenshot",
            vec![("base64data".to_string(), "image/png".to_string())],
        )
        .await
        .unwrap();

        assert_eq!(result, r#"{"summary": "a login screen"}"#);
        assert_eq!(mock.call_count(), 1);

        // Verify the VLM received the system prompt + user message with images
        let messages = mock.last_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages[1].role, "user");
        // User message should contain image parts
        assert!(
            matches!(&messages[1].content, Some(Content::Parts(parts)) if parts.len() >= 2),
            "VLM user message should contain text + image parts"
        );
    }
}
