use clickweave_core::{NodeKind, Workflow};
use clickweave_llm::{LlmClient, LlmConfig, Message, build_step_prompt, workflow_system_prompt};
use clickweave_mcp::McpClient;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorState {
    Idle,
    Running,
    Paused,
    Error,
}

pub enum ExecutorCommand {
    Stop,
    Pause,
    Resume,
    Step,
}

pub struct WorkflowExecutor {
    workflow: Workflow,
    llm: LlmClient,
    mcp_command: String,
    project_path: Option<PathBuf>,
    current_node: Option<Uuid>,
}

impl WorkflowExecutor {
    pub fn new(
        workflow: Workflow,
        llm_config: LlmConfig,
        mcp_command: String,
        project_path: Option<PathBuf>,
    ) -> Self {
        Self {
            workflow,
            llm: LlmClient::new(llm_config),
            mcp_command,
            project_path,
            current_node: None,
        }
    }

    pub async fn run(&mut self, command_rx: Receiver<ExecutorCommand>) {
        info!("Starting workflow execution");

        // Spawn MCP server
        let mcp = if self.mcp_command == "npx" {
            McpClient::spawn_npx()
        } else {
            McpClient::spawn(&self.mcp_command, &[])
        };

        let mut mcp = match mcp {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to spawn MCP server: {}", e);
                return;
            }
        };

        info!("MCP server ready with {} tools", mcp.tools().len());

        // Get execution order
        let execution_order = self.workflow.execution_order();
        info!("Execution order: {} nodes", execution_order.len());

        // Convert MCP tools to OpenAI format
        let tools: Vec<Value> = mcp
            .tools_as_openai()
            .into_iter()
            .map(|t| serde_json::to_value(t).unwrap())
            .collect();

        // Execute each node
        for node_id in execution_order {
            // Check for stop command
            if let Ok(cmd) = command_rx.try_recv() {
                match cmd {
                    ExecutorCommand::Stop => {
                        info!("Workflow stopped by user");
                        return;
                    }
                    _ => {}
                }
            }

            let Some(node) = self.workflow.find_node(node_id) else {
                continue;
            };

            self.current_node = Some(node_id);

            // Skip Start and End nodes
            if node.kind != NodeKind::Step {
                info!("Skipping {} node: {}", node.kind.display_name(), node.name);
                continue;
            }

            info!("Executing step: {}", node.name);

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
            let mut tool_call_count = 0;

            // Tool loop
            loop {
                if tool_call_count >= max_tool_calls {
                    warn!("Max tool calls reached for step: {}", node.name);
                    break;
                }

                // Check for stop
                if let Ok(ExecutorCommand::Stop) = command_rx.try_recv() {
                    info!("Workflow stopped by user");
                    return;
                }

                // Call LLM
                let response = match self.llm.chat(messages.clone(), Some(tools.clone())).await {
                    Ok(r) => r,
                    Err(e) => {
                        error!("LLM error: {}", e);
                        return;
                    }
                };

                let Some(choice) = response.choices.first() else {
                    error!("No response from LLM");
                    return;
                };

                let msg = &choice.message;

                // Check if there are tool calls
                if let Some(tool_calls) = &msg.tool_calls {
                    if tool_calls.is_empty() {
                        // No tool calls, check for completion
                        if let Some(content) = &msg.content {
                            if self.check_step_complete(content) {
                                info!("Step completed: {}", node.name);
                                break;
                            }
                        }
                        break;
                    }

                    // Add assistant message with tool calls
                    messages.push(Message::assistant_tool_calls(tool_calls.clone()));

                    // Execute each tool call
                    for tool_call in tool_calls {
                        tool_call_count += 1;
                        info!("Tool call: {} ({})", tool_call.function.name, tool_call.id);

                        let args: Option<Value> =
                            serde_json::from_str(&tool_call.function.arguments).ok();

                        // Handle image_path - make it absolute if relative
                        let args = self.resolve_image_paths(args);

                        match mcp.call_tool(&tool_call.function.name, args) {
                            Ok(result) => {
                                let result_text: String = result
                                    .content
                                    .iter()
                                    .filter_map(|c| c.as_text())
                                    .collect::<Vec<_>>()
                                    .join("\n");

                                info!("Tool result: {} chars", result_text.len());

                                messages.push(Message::tool_result(&tool_call.id, result_text));
                            }
                            Err(e) => {
                                error!("Tool call failed: {}", e);
                                messages.push(Message::tool_result(
                                    &tool_call.id,
                                    format!("Error: {}", e),
                                ));
                            }
                        }
                    }
                } else {
                    // No tool calls - check content for completion
                    if let Some(content) = &msg.content {
                        if self.check_step_complete(content) {
                            info!("Step completed: {}", node.name);
                        } else {
                            info!("Step finished (no tool calls): {}", node.name);
                        }
                    }
                    break;
                }
            }
        }

        info!("Workflow execution completed");
    }

    fn check_step_complete(&self, content: &str) -> bool {
        // Try to parse as JSON and check for step_complete
        if let Ok(v) = serde_json::from_str::<Value>(content) {
            if let Some(complete) = v.get("step_complete").and_then(|v| v.as_bool()) {
                return complete;
            }
        }
        false
    }

    fn resolve_image_paths(&self, args: Option<Value>) -> Option<Value> {
        let mut args = args?;

        if let Some(obj) = args.as_object_mut() {
            // Check common image path field names
            for key in ["image_path", "imagePath", "path", "file", "template_path"] {
                if let Some(Value::String(path)) = obj.get(key) {
                    if !path.starts_with('/') {
                        if let Some(proj) = &self.project_path {
                            let absolute = proj.join(path);
                            obj.insert(
                                key.to_string(),
                                Value::String(absolute.to_string_lossy().to_string()),
                            );
                        }
                    }
                }
            }
        }

        Some(args)
    }
}
