use super::{ExecutorCommand, ExecutorEvent, ExecutorState, WorkflowExecutor};
use clickweave_core::{EdgeOutput, NodeType, RunStatus};
use clickweave_llm::ChatBackend;
use clickweave_mcp::McpClient;
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use uuid::Uuid;

impl<C: ChatBackend> WorkflowExecutor<C> {
    /// Find entry points: nodes with no incoming edges.
    /// EndLoop back-edges (edges where the source is an EndLoop node)
    /// are NOT counted as incoming edges — this prevents loops from breaking
    /// entry point detection.
    pub(crate) fn entry_points(&self) -> Vec<Uuid> {
        let endloop_nodes: HashSet<Uuid> = self
            .workflow
            .nodes
            .iter()
            .filter(|n| matches!(n.node_type, NodeType::EndLoop(_)))
            .map(|n| n.id)
            .collect();

        let targets: HashSet<Uuid> = self
            .workflow
            .edges
            .iter()
            .filter(|e| !endloop_nodes.contains(&e.from))
            .map(|e| e.to)
            .collect();

        self.workflow
            .nodes
            .iter()
            .filter(|n| !targets.contains(&n.id))
            .map(|n| n.id)
            .collect()
    }

    /// Follow the single outgoing edge from a regular node (output is None).
    pub(crate) fn follow_single_edge(&self, from: Uuid) -> Option<Uuid> {
        self.workflow
            .edges
            .iter()
            .find(|e| e.from == from && e.output.is_none())
            .map(|e| e.to)
    }

    /// Follow a specific labeled edge from a control flow node.
    pub(crate) fn follow_edge(&self, from: Uuid, output: &EdgeOutput) -> Option<Uuid> {
        self.workflow
            .edges
            .iter()
            .find(|e| e.from == from && e.output.as_ref() == Some(output))
            .map(|e| e.to)
    }

    /// Follow the "default" edge when a control flow node is disabled.
    /// Falls through to the non-executing branch: IfFalse, LoopDone, or
    /// the first available outgoing edge for Switch.
    fn follow_disabled_edge(&self, node_id: Uuid, node_type: &NodeType) -> Option<Uuid> {
        match node_type {
            NodeType::If(_) => self.follow_edge(node_id, &EdgeOutput::IfFalse),
            NodeType::Loop(_) => self.follow_edge(node_id, &EdgeOutput::LoopDone),
            NodeType::Switch(_) => self
                .follow_edge(node_id, &EdgeOutput::SwitchDefault)
                .or_else(|| {
                    // No default edge — pick the first case edge as fallthrough
                    self.workflow
                        .edges
                        .iter()
                        .find(|e| e.from == node_id && e.output.is_some())
                        .map(|e| e.to)
                }),
            // EndLoop and regular nodes: follow_single_edge is fine
            _ => self.follow_single_edge(node_id),
        }
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

        match self.storage.begin_execution() {
            Ok(exec_dir) => self.log(format!("Execution dir: {}", exec_dir)),
            Err(e) => {
                self.emit_error(format!("Failed to create execution directory: {}", e));
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }
        }

        // Find the first entry point to start walking from
        let entries = self.entry_points();
        if entries.is_empty() {
            self.emit_error("No entry point found in workflow".to_string());
            self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
            return;
        }
        let mut current: Option<Uuid> = Some(entries[0]);

        self.log("Starting graph walk from entry point".to_string());

        let tools = mcp.tools_as_openai();

        while let Some(node_id) = current {
            if self.stop_requested(&mut command_rx) {
                self.log("Workflow stopped by user");
                self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
                return;
            }

            let Some(node) = self.workflow.find_node(node_id) else {
                self.log(format!("Node {} not found, stopping", node_id));
                break;
            };

            if !node.enabled {
                self.log(format!("Skipping disabled node: {}", node.name));
                current = self.follow_disabled_edge(node_id, &node.node_type);
                continue;
            }

            let node_name = node.name.clone();
            let node_type = node.node_type.clone();

            match &node_type {
                // --- Control flow: If ---
                NodeType::If(params) => {
                    self.log(format!("Evaluating If: {}", node_name));
                    let result = self.context.evaluate_condition(&params.condition);
                    let resolved_left = self.context.resolve_value_ref(&params.condition.left);
                    let resolved_right = self.context.resolve_value_ref(&params.condition.right);
                    let output_taken = if result { "IfTrue" } else { "IfFalse" };

                    self.record_event(
                        None,
                        "branch_evaluated",
                        serde_json::json!({
                            "node_id": node_id.to_string(),
                            "node_name": node_name,
                            "condition": format!("{:?} {:?} {:?}",
                                params.condition.left,
                                params.condition.operator,
                                params.condition.right),
                            "resolved_left": resolved_left,
                            "resolved_right": resolved_right,
                            "result": result,
                            "output_taken": output_taken,
                        }),
                    );

                    current = if result {
                        self.follow_edge(node_id, &EdgeOutput::IfTrue)
                    } else {
                        self.follow_edge(node_id, &EdgeOutput::IfFalse)
                    };
                }

                // --- Control flow: Switch ---
                NodeType::Switch(params) => {
                    self.log(format!("Evaluating Switch: {}", node_name));
                    let matched = params
                        .cases
                        .iter()
                        .find(|c| self.context.evaluate_condition(&c.condition));

                    let (output_taken, next) = match matched {
                        Some(case) => {
                            let name = case.name.clone();
                            (
                                format!("SwitchCase({})", name),
                                self.follow_edge(node_id, &EdgeOutput::SwitchCase { name }),
                            )
                        }
                        None => (
                            "SwitchDefault".to_string(),
                            self.follow_edge(node_id, &EdgeOutput::SwitchDefault),
                        ),
                    };

                    self.record_event(
                        None,
                        "branch_evaluated",
                        serde_json::json!({
                            "node_id": node_id.to_string(),
                            "node_name": node_name,
                            "output_taken": output_taken,
                        }),
                    );

                    if next.is_none() {
                        self.log(format!(
                            "Warning: Switch '{}' had no matching case and no default edge — workflow path ends here",
                            node_name
                        ));
                    }

                    current = next;
                }

                // --- Control flow: Loop ---
                // Do-while semantics: exit condition is NOT checked on iteration 0.
                // The loop body always runs at least once. This is intentional for UI
                // automation where the common pattern is "try action, check result,
                // retry if needed."
                NodeType::Loop(params) => {
                    let iteration = *self.context.loop_counters.get(&node_id).unwrap_or(&0);

                    let should_exit = if iteration >= params.max_iterations {
                        // Safety cap hit — this is a warning, likely something unexpected
                        self.log(format!(
                            "Loop '{}' hit max iterations ({}), exiting",
                            node_name, params.max_iterations
                        ));
                        self.record_event(
                            None,
                            "loop_exited",
                            serde_json::json!({
                                "node_id": node_id.to_string(),
                                "node_name": node_name,
                                "reason": "max_iterations",
                                "iterations_completed": iteration,
                            }),
                        );
                        true
                    } else if iteration > 0
                        && self.context.evaluate_condition(&params.exit_condition)
                    {
                        // Exit condition met (checked from iteration 1 onward)
                        self.log(format!(
                            "Loop '{}' exit condition met after {} iterations",
                            node_name, iteration
                        ));
                        self.record_event(
                            None,
                            "loop_exited",
                            serde_json::json!({
                                "node_id": node_id.to_string(),
                                "node_name": node_name,
                                "reason": "condition_met",
                                "iterations_completed": iteration,
                            }),
                        );
                        true
                    } else {
                        false
                    };

                    if should_exit {
                        self.context.loop_counters.remove(&node_id);
                        current = self.follow_edge(node_id, &EdgeOutput::LoopDone);
                    } else {
                        self.log(format!("Loop '{}' iteration {}", node_name, iteration));
                        self.record_event(
                            None,
                            "loop_iteration",
                            serde_json::json!({
                                "node_id": node_id.to_string(),
                                "node_name": node_name,
                                "iteration": iteration,
                            }),
                        );
                        *self.context.loop_counters.entry(node_id).or_insert(0) += 1;
                        current = self.follow_edge(node_id, &EdgeOutput::LoopBody);
                    }
                }

                // --- Control flow: EndLoop ---
                // Jump back to the paired Loop node. The Loop will re-evaluate
                // its exit condition on the next pass.
                NodeType::EndLoop(params) => {
                    self.log(format!("EndLoop: jumping back to Loop {}", params.loop_id));
                    current = Some(params.loop_id);
                }

                // --- Regular execution nodes ---
                _ => {
                    self.emit(ExecutorEvent::NodeStarted(node_id));
                    self.log(format!(
                        "Executing node: {} ({})",
                        node_name,
                        node_type.display_name()
                    ));

                    let timeout_ms = node.timeout_ms;
                    let settle_ms = node.settle_ms;
                    let retries = node.retries;
                    let trace_level = node.trace_level;

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
                        serde_json::json!({
                            "name": node_name,
                            "type": node_type.display_name(),
                        }),
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
                                    serde_json::json!({
                                        "attempt": attempt,
                                        "error": e,
                                    }),
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

                    // Store node outputs in RuntimeContext for condition evaluation.
                    let sanitized = sanitize_node_name(&node_name);
                    self.context.set_variable(
                        format!("{}.success", sanitized),
                        serde_json::Value::Bool(true),
                    );
                    self.record_event(
                        node_run.as_ref(),
                        "variable_set",
                        serde_json::json!({
                            "node_name": node_name,
                            "variable": format!("{}.success", sanitized),
                            "value": true,
                        }),
                    );

                    if let Some(ms) = settle_ms.filter(|&ms| ms > 0) {
                        self.log(format!("Settling for {}ms", ms));
                        tokio::time::sleep(Duration::from_millis(ms)).await;
                    }

                    if let Some(ref mut run) = node_run {
                        self.finalize_run(run, RunStatus::Ok);
                    }
                    self.emit(ExecutorEvent::NodeCompleted(node_id));

                    current = self.follow_single_edge(node_id);
                }
            }
        }

        self.log("Workflow execution completed");
        self.emit(ExecutorEvent::WorkflowCompleted);
        self.emit(ExecutorEvent::StateChanged(ExecutorState::Idle));
    }
}

/// Sanitize a node name for use as a variable prefix.
/// Converts to lowercase, replaces spaces and non-alphanumeric chars with underscores.
fn sanitize_node_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_simple_name() {
        assert_eq!(sanitize_node_name("Find Text"), "find_text");
    }

    #[test]
    fn sanitize_name_with_special_chars() {
        assert_eq!(
            sanitize_node_name("Click (Login Button)"),
            "click__login_button_"
        );
    }

    #[test]
    fn sanitize_name_preserves_underscores() {
        assert_eq!(sanitize_node_name("my_node_1"), "my_node_1");
    }

    #[test]
    fn sanitize_empty_name() {
        assert_eq!(sanitize_node_name(""), "");
    }
}
