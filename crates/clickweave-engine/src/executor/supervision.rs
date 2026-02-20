use super::WorkflowExecutor;
use clickweave_core::NodeType;
use clickweave_llm::{ChatBackend, Content, ContentPart, ImageUrl, Message};
use clickweave_mcp::{McpClient, ToolContent};
use serde_json::Value;
use tracing::debug;

/// Result of LLM step verification.
pub(crate) struct VerificationResult {
    pub passed: bool,
    pub reasoning: String,
    /// Path to the saved screenshot artifact, if captured.
    pub screenshot_path: Option<String>,
}

impl<C: ChatBackend> WorkflowExecutor<C> {
    /// Take a screenshot and ask the LLM whether the step achieved its intent.
    pub(crate) async fn verify_step(
        &self,
        node_name: &str,
        node_type: &NodeType,
        mcp: &McpClient,
    ) -> VerificationResult {
        // Skip verification for steps with no observable effect
        if matches!(node_type, NodeType::TakeScreenshot(_)) {
            return VerificationResult {
                passed: true,
                reasoning: "Screenshot steps are not verified".to_string(),
                screenshot_path: None,
            };
        }

        debug!(node_name = node_name, "verifying step via screenshot");

        let action = node_type.action_description();
        let app_name = self
            .focused_app
            .read()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| "unknown".to_string());

        // Capture screenshot
        let screenshot_data = self.capture_verification_screenshot(mcp).await;
        let Some(image_base64) = screenshot_data else {
            // If screenshot fails, assume pass — don't block on capture failures
            self.log("Supervision: screenshot capture failed, assuming pass".to_string());
            return VerificationResult {
                passed: true,
                reasoning: "Could not capture screenshot for verification".to_string(),
                screenshot_path: None,
            };
        };

        // Build verification prompt
        let prompt = format!(
            "You are verifying a UI automation step.\n\
             \n\
             Step: \"{}\" — {}\n\
             App: {}\n\
             \n\
             The step has executed. Look at the screenshot and determine if it worked correctly.\n\
             \n\
             Return ONLY a JSON object:\n\
             {{\"passed\": true/false, \"reasoning\": \"brief explanation\"}}",
            node_name, action, app_name
        );

        let messages = vec![Message {
            role: "user".to_string(),
            content: Some(Content::Parts(vec![
                ContentPart::Text { text: prompt },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: format!("data:image/png;base64,{}", image_base64),
                    },
                },
            ])),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }];

        // Prefer VLM for image understanding, fall back to agent
        let backend = self.vlm.as_ref().unwrap_or(&self.agent);

        let result = match backend.chat(messages, None).await {
            Ok(response) => {
                let raw = response
                    .choices
                    .first()
                    .and_then(|c| c.message.content_text())
                    .unwrap_or("");
                parse_verification_response(raw)
            }
            Err(e) => {
                self.log(format!("Supervision: LLM verification failed: {}", e));
                // On LLM error, assume pass — don't block execution
                (true, format!("LLM verification error: {}", e))
            }
        };

        self.log(format!(
            "Supervision: {} — {} ({})",
            node_name,
            if result.0 { "PASSED" } else { "FAILED" },
            result.1
        ));

        VerificationResult {
            passed: result.0,
            reasoning: result.1,
            screenshot_path: None,
        }
    }

    /// Capture a screenshot for verification. Returns base64-encoded image data.
    async fn capture_verification_screenshot(&self, mcp: &McpClient) -> Option<String> {
        let app_name = self.focused_app.read().ok().and_then(|g| g.clone());
        let mut args = serde_json::json!({ "format": "png" });
        if let Some(ref name) = app_name {
            args["app_name"] = Value::String(name.clone());
        }

        let result = mcp.call_tool("take_screenshot", Some(args)).await.ok()?;
        for content in &result.content {
            if let ToolContent::Image { data, .. } = content {
                return Some(data.clone());
            }
        }
        None
    }
}

/// Parse the LLM's JSON verification response. Returns (passed, reasoning).
fn parse_verification_response(raw: &str) -> (bool, String) {
    let text = super::app_resolve::strip_code_block(raw);
    let json_text = super::app_resolve::extract_json_object(text);

    if let Some(json_str) = json_text
        && let Ok(parsed) = serde_json::from_str::<Value>(json_str)
    {
        let passed = parsed["passed"].as_bool().unwrap_or(true);
        let reasoning = parsed["reasoning"]
            .as_str()
            .unwrap_or("no reasoning provided")
            .to_string();
        return (passed, reasoning);
    }

    // If we can't parse, assume pass
    (
        true,
        format!("Could not parse verification response: {}", raw),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_verification_pass() {
        let (passed, reasoning) = parse_verification_response(
            r#"{"passed": true, "reasoning": "Button 2 is highlighted"}"#,
        );
        assert!(passed);
        assert!(reasoning.contains("highlighted"));
    }

    #[test]
    fn parse_verification_fail() {
        let (passed, reasoning) = parse_verification_response(
            r#"{"passed": false, "reasoning": "Display still shows 0"}"#,
        );
        assert!(!passed);
        assert!(reasoning.contains("still shows 0"));
    }

    #[test]
    fn parse_verification_code_block() {
        let (passed, _) =
            parse_verification_response("```json\n{\"passed\": true, \"reasoning\": \"ok\"}\n```");
        assert!(passed);
    }

    #[test]
    fn parse_verification_malformed_assumes_pass() {
        let (passed, _) = parse_verification_response("I think it worked fine");
        assert!(passed);
    }
}
