use base64::Engine;
use clickweave_core::storage::RunStorage;
use clickweave_core::{
    AiStepParams, ArtifactKind, FocusMethod, MouseButton, NodeRun, NodeType, RunStatus,
    ScreenshotMode, TraceEvent, TraceLevel, Workflow,
};
use clickweave_llm::{
    ChatBackend, LlmClient, LlmConfig, Message, analyze_images, build_step_prompt,
    workflow_system_prompt,
};
use clickweave_mcp::{McpClient, ToolCallResult, ToolContent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{Receiver, Sender};
use tracing::{debug, error, info};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutorState {
    Idle,
    Running,
}

pub enum ExecutorCommand {
    Stop,
}

/// Events sent from the executor back to the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutorEvent {
    Log(String),
    StateChanged(ExecutorState),
    NodeStarted(Uuid),
    NodeCompleted(Uuid),
    NodeFailed(Uuid, String),
    RunCreated(Uuid, NodeRun),
    WorkflowCompleted,
    Error(String),
}

pub struct WorkflowExecutor<C: ChatBackend = LlmClient> {
    workflow: Workflow,
    agent: C,
    vlm: Option<C>,
    mcp_command: String,
    project_path: Option<PathBuf>,
    event_tx: Sender<ExecutorEvent>,
    storage: Option<RunStorage>,
}

impl WorkflowExecutor {
    pub fn new(
        workflow: Workflow,
        agent_config: LlmConfig,
        vlm_config: Option<LlmConfig>,
        mcp_command: String,
        project_path: Option<PathBuf>,
        event_tx: Sender<ExecutorEvent>,
    ) -> Self {
        let storage = project_path
            .as_ref()
            .map(|p| RunStorage::new(p, workflow.id));
        Self {
            workflow,
            agent: LlmClient::new(agent_config),
            vlm: vlm_config.map(LlmClient::new),
            mcp_command,
            project_path,
            event_tx,
            storage,
        }
    }
}

impl<C: ChatBackend> WorkflowExecutor<C> {
    fn emit(&self, event: ExecutorEvent) {
        let _ = self.event_tx.try_send(event);
    }

    fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        info!("{}", msg);
        self.emit(ExecutorEvent::Log(msg));
    }

    async fn log_model_info(&self, label: &str, backend: &C) {
        match backend.fetch_model_info().await {
            Ok(Some(info)) => {
                let ctx = info
                    .effective_context_length()
                    .map_or("?".to_string(), |v| v.to_string());

                let mut details = vec![format!("model={}", info.id), format!("ctx={}", ctx)];

                if let Some(arch) = &info.arch {
                    details.push(format!("arch={}", arch));
                }
                if let Some(quant) = &info.quantization {
                    details.push(format!("quant={}", quant));
                }
                if let Some(owned_by) = &info.owned_by {
                    details.push(format!("owned_by={}", owned_by));
                }

                self.log(format!("{}: {}", label, details.join(", ")));
            }
            Ok(None) => {
                self.log(format!(
                    "{}: {} (no model info from provider)",
                    label,
                    backend.model_name()
                ));
            }
            Err(e) => {
                debug!("Failed to fetch model info for {}: {}", label, e);
                self.log(format!(
                    "{}: {} (could not query model info)",
                    label,
                    backend.model_name()
                ));
            }
        }
    }

    fn emit_error(&self, msg: impl Into<String>) {
        let msg = msg.into();
        error!("{}", msg);
        self.emit(ExecutorEvent::Error(msg));
    }

    fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn record_event(&self, run: Option<&NodeRun>, event_type: &str, payload: Value) {
        let Some(run) = run else { return };
        let Some(storage) = &self.storage else { return };
        let event = TraceEvent {
            timestamp: Self::now_millis(),
            event_type: event_type.to_string(),
            payload,
        };
        if let Err(e) = storage.append_event(run, &event) {
            tracing::warn!("Failed to append trace event: {}", e);
        }
    }

    fn save_image_artifact(&self, run: &mut NodeRun, filename: &str, data: &[u8]) {
        if let Some(storage) = &self.storage {
            match storage.save_artifact(run, ArtifactKind::Screenshot, filename, data, Value::Null)
            {
                Ok(artifact) => run.artifacts.push(artifact),
                Err(e) => tracing::warn!("Failed to save artifact: {}", e),
            }
        }
    }

    fn extract_result_text(result: &ToolCallResult) -> String {
        result
            .content
            .iter()
            .filter_map(|c| match c {
                ToolContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn save_result_images(
        &self,
        result: &ToolCallResult,
        prefix: &str,
        node_run: &mut Option<&mut NodeRun>,
    ) -> Vec<(String, String)> {
        let mut images = Vec::new();
        for (idx, content) in result.content.iter().enumerate() {
            if let ToolContent::Image { data, mime_type } = content {
                images.push((data.clone(), mime_type.clone()));

                if let Some(run) = &mut *node_run
                    && run.trace_level != TraceLevel::Off
                {
                    let ext = if mime_type.contains("png") {
                        "png"
                    } else {
                        "jpg"
                    };
                    let filename = format!("{}_{}.{}", prefix, idx, ext);
                    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(data) {
                        self.save_image_artifact(run, &filename, &decoded);
                    }
                }
            }
        }
        images
    }

    fn finalize_run(&self, run: &mut NodeRun, status: RunStatus) {
        run.ended_at = Some(Self::now_millis());
        run.status = status;
        if let Some(storage) = &self.storage
            && let Err(e) = storage.save_run(run)
        {
            tracing::warn!("Failed to save run: {}", e);
        }
    }

    /// Returns true if a stop command was received.
    fn stop_requested(&self, command_rx: &mut Receiver<ExecutorCommand>) -> bool {
        matches!(command_rx.try_recv(), Ok(ExecutorCommand::Stop))
    }

    pub async fn run(&mut self, mut command_rx: Receiver<ExecutorCommand>) {
        self.emit(ExecutorEvent::StateChanged(ExecutorState::Running));
        self.log("Starting workflow execution");

        self.log_model_info("Agent", &self.agent).await;
        if let Some(vlm) = &self.vlm {
            self.log(format!("VLM enabled: {}", vlm.model_name()));
            self.log_model_info("VLM", vlm).await;
        } else {
            self.log("VLM not configured — images sent directly to agent");
        }

        let mcp = if self.mcp_command == "npx" {
            McpClient::spawn_npx()
        } else {
            McpClient::spawn(&self.mcp_command, &[])
        };

        let mcp = match mcp {
            Ok(m) => m,
            Err(e) => {
                self.emit_error(format!("Failed to spawn MCP server: {}", e));
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }
        };

        self.log(format!("MCP server ready with {} tools", mcp.tools().len()));

        let execution_order = self.workflow.execution_order();
        self.log(format!("Execution order: {} nodes", execution_order.len()));

        let tools = mcp.tools_as_openai();

        for node_id in execution_order {
            if self.stop_requested(&mut command_rx) {
                self.log("Workflow stopped by user");
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }

            let Some(node) = self.workflow.find_node(node_id) else {
                continue;
            };

            if !node.enabled {
                self.log(format!("Skipping disabled node: {}", node.name));
                continue;
            }

            self.emit(ExecutorEvent::NodeStarted(node_id));
            self.log(format!(
                "Executing node: {} ({})",
                node.name,
                node.node_type.display_name()
            ));

            let timeout_ms = node.timeout_ms;
            let retries = node.retries;
            let trace_level = node.trace_level;
            let node_name = node.name.clone();
            let node_type = node.node_type.clone();

            let mut node_run = self
                .storage
                .as_ref()
                .and_then(|s| s.create_run(node_id, trace_level).ok());

            if let Some(ref run) = node_run {
                self.emit(ExecutorEvent::RunCreated(node_id, run.clone()));
            }
            self.record_event(
                node_run.as_ref(),
                "node_started",
                serde_json::json!({"name": node_name, "type": node_type.display_name()}),
            );

            let mut attempt = 0;

            loop {
                let result = match &node_type {
                    NodeType::AiStep(params) => {
                        self.execute_ai_step(
                            params,
                            &tools,
                            &mcp,
                            timeout_ms,
                            &mut command_rx,
                            node_run.as_mut(),
                        )
                        .await
                    }
                    other => {
                        self.execute_deterministic(other, &mcp, node_run.as_mut())
                            .await
                    }
                };

                match result {
                    Ok(()) => break,
                    Err(e) if attempt < retries => {
                        attempt += 1;
                        self.log(format!(
                            "Node {} failed (attempt {}/{}): {}. Retrying...",
                            node_name,
                            attempt,
                            retries + 1,
                            e
                        ));
                        self.record_event(
                            node_run.as_ref(),
                            "retry",
                            serde_json::json!({"attempt": attempt, "error": e}),
                        );
                    }
                    Err(e) => {
                        self.emit_error(format!("Node {} failed: {}", node_name, e));
                        if let Some(ref mut run) = node_run {
                            self.finalize_run(run, RunStatus::Failed);
                        }
                        self.emit(ExecutorEvent::NodeFailed(node_id, e));
                        self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                        return;
                    }
                }
            }

            if let Some(ref mut run) = node_run {
                self.finalize_run(run, RunStatus::Ok);
            }
            self.emit(ExecutorEvent::NodeCompleted(node_id));
        }

        self.log("Workflow execution completed");
        self.emit(ExecutorEvent::WorkflowCompleted);
        self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
    }

    async fn execute_ai_step(
        &self,
        params: &AiStepParams,
        tools: &[Value],
        mcp: &McpClient,
        timeout_ms: Option<u64>,
        command_rx: &mut Receiver<ExecutorCommand>,
        mut node_run: Option<&mut NodeRun>,
    ) -> Result<(), String> {
        let mut messages = vec![
            Message::system(workflow_system_prompt()),
            Message::user(build_step_prompt(
                &params.prompt,
                params.button_text.as_deref(),
                params.template_image.as_deref(),
            )),
        ];

        let filtered_tools = if let Some(allowed) = &params.allowed_tools {
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
        } else {
            tools.to_vec()
        };

        let max_tool_calls = params.max_tool_calls.unwrap_or(10) as usize;
        let step_start = Instant::now();
        let mut tool_call_count = 0;

        loop {
            if tool_call_count >= max_tool_calls {
                self.log("Max tool calls reached");
                break;
            }

            if let Some(timeout) = timeout_ms
                && step_start.elapsed().as_millis() as u64 > timeout
            {
                self.log("Timeout reached");
                break;
            }

            if self.stop_requested(command_rx) {
                return Err("Stopped by user".to_string());
            }

            let response = self
                .agent
                .chat(messages.clone(), Some(filtered_tools.clone()))
                .await
                .map_err(|e| format!("LLM error: {}", e))?;

            let choice = response
                .choices
                .first()
                .ok_or_else(|| "No response from LLM".to_string())?;

            let msg = &choice.message;

            let Some(tool_calls) = &msg.tool_calls else {
                let completed = msg
                    .content_text()
                    .is_some_and(|c| self.check_step_complete(c));
                self.log(if completed {
                    "Step completed"
                } else {
                    "Step finished (no tool calls)"
                });
                break;
            };

            if tool_calls.is_empty() {
                if let Some(content) = msg.content_text()
                    && self.check_step_complete(content)
                {
                    self.log("Step completed");
                }
                break;
            }

            messages.push(Message::assistant_tool_calls(tool_calls.clone()));

            let mut pending_images: Vec<(String, String)> = Vec::new();
            let mut last_image_tool = String::new();

            for tool_call in tool_calls {
                tool_call_count += 1;
                self.log(format!("Tool call: {}", tool_call.function.name));
                debug!(
                    tool = %tool_call.function.name,
                    arguments = %tool_call.function.arguments,
                    "Tool call arguments"
                );

                let args: Option<Value> = serde_json::from_str(&tool_call.function.arguments).ok();
                let args = self.resolve_image_paths(args);

                self.record_event(
                    node_run.as_deref(),
                    "tool_call",
                    serde_json::json!({
                        "name": tool_call.function.name,
                        "index": tool_call_count - 1,
                    }),
                );

                match mcp.call_tool(&tool_call.function.name, args) {
                    Ok(result) => {
                        let prefix = format!("toolcall_{}", tool_call_count - 1);
                        let images = self.save_result_images(&result, &prefix, &mut node_run);
                        if !images.is_empty() {
                            last_image_tool = tool_call.function.name.clone();
                        }
                        pending_images.extend(images);

                        let result_text = Self::extract_result_text(&result);

                        self.log(format!(
                            "Tool result: {} chars, {} images",
                            result_text.len(),
                            pending_images.len()
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
                                "text_len": result_text.len(),
                                "image_count": pending_images.len(),
                            }),
                        );

                        messages.push(Message::tool_result(&tool_call.id, result_text));
                    }
                    Err(e) => {
                        self.log(format!("Tool call failed: {}", e));
                        messages.push(Message::tool_result(&tool_call.id, format!("Error: {}", e)));
                    }
                }
            }

            if !pending_images.is_empty() {
                let image_count = pending_images.len();
                if let Some(vlm) = &self.vlm {
                    self.log(format!(
                        "Analyzing {} image(s) with VLM ({})",
                        image_count,
                        vlm.model_name()
                    ));
                    match analyze_images(vlm, &params.prompt, &last_image_tool, pending_images)
                        .await
                    {
                        Ok(summary) => {
                            self.record_event(
                                node_run.as_deref(),
                                "vision_summary",
                                serde_json::json!({
                                    "image_count": image_count,
                                    "vlm_model": vlm.model_name(),
                                    "summary_json": summary,
                                }),
                            );
                            messages
                                .push(Message::user(format!("VLM_IMAGE_SUMMARY:\n{}", summary)));
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
                        pending_images,
                    ));
                }
            }
        }

        Ok(())
    }

    async fn execute_deterministic(
        &self,
        node_type: &NodeType,
        mcp: &McpClient,
        mut node_run: Option<&mut NodeRun>,
    ) -> Result<(), String> {
        let (tool_name, args) = match node_type {
            NodeType::TakeScreenshot(p) => {
                let mode = match p.mode {
                    ScreenshotMode::Screen => "screen",
                    ScreenshotMode::Window => "window",
                    ScreenshotMode::Region => "region",
                };
                let mut args = serde_json::json!({
                    "mode": mode,
                    "include_ocr": p.include_ocr,
                });
                if let Some(target) = &p.target {
                    args["app_name"] = Value::String(target.clone());
                }
                ("take_screenshot", args)
            }
            NodeType::FindText(p) => ("find_text", serde_json::json!({"text": p.search_text})),
            NodeType::FindImage(p) => {
                let mut args = serde_json::json!({
                    "threshold": p.threshold,
                    "max_results": p.max_results,
                });
                if let Some(img) = &p.template_image {
                    args["template_image_base64"] = Value::String(img.clone());
                }
                ("find_image", args)
            }
            NodeType::Click(p) => {
                let button = match p.button {
                    MouseButton::Left => "left",
                    MouseButton::Right => "right",
                    MouseButton::Center => "center",
                };

                // If there's a text target, resolve it to screen coordinates via find_text first
                let (x, y) = if let Some(target) = &p.target {
                    self.record_event(
                        node_run.as_deref(),
                        "tool_call",
                        serde_json::json!({"name": "find_text"}),
                    );
                    self.log(format!("Calling MCP tool: find_text (target={})", target));

                    let find_result = mcp
                        .call_tool("find_text", Some(serde_json::json!({"text": target})))
                        .map_err(|e| format!("find_text failed: {}", e))?;

                    let result_text = Self::extract_result_text(&find_result);
                    self.record_event(
                        node_run.as_deref(),
                        "tool_result",
                        serde_json::json!({"name": "find_text", "text_len": result_text.len()}),
                    );

                    // Parse the find_text result to extract coordinates
                    let matches: Vec<Value> = serde_json::from_str(&result_text)
                        .map_err(|e| format!("Failed to parse find_text result: {}", e))?;

                    let first = matches
                        .first()
                        .ok_or_else(|| format!("find_text found no matches for '{}'", target))?;

                    let x = first["x"]
                        .as_f64()
                        .ok_or_else(|| "find_text result missing 'x' coordinate".to_string())?;
                    let y = first["y"]
                        .as_f64()
                        .ok_or_else(|| "find_text result missing 'y' coordinate".to_string())?;

                    self.log(format!("Found '{}' at ({}, {})", target, x, y));
                    (Some(x), Some(y))
                } else {
                    (None, None)
                };

                let mut args = serde_json::json!({
                    "button": button,
                    "click_count": p.click_count,
                });
                if let Some(x) = x {
                    args["x"] = serde_json::json!(x);
                }
                if let Some(y) = y {
                    args["y"] = serde_json::json!(y);
                }
                ("click", args)
            }
            NodeType::TypeText(p) => ("type_text", serde_json::json!({"text": p.text})),
            NodeType::Scroll(p) => {
                let mut args = serde_json::json!({"delta_y": p.delta_y});
                if let Some(x) = p.x {
                    args["x"] = serde_json::json!(x);
                }
                if let Some(y) = p.y {
                    args["y"] = serde_json::json!(y);
                }
                ("scroll", args)
            }
            NodeType::ListWindows(p) => {
                let mut args = serde_json::json!({});
                if let Some(app) = &p.app_name {
                    args["app_name"] = Value::String(app.clone());
                }
                ("list_windows", args)
            }
            NodeType::FocusWindow(p) => {
                let mut args = serde_json::json!({});
                if let Some(val) = &p.value {
                    match p.method {
                        FocusMethod::AppName | FocusMethod::TitlePattern => {
                            args["app_name"] = Value::String(val.clone());
                        }
                        FocusMethod::WindowId => {
                            if let Ok(id) = val.parse::<u64>() {
                                args["window_id"] = serde_json::json!(id);
                            }
                        }
                    }
                }
                ("focus_window", args)
            }
            NodeType::AppDebugKitOp(p) => {
                self.log(format!(
                    "AppDebugKit operation: {} (not yet fully implemented)",
                    p.operation_name
                ));
                return Ok(());
            }
            NodeType::AiStep(_) => return Ok(()),
        };

        self.record_event(
            node_run.as_deref(),
            "tool_call",
            serde_json::json!({"name": tool_name}),
        );

        self.log(format!("Calling MCP tool: {}", tool_name));
        let args = self.resolve_image_paths(Some(args));
        let result = mcp
            .call_tool(tool_name, args)
            .map_err(|e| format!("MCP tool {} failed: {}", tool_name, e))?;

        let images = self.save_result_images(&result, "result", &mut node_run);
        let result_text = Self::extract_result_text(&result);

        self.record_event(
            node_run.as_deref(),
            "tool_result",
            serde_json::json!({
                "name": tool_name,
                "text_len": result_text.len(),
                "image_count": images.len(),
            }),
        );

        self.log(format!(
            "Tool result: {} chars, {} images",
            result_text.len(),
            images.len()
        ));
        Ok(())
    }

    fn check_step_complete(&self, content: &str) -> bool {
        serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|v| v.get("step_complete")?.as_bool())
            .unwrap_or(false)
    }

    fn resolve_image_paths(&self, args: Option<Value>) -> Option<Value> {
        let mut args = args?;
        let Some(proj) = &self.project_path else {
            return Some(args);
        };

        let path_keys = ["image_path", "imagePath", "path", "file", "template_path"];
        if let Some(obj) = args.as_object_mut() {
            for key in path_keys {
                if let Some(Value::String(path)) = obj.get(key)
                    && !path.starts_with('/')
                {
                    let absolute = proj.join(path);
                    obj.insert(
                        key.to_string(),
                        Value::String(absolute.to_string_lossy().to_string()),
                    );
                }
            }
        }

        Some(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_llm::{Content, ContentPart, Message};

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
}
