use super::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use clickweave_core::{NodeType, RunStatus};
use clickweave_llm::ChatBackend;
use clickweave_mcp::McpClient;
use tokio::sync::mpsc::Receiver;

impl<C: ChatBackend> WorkflowExecutor<C> {
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

        match self.storage.begin_execution() {
            Ok(exec_dir) => self.log(format!("Execution dir: {}", exec_dir)),
            Err(e) => {
                self.emit_error(format!("Failed to create execution directory: {}", e));
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }
        }

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
                .create_run(node_id, &node_name, trace_level)
                .ok();

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
                        self.evict_app_cache_for_node(&node_type);
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
}
