use super::{ExecutorResult, Mcp, WorkflowExecutor};
use clickweave_core::output_schema::VerificationMethod;
use clickweave_core::{NodeRun, NodeType};
use clickweave_llm::ChatBackend;
use serde_json::Value;

/// Extract verification config from any action node type.
/// Returns Some((method, assertion)) only if both are set.
pub(crate) fn extract_verification_config(
    node_type: &NodeType,
) -> Option<(VerificationMethod, String)> {
    macro_rules! check {
        ($p:expr) => {
            if let (Some(method), Some(assertion)) =
                (&$p.verification_method, &$p.verification_assertion)
            {
                return Some((*method, assertion.clone()));
            }
        };
    }
    match node_type {
        NodeType::Click(p) => check!(p),
        NodeType::Hover(p) => check!(p),
        NodeType::TypeText(p) => check!(p),
        NodeType::PressKey(p) => check!(p),
        NodeType::Scroll(p) => check!(p),
        NodeType::FocusWindow(p) => check!(p),
        NodeType::Drag(p) => check!(p),
        NodeType::LaunchApp(p) => check!(p),
        NodeType::QuitApp(p) => check!(p),
        NodeType::CdpClick(p) => check!(p),
        NodeType::CdpHover(p) => check!(p),
        NodeType::CdpFill(p) => check!(p),
        NodeType::CdpType(p) => check!(p),
        NodeType::CdpPressKey(p) => check!(p),
        NodeType::CdpNavigate(p) => check!(p),
        NodeType::CdpNewPage(p) => check!(p),
        NodeType::CdpClosePage(p) => check!(p),
        NodeType::CdpSelectPage(p) => check!(p),
        NodeType::CdpHandleDialog(p) => check!(p),
        _ => {}
    }
    None
}

impl<C: ChatBackend> WorkflowExecutor<C> {
    /// Run post-action VLM verification if the node has verification enabled.
    /// Stores `verified` and `verification_reasoning` in RuntimeContext.
    pub(crate) async fn run_action_verification(
        &mut self,
        auto_id: &str,
        method: &VerificationMethod,
        assertion: &str,
        mcp: &(impl Mcp + ?Sized),
        node_run: Option<&NodeRun>,
    ) -> ExecutorResult<()> {
        match method {
            VerificationMethod::Vlm => {
                self.log(format!("Running VLM verification for {}", auto_id));

                // Wait for UI to settle after action
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;

                let screenshot_result = mcp
                    .call_tool(
                        "take_screenshot",
                        Some(serde_json::json!({"mode": "screen", "include_ocr": false})),
                    )
                    .await;

                let (verified, reasoning) = match screenshot_result {
                    Ok(result) => {
                        let _screenshot_text = Self::extract_result_text(&result);
                        match &self.verdict_vlm {
                            Some(_vlm) => {
                                // VLM verification not yet fully wired — follows supervision.rs pattern
                                (false, "VLM verification not yet fully wired".to_string())
                            }
                            None => (false, "No VLM configured for verification".to_string()),
                        }
                    }
                    Err(e) => (false, format!("Screenshot failed: {}", e)),
                };

                self.context
                    .set_variable(format!("{}.verified", auto_id), Value::Bool(verified));
                self.context.set_variable(
                    format!("{}.verification_reasoning", auto_id),
                    Value::String(reasoning.clone()),
                );

                self.record_event(
                    node_run,
                    "action_verification",
                    serde_json::json!({
                        "auto_id": auto_id,
                        "verified": verified,
                        "reasoning": reasoning,
                    }),
                );

                // Suppress unused variable warning — assertion will be used
                // once VLM verification is fully wired.
                let _ = assertion;

                Ok(())
            }
        }
    }
}
