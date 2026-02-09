use base64::Engine;
use clickweave_core::storage::RunStorage;
use clickweave_core::{
    AiStepParams, ArtifactKind, NodeRun, NodeType, RunStatus, TraceEvent, TraceLevel, Workflow,
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
    orchestrator: C,
    vlm: Option<C>,
    mcp_command: String,
    project_path: Option<PathBuf>,
    event_tx: Sender<ExecutorEvent>,
    storage: Option<RunStorage>,
}

impl WorkflowExecutor {
    pub fn new(
        workflow: Workflow,
        orchestrator_config: LlmConfig,
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
            orchestrator: LlmClient::new(orchestrator_config),
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

    fn record_event(&self, run: &NodeRun, event_type: &str, payload: Value) {
        if let Some(storage) = &self.storage {
            let event = TraceEvent {
                timestamp: Self::now_millis(),
                event_type: event_type.to_string(),
                payload,
            };
            if let Err(e) = storage.append_event(run, &event) {
                tracing::warn!("Failed to append trace event: {}", e);
            }
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

        // Log model info from /v1/models
        self.log_model_info("Orchestrator", &self.orchestrator)
            .await;
        if let Some(vlm) = &self.vlm {
            self.log(format!("VLM enabled: {}", vlm.model_name()));
            self.log_model_info("VLM", vlm).await;
        } else {
            self.log("VLM not configured — images sent directly to orchestrator");
        }

        // Spawn MCP server
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

        // Get execution order
        let execution_order = self.workflow.execution_order();
        self.log(format!("Execution order: {} nodes", execution_order.len()));

        // Convert MCP tools to OpenAI format
        let tools: Vec<Value> = mcp
            .tools_as_openai()
            .into_iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();

        for node_id in execution_order {
            if self.stop_requested(&mut command_rx) {
                self.log("Workflow stopped by user");
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }

            let Some(node) = self.workflow.find_node(node_id) else {
                continue;
            };

            // Skip disabled nodes
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

            // Create a run record
            let mut node_run = self
                .storage
                .as_ref()
                .and_then(|s| s.create_run(node_id, trace_level).ok());

            if let Some(ref run) = node_run {
                self.emit(ExecutorEvent::RunCreated(node_id, run.clone()));
                self.record_event(
                    run,
                    "node_started",
                    serde_json::json!({"name": node_name, "type": node_type.display_name()}),
                );
            }

            let mut attempt = 0;

            let succeeded = loop {
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
                    Ok(()) => break true,
                    Err(e) => {
                        if attempt < retries {
                            attempt += 1;
                            self.log(format!(
                                "Node {} failed (attempt {}/{}): {}. Retrying...",
                                node_name,
                                attempt,
                                retries + 1,
                                e
                            ));
                            if let Some(ref run) = node_run {
                                self.record_event(
                                    run,
                                    "retry",
                                    serde_json::json!({"attempt": attempt, "error": e}),
                                );
                            }
                        } else {
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
            };

            if succeeded {
                if let Some(ref mut run) = node_run {
                    self.finalize_run(run, RunStatus::Ok);
                }
                self.emit(ExecutorEvent::NodeCompleted(node_id));
            }
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
                .orchestrator
                .chat(messages.clone(), Some(tools.to_vec()))
                .await
                .map_err(|e| format!("LLM error: {}", e))?;

            let choice = response
                .choices
                .first()
                .ok_or_else(|| "No response from LLM".to_string())?;

            let msg = &choice.message;

            if let Some(tool_calls) = &msg.tool_calls {
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

                    let args: Option<Value> =
                        serde_json::from_str(&tool_call.function.arguments).ok();
                    let args = self.resolve_image_paths(args);

                    // Record tool call event
                    if let Some(ref run) = node_run {
                        self.record_event(
                            run,
                            "tool_call",
                            serde_json::json!({
                                "name": tool_call.function.name,
                                "index": tool_call_count - 1,
                            }),
                        );
                    }

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

                            if let Some(ref run) = node_run {
                                self.record_event(
                                    run,
                                    "tool_result",
                                    serde_json::json!({
                                        "name": tool_call.function.name,
                                        "text_len": result_text.len(),
                                        "image_count": pending_images.len(),
                                    }),
                                );
                            }

                            messages.push(Message::tool_result(&tool_call.id, result_text));
                        }
                        Err(e) => {
                            self.log(format!("Tool call failed: {}", e));
                            messages
                                .push(Message::tool_result(&tool_call.id, format!("Error: {}", e)));
                        }
                    }
                }

                if !pending_images.is_empty() {
                    let image_count = pending_images.len();
                    if let Some(vlm) = &self.vlm {
                        // Route images through the VLM
                        self.log(format!(
                            "Analyzing {} image(s) with VLM ({})",
                            image_count,
                            vlm.model_name()
                        ));
                        match analyze_images(vlm, &params.prompt, &last_image_tool, pending_images)
                            .await
                        {
                            Ok(summary) => {
                                if let Some(ref run) = node_run {
                                    self.record_event(
                                        run,
                                        "vision_summary",
                                        serde_json::json!({
                                            "image_count": image_count,
                                            "vlm_model": vlm.model_name(),
                                            "summary_json": summary,
                                        }),
                                    );
                                }
                                messages.push(Message::user(format!(
                                    "VLM_IMAGE_SUMMARY:\n{}",
                                    summary
                                )));
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
                        // No VLM configured — send images directly to orchestrator
                        messages.push(Message::user_with_images(
                            "Here are the images from the tool results above.",
                            pending_images,
                        ));
                    }
                }
            } else {
                let completed = msg
                    .content_text()
                    .is_some_and(|c| self.check_step_complete(c));
                self.log(if completed {
                    "Step completed"
                } else {
                    "Step finished (no tool calls)"
                });
                break;
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
                let mut args = serde_json::Map::new();
                args.insert(
                    "mode".to_string(),
                    Value::String(
                        match p.mode {
                            clickweave_core::ScreenshotMode::Screen => "screen",
                            clickweave_core::ScreenshotMode::Window => "window",
                            clickweave_core::ScreenshotMode::Region => "region",
                        }
                        .to_string(),
                    ),
                );
                if let Some(target) = &p.target {
                    args.insert("app_name".to_string(), Value::String(target.clone()));
                }
                args.insert("include_ocr".to_string(), Value::Bool(p.include_ocr));
                ("take_screenshot", Value::Object(args))
            }
            NodeType::FindText(p) => {
                let mut args = serde_json::Map::new();
                args.insert("text".to_string(), Value::String(p.search_text.clone()));
                ("find_text", Value::Object(args))
            }
            NodeType::FindImage(p) => {
                let mut args = serde_json::Map::new();
                if let Some(img) = &p.template_image {
                    args.insert(
                        "template_image_base64".to_string(),
                        Value::String(img.clone()),
                    );
                }
                args.insert(
                    "threshold".to_string(),
                    Value::Number(serde_json::Number::from_f64(p.threshold).unwrap()),
                );
                args.insert(
                    "max_results".to_string(),
                    Value::Number(p.max_results.into()),
                );
                ("find_image", Value::Object(args))
            }
            NodeType::Click(p) => {
                let mut args = serde_json::Map::new();
                if let Some(target) = &p.target {
                    args.insert("target".to_string(), Value::String(target.clone()));
                }
                args.insert(
                    "button".to_string(),
                    Value::String(
                        match p.button {
                            clickweave_core::MouseButton::Left => "left",
                            clickweave_core::MouseButton::Right => "right",
                            clickweave_core::MouseButton::Center => "center",
                        }
                        .to_string(),
                    ),
                );
                args.insert(
                    "click_count".to_string(),
                    Value::Number(p.click_count.into()),
                );
                ("click", Value::Object(args))
            }
            NodeType::TypeText(p) => {
                let mut args = serde_json::Map::new();
                args.insert("text".to_string(), Value::String(p.text.clone()));
                ("type_text", Value::Object(args))
            }
            NodeType::Scroll(p) => {
                let mut args = serde_json::Map::new();
                args.insert("delta_y".to_string(), Value::Number(p.delta_y.into()));
                if let Some(x) = p.x {
                    args.insert(
                        "x".to_string(),
                        Value::Number(serde_json::Number::from_f64(x).unwrap()),
                    );
                }
                if let Some(y) = p.y {
                    args.insert(
                        "y".to_string(),
                        Value::Number(serde_json::Number::from_f64(y).unwrap()),
                    );
                }
                ("scroll", Value::Object(args))
            }
            NodeType::ListWindows(p) => {
                let mut args = serde_json::Map::new();
                if let Some(app) = &p.app_name {
                    args.insert("app_name".to_string(), Value::String(app.clone()));
                }
                ("list_windows", Value::Object(args))
            }
            NodeType::FocusWindow(p) => {
                let mut args = serde_json::Map::new();
                if let Some(val) = &p.value {
                    match p.method {
                        clickweave_core::FocusMethod::AppName => {
                            args.insert("app_name".to_string(), Value::String(val.clone()));
                        }
                        clickweave_core::FocusMethod::WindowId => {
                            if let Ok(id) = val.parse::<u64>() {
                                args.insert("window_id".to_string(), Value::Number(id.into()));
                            }
                        }
                        clickweave_core::FocusMethod::TitlePattern => {
                            args.insert("app_name".to_string(), Value::String(val.clone()));
                        }
                    }
                }
                ("focus_window", Value::Object(args))
            }
            NodeType::AppDebugKitOp(p) => {
                self.log(format!(
                    "AppDebugKit operation: {} (not yet fully implemented)",
                    p.operation_name
                ));
                return Ok(());
            }
            NodeType::AiStep(_) => {
                // Should not reach here - handled separately
                return Ok(());
            }
        };

        // Record tool call event
        if let Some(ref run) = node_run {
            self.record_event(run, "tool_call", serde_json::json!({"name": tool_name}));
        }

        self.log(format!("Calling MCP tool: {}", tool_name));
        let args = self.resolve_image_paths(Some(args));
        let result = mcp
            .call_tool(tool_name, args)
            .map_err(|e| format!("MCP tool {} failed: {}", tool_name, e))?;

        let images = self.save_result_images(&result, "result", &mut node_run);
        let result_text = Self::extract_result_text(&result);

        if let Some(ref run) = node_run {
            self.record_event(
                run,
                "tool_result",
                serde_json::json!({
                    "name": tool_name,
                    "text_len": result_text.len(),
                    "image_count": images.len(),
                }),
            );
        }

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
                            "Message[{}] (role={}) contains image content — orchestrator should never receive images when VLM is configured",
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
            orchestrator: C,
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
                orchestrator,
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

        // Simulate: orchestrator made a tool call, got a result with images
        messages.push(Message::tool_result("call_1", "screenshot taken"));

        // VLM analyzed the images and produced a summary
        let vlm_summary = r#"{"summary": "Login page with username/password fields"}"#;
        messages.push(Message::user(format!(
            "VLM_IMAGE_SUMMARY:\n{}",
            vlm_summary
        )));

        // Verify: no images in the orchestrator messages
        assert_no_images(&messages);

        // Verify: the VLM summary is present as plain text
        let last = messages.last().unwrap();
        assert!(matches!(&last.content, Some(Content::Text(t)) if t.contains("VLM_IMAGE_SUMMARY")));
    }
}
