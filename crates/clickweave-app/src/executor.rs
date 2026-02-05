use clickweave_core::{AiStepParams, NodeType, Workflow};
use clickweave_llm::{LlmClient, LlmConfig, Message, build_step_prompt, workflow_system_prompt};
use clickweave_mcp::{McpClient, ToolContent};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;
use tracing::{error, info};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutorState {
    Idle,
    Running,
}

pub enum ExecutorCommand {
    Stop,
}

/// Events sent from the executor back to the UI
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ExecutorEvent {
    Log(String),
    StateChanged(ExecutorState),
    NodeStarted(Uuid),
    NodeCompleted(Uuid),
    NodeFailed(Uuid, String),
    WorkflowCompleted,
    Error(String),
}

pub struct WorkflowExecutor {
    workflow: Workflow,
    llm: LlmClient,
    mcp_command: String,
    project_path: Option<PathBuf>,
    event_tx: Sender<ExecutorEvent>,
}

impl WorkflowExecutor {
    pub fn new(
        workflow: Workflow,
        llm_config: LlmConfig,
        mcp_command: String,
        project_path: Option<PathBuf>,
        event_tx: Sender<ExecutorEvent>,
    ) -> Self {
        Self {
            workflow,
            llm: LlmClient::new(llm_config),
            mcp_command,
            project_path,
            event_tx,
        }
    }

    fn emit(&self, event: ExecutorEvent) {
        let _ = self.event_tx.send(event);
    }

    fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        info!("{}", msg);
        self.emit(ExecutorEvent::Log(msg));
    }

    fn emit_error(&self, msg: impl Into<String>) {
        let msg = msg.into();
        error!("{}", msg);
        self.emit(ExecutorEvent::Error(msg));
    }

    /// Returns true if a stop command was received.
    fn stop_requested(&self, command_rx: &Receiver<ExecutorCommand>) -> bool {
        matches!(command_rx.try_recv(), Ok(ExecutorCommand::Stop))
    }

    pub async fn run(&mut self, command_rx: Receiver<ExecutorCommand>) {
        self.emit(ExecutorEvent::StateChanged(ExecutorState::Running));
        self.log("Starting workflow execution");

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
            if self.stop_requested(&command_rx) {
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
            let node_name = node.name.clone();
            let node_type = node.node_type.clone();

            let mut attempt = 0;

            let succeeded = loop {
                let result = match &node_type {
                    NodeType::AiStep(params) => {
                        self.execute_ai_step(params, &tools, &mcp, timeout_ms, &command_rx)
                            .await
                    }
                    other => self.execute_deterministic(other, &mcp).await,
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
                        } else {
                            self.emit_error(format!("Node {} failed: {}", node_name, e));
                            self.emit(ExecutorEvent::NodeFailed(node_id, e));
                            self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                            return;
                        }
                    }
                }
            };

            if succeeded {
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
        command_rx: &Receiver<ExecutorCommand>,
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
                .llm
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

                for tool_call in tool_calls {
                    tool_call_count += 1;
                    self.log(format!("Tool call: {}", tool_call.function.name));

                    let args: Option<Value> =
                        serde_json::from_str(&tool_call.function.arguments).ok();
                    let args = self.resolve_image_paths(args);

                    match mcp.call_tool(&tool_call.function.name, args) {
                        Ok(result) => {
                            for content in &result.content {
                                if let ToolContent::Image { data, mime_type } = content {
                                    pending_images.push((data.clone(), mime_type.clone()));
                                }
                            }

                            let result_text: String = result
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    ToolContent::Text { text } => Some(text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            self.log(format!(
                                "Tool result: {} chars, {} images",
                                result_text.len(),
                                pending_images.len()
                            ));

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
                    messages.push(Message::user_with_images(
                        "Here are the images from the tool results above.",
                        pending_images,
                    ));
                }
            } else {
                if let Some(content) = msg.content_text() {
                    if self.check_step_complete(content) {
                        self.log("Step completed");
                    } else {
                        self.log("Step finished (no tool calls)");
                    }
                }
                break;
            }
        }

        Ok(())
    }

    async fn execute_deterministic(
        &self,
        node_type: &NodeType,
        mcp: &McpClient,
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
                    // target is treated as coordinates or element reference
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

        self.log(format!("Calling MCP tool: {}", tool_name));
        let args = self.resolve_image_paths(Some(args));
        match mcp.call_tool(tool_name, args) {
            Ok(result) => {
                let result_text: String = result
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ToolContent::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let image_count = result
                    .content
                    .iter()
                    .filter(|c| matches!(c, ToolContent::Image { .. }))
                    .count();

                self.log(format!(
                    "Tool result: {} chars, {} images",
                    result_text.len(),
                    image_count
                ));
                Ok(())
            }
            Err(e) => Err(format!("MCP tool {} failed: {}", tool_name, e)),
        }
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
