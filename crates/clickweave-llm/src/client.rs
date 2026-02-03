use crate::types::*;
use anyhow::{Context, Result};
use serde_json::Value;
use tracing::{debug, info};

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

    pub async fn chat(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.config.base_url);

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

/// System prompt for workflow execution
pub fn workflow_system_prompt() -> String {
    r#"You are a UI automation assistant. You execute workflow steps by using the available tools.

For each step, you will receive:
- A prompt describing what action to take
- Optional button_text: text to find and click
- Optional image_path: path to an image to find on screen

Use the MCP tools to:
1. Take screenshots to see the current state
2. Find text or images on screen
3. Click, type, or scroll as needed

When you have completed the step's objective, respond with a JSON object:
{"step_complete": true, "summary": "Brief description of what was done"}

If you encounter an error or cannot complete the step:
{"step_complete": false, "error": "Description of the problem"}

Be precise with coordinates and verify actions by taking screenshots."#
        .to_string()
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
