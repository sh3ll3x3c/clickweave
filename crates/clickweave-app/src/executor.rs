use clickweave_core::{NodeKind, Workflow};
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

            // Skip Start and End nodes
            if node.kind != NodeKind::Step {
                continue;
            }

            self.emit(ExecutorEvent::NodeStarted(node_id));
            self.log(format!("Executing step: {}", node.name));

            // Build messages
            let mut messages = vec![
                Message::system(workflow_system_prompt()),
                Message::user(build_step_prompt(
                    &node.params.prompt,
                    node.params.button_text.as_deref(),
                    node.params.image_path.as_deref(),
                )),
            ];

            let max_tool_calls = node.params.max_tool_calls.unwrap_or(10) as usize;
            let timeout_ms = node.params.timeout_ms;
            let step_start = Instant::now();
            let mut tool_call_count = 0;
            let mut step_error = false;

            // Tool loop
            loop {
                if tool_call_count >= max_tool_calls {
                    self.log(format!("Max tool calls reached for step: {}", node.name));
                    break;
                }

                if let Some(timeout) = timeout_ms
                    && step_start.elapsed().as_millis() as u64 > timeout
                {
                    self.log(format!("Timeout reached for step: {}", node.name));
                    break;
                }

                if self.stop_requested(&command_rx) {
                    self.log("Workflow stopped by user");
                    self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                    return;
                }

                // Call LLM
                let response = match self.llm.chat(messages.clone(), Some(tools.clone())).await {
                    Ok(r) => r,
                    Err(e) => {
                        self.emit_error(format!("LLM error: {}", e));
                        step_error = true;
                        break;
                    }
                };

                let Some(choice) = response.choices.first() else {
                    self.emit_error("No response from LLM");
                    step_error = true;
                    break;
                };

                let msg = &choice.message;

                // Check if there are tool calls
                if let Some(tool_calls) = &msg.tool_calls {
                    if tool_calls.is_empty() {
                        if let Some(content) = msg.content_text()
                            && self.check_step_complete(content)
                        {
                            self.log(format!("Step completed: {}", node.name));
                        }
                        break;
                    }

                    // Add assistant message with tool calls
                    messages.push(Message::assistant_tool_calls(tool_calls.clone()));

                    // Execute each tool call, collecting any images
                    let mut pending_images: Vec<(String, String)> = Vec::new();

                    for tool_call in tool_calls {
                        tool_call_count += 1;
                        self.log(format!("Tool call: {}", tool_call.function.name));

                        let args: Option<Value> =
                            serde_json::from_str(&tool_call.function.arguments).ok();

                        // Handle image_path - make it absolute if relative
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
                                messages.push(Message::tool_result(
                                    &tool_call.id,
                                    format!("Error: {}", e),
                                ));
                            }
                        }
                    }

                    // If images were returned, add them as a user message for vision
                    if !pending_images.is_empty() {
                        messages.push(Message::user_with_images(
                            "Here are the images from the tool results above.",
                            pending_images,
                        ));
                    }
                } else {
                    // No tool calls - check content for completion
                    if let Some(content) = msg.content_text() {
                        if self.check_step_complete(content) {
                            self.log(format!("Step completed: {}", node.name));
                        } else {
                            self.log(format!("Step finished (no tool calls): {}", node.name));
                        }
                    }
                    break;
                }
            }

            if step_error {
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }

            self.emit(ExecutorEvent::NodeCompleted(node_id));
        }

        self.log("Workflow execution completed");
        self.emit(ExecutorEvent::WorkflowCompleted);
        self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
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
