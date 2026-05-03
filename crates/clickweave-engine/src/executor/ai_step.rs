use super::Mcp;
use super::retry_context::RetryContext;
use super::{ExecutorError, ExecutorResult, WorkflowExecutor};
use clickweave_core::{AiStepParams, AppKind, NodeRun};
use clickweave_llm::{
    ChatBackend, Message, ToolCall, analyze_images, build_step_prompt, workflow_system_prompt,
};
use serde_json::Value;
use std::time::{Duration, Instant};
use tracing::debug;

#[derive(Default)]
struct AiStepImageBatch {
    pending_images: Vec<(String, String)>,
    last_image_tool: String,
}

impl<C: ChatBackend> WorkflowExecutor<C> {
    pub(crate) async fn execute_ai_step(
        &mut self,
        params: &AiStepParams,
        tools: &[Value],
        mcp: &(impl Mcp + ?Sized),
        timeout_ms: Option<u64>,
        mut node_run: Option<&mut NodeRun>,
        retry_ctx: &mut RetryContext,
    ) -> ExecutorResult<Value> {
        // Clear deterministic tool result so supervision doesn't attribute
        // a previous node's output to this AI step.
        retry_ctx.last_tool_result = None;

        let mut messages = build_ai_step_messages(params);
        let filtered_tools = self.filter_ai_step_tools(params, tools);
        let max_tool_calls = params.max_tool_calls.unwrap_or(10) as usize;
        let step_start = Instant::now();
        let mut tool_call_count = 0;
        let mut last_assistant_text = String::new();

        loop {
            if ai_step_timed_out(step_start, timeout_ms) {
                self.log("Timeout reached");
                break;
            }

            if self.is_cancelled() {
                return Err(ExecutorError::Cancelled);
            }

            let response = self
                .agent
                .chat(&messages, Some(&filtered_tools))
                .await
                .map_err(|e| ExecutorError::Llm(e.to_string()))?;

            let choice = response
                .choices
                .first()
                .ok_or(ExecutorError::Llm("No response from LLM".to_string()))?;

            let msg = &choice.message;

            let Some(tool_calls) = &msg.tool_calls else {
                self.finish_ai_step_without_tool_calls(msg, &mut last_assistant_text);
                break;
            };

            if tool_calls.is_empty() {
                self.finish_ai_step_empty_tool_calls(msg, &mut last_assistant_text);
                break;
            }

            messages.push(Message::assistant_tool_calls(tool_calls.clone()));

            let mut image_batch = AiStepImageBatch::default();

            for tool_call in tool_calls {
                if tool_call_count >= max_tool_calls {
                    self.log("Max tool calls reached mid-response, skipping remaining");
                    break;
                }
                tool_call_count += 1;
                self.execute_ai_step_tool_call(
                    tool_call,
                    tool_call_count - 1,
                    mcp,
                    &mut messages,
                    &mut image_batch,
                    &mut node_run,
                    retry_ctx,
                )
                .await;
            }

            let budget_exhausted = tool_call_count >= max_tool_calls;
            self.append_ai_step_image_context(
                params,
                image_batch,
                &mut messages,
                node_run.as_deref(),
            )
            .await;

            if budget_exhausted {
                self.log("Max tool calls reached");
                break;
            }
        }

        Ok(Value::String(last_assistant_text))
    }

    fn filter_ai_step_tools(&self, params: &AiStepParams, tools: &[Value]) -> Vec<Value> {
        let Some(allowed) = &params.allowed_tools else {
            return tools.to_vec();
        };

        let filtered: Vec<Value> = tools
            .iter()
            .filter(|t| {
                t.pointer("/function/name")
                    .and_then(|n| n.as_str())
                    .is_some_and(|name| allowed.iter().any(|a| a == name))
            })
            .cloned()
            .collect();
        self.log(format!(
            "Filtered tools: {}/{} allowed",
            filtered.len(),
            tools.len()
        ));
        filtered
    }

    fn finish_ai_step_without_tool_calls(&self, msg: &Message, last_assistant_text: &mut String) {
        if let Some(content) = msg.content_text() {
            *last_assistant_text = content.to_string();
            let completed = self.check_step_complete(content);
            self.log(if completed {
                "Step completed"
            } else {
                "Step finished (no tool calls)"
            });
        } else {
            self.log("Step finished (no tool calls)");
        }
    }

    fn finish_ai_step_empty_tool_calls(&self, msg: &Message, last_assistant_text: &mut String) {
        if let Some(content) = msg.content_text() {
            *last_assistant_text = content.to_string();
            if self.check_step_complete(content) {
                self.log("Step completed");
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_ai_step_tool_call(
        &mut self,
        tool_call: &ToolCall,
        tool_call_index: usize,
        mcp: &(impl Mcp + ?Sized),
        messages: &mut Vec<Message>,
        image_batch: &mut AiStepImageBatch,
        node_run: &mut Option<&mut NodeRun>,
        retry_ctx: &mut RetryContext,
    ) {
        self.log(format!("Tool call: {}", tool_call.function.name));
        debug!(
            tool = %tool_call.function.name,
            arguments = %tool_call.function.arguments,
            "Tool call arguments"
        );

        let Some(args) = self.ai_step_tool_args(tool_call, messages) else {
            return;
        };
        let args = self.resolve_image_paths(args);
        let tool_app_name = app_name_from_tool_args(&args);

        self.record_event(
            node_run.as_deref(),
            "tool_call",
            serde_json::json!({
                "name": tool_call.function.name,
                "index": tool_call_index,
                "args": args,
            }),
        );

        match mcp.call_tool(&tool_call.function.name, args).await {
            Ok(result) => {
                self.handle_ai_step_tool_success(
                    tool_call,
                    tool_call_index,
                    result,
                    messages,
                    image_batch,
                    node_run,
                    tool_app_name.as_deref(),
                    retry_ctx,
                );
            }
            Err(e) => {
                self.log(format!("Tool call failed: {}", e));
                messages.push(Message::tool_result(&tool_call.id, format!("Error: {}", e)));
            }
        }
    }

    fn ai_step_tool_args(
        &self,
        tool_call: &ToolCall,
        messages: &mut Vec<Message>,
    ) -> Option<Option<Value>> {
        match &tool_call.function.arguments {
            Value::String(raw) => {
                self.log(format!(
                    "Malformed tool call arguments for {}: {} — skipping",
                    tool_call.function.name, raw
                ));
                messages.push(Message::tool_result(
                    &tool_call.id,
                    format!("Error: invalid arguments — {}", raw),
                ));
                None
            }
            Value::Null => Some(None),
            other => Some(Some(other.clone())),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_ai_step_tool_success(
        &mut self,
        tool_call: &ToolCall,
        tool_call_index: usize,
        result: clickweave_mcp::ToolCallResult,
        messages: &mut Vec<Message>,
        image_batch: &mut AiStepImageBatch,
        node_run: &mut Option<&mut NodeRun>,
        tool_app_name: Option<&str>,
        retry_ctx: &mut RetryContext,
    ) {
        let prefix = format!("toolcall_{tool_call_index}");
        let images = self.save_result_images(&result, &prefix, node_run);
        let tool_image_count = images.len();
        if !images.is_empty() {
            image_batch.last_image_tool = tool_call.function.name.clone();
        }
        image_batch.pending_images.extend(images);

        let result_text = crate::cdp_lifecycle::extract_text(&result);
        self.log(format!(
            "Tool result ({} chars, {} images): {}",
            result_text.chars().count(),
            tool_image_count,
            Self::preview_for_log(&result_text, 300)
        ));
        debug!(
            tool = %tool_call.function.name,
            result = %result_text,
            "Tool result text"
        );

        self.record_event(
            node_run.as_deref(),
            "tool_result",
            serde_json::json!({
                "name": tool_call.function.name,
                "text": Self::truncate_for_trace(&result_text, 8192),
                "text_len": result_text.len(),
                "image_count": tool_image_count,
            }),
        );

        if result.is_error != Some(true) {
            self.apply_ai_step_focus_side_effect(
                &tool_call.function.name,
                tool_app_name,
                retry_ctx,
            );
        }

        messages.push(Message::tool_result(&tool_call.id, result_text));
    }

    fn apply_ai_step_focus_side_effect(
        &mut self,
        tool_name: &str,
        tool_app_name: Option<&str>,
        retry_ctx: &mut RetryContext,
    ) {
        match tool_name {
            "focus_window" | "launch_app" => {
                if let Some(app) = tool_app_name {
                    *self.write_focused_app() = Some((app.to_string(), AppKind::Native, 0));
                    retry_ctx.focus_dirty = true;
                }
            }
            "quit_app" => {
                if let Some(app) = tool_app_name {
                    if self.focused_app_name().as_deref() == Some(app) {
                        *self.write_focused_app() = None;
                        retry_ctx.focus_dirty = true;
                    }
                    self.cdp_state.mark_app_quit(app);
                }
            }
            _ => {}
        }
    }

    async fn append_ai_step_image_context(
        &mut self,
        params: &AiStepParams,
        image_batch: AiStepImageBatch,
        messages: &mut Vec<Message>,
        node_run: Option<&NodeRun>,
    ) {
        if image_batch.pending_images.is_empty() {
            return;
        }

        let image_count = image_batch.pending_images.len();
        let prepared_images: Vec<(String, String)> = image_batch
            .pending_images
            .into_iter()
            .filter_map(|(b64, _mime)| {
                clickweave_llm::prepare_base64_image_for_vlm(
                    &b64,
                    clickweave_llm::DEFAULT_MAX_DIMENSION,
                )
            })
            .collect();

        if prepared_images.is_empty() {
            self.log(format!(
                "Failed to prepare {} image(s) for VLM",
                image_count
            ));
        } else if let Some(vlm) = self.vision_backend() {
            let vlm_model = vlm.model_name().to_string();
            self.log(format!(
                "Analyzing {} image(s) with VLM ({})",
                image_count, vlm_model
            ));
            match analyze_images(
                vlm,
                &params.prompt,
                &image_batch.last_image_tool,
                prepared_images,
            )
            .await
            {
                Ok(summary) => {
                    self.record_event(
                        node_run,
                        "vision_summary",
                        serde_json::json!({
                            "image_count": image_count,
                            "vlm_model": vlm_model,
                            "summary_json": summary,
                        }),
                    );
                    messages.push(Message::user(format!("VLM_IMAGE_SUMMARY:\n{}", summary)));
                }
                Err(e) => {
                    self.log(format!("VLM analysis failed: {}", e));
                    messages.push(Message::user(
                        "(Vision analysis failed; consider using find_text or find_image for precise targeting)"
                            .to_string(),
                    ));
                }
            }
        } else {
            messages.push(Message::user_with_images(
                "Here are the images from the tool results above.",
                prepared_images,
            ));
        }
    }
}

fn build_ai_step_messages(params: &AiStepParams) -> Vec<Message> {
    vec![
        Message::system(workflow_system_prompt()),
        Message::user(build_step_prompt(
            &params.prompt,
            params.button_text.as_deref(),
            params.template_image.as_deref(),
        )),
    ]
}

fn ai_step_timed_out(step_start: Instant, timeout_ms: Option<u64>) -> bool {
    timeout_ms.is_some_and(|timeout| step_start.elapsed() > Duration::from_millis(timeout))
}

fn app_name_from_tool_args(args: &Option<Value>) -> Option<String> {
    args.as_ref()
        .and_then(|a| a.get("app_name"))
        .and_then(|v| v.as_str())
        .map(String::from)
}
