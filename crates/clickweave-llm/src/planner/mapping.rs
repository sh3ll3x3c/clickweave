use super::PlanStep;
use anyhow::{Result, anyhow};
use clickweave_core::{AiStepParams, EndLoopParams, IfParams, LoopParams, NodeType, tool_mapping};
use serde_json::Value;

/// Map a PlanStep to a NodeType.
pub(crate) fn step_to_node_type(step: &PlanStep, tools: &[Value]) -> Result<(NodeType, String)> {
    match step {
        PlanStep::Tool {
            tool_name,
            arguments,
            name,
        } => {
            let display = name.clone().unwrap_or_else(|| tool_name.replace('_', " "));
            let node_type = tool_mapping::tool_invocation_to_node_type(tool_name, arguments, tools)
                .map_err(|e| anyhow!("{}", e))?;
            Ok((node_type, display))
        }
        PlanStep::AiTransform { name, kind, .. } => {
            let display = name
                .clone()
                .unwrap_or_else(|| format!("AI Transform ({})", kind));
            Ok((
                NodeType::AiStep(AiStepParams {
                    prompt: format!("Perform a '{}' transform on the input.", kind),
                    allowed_tools: Some(vec![]),
                    max_tool_calls: Some(0),
                    ..Default::default()
                }),
                display,
            ))
        }
        PlanStep::AiStep {
            prompt,
            allowed_tools,
            max_tool_calls,
            timeout_ms,
            name,
        } => {
            let display = name.clone().unwrap_or_else(|| "AI Step".to_string());
            Ok((
                NodeType::AiStep(AiStepParams {
                    prompt: prompt.clone(),
                    allowed_tools: allowed_tools.clone(),
                    max_tool_calls: *max_tool_calls,
                    timeout_ms: *timeout_ms,
                    ..Default::default()
                }),
                display,
            ))
        }
        PlanStep::If { name, condition } => {
            let display = name.clone().unwrap_or_else(|| "If".to_string());
            Ok((
                NodeType::If(IfParams {
                    condition: condition.clone(),
                }),
                display,
            ))
        }
        PlanStep::Loop {
            name,
            exit_condition,
            max_iterations,
        } => {
            let display = name.clone().unwrap_or_else(|| "Loop".to_string());
            Ok((
                NodeType::Loop(LoopParams {
                    exit_condition: exit_condition.clone(),
                    max_iterations: max_iterations.unwrap_or(100),
                }),
                display,
            ))
        }
        PlanStep::EndLoop { name, .. } => {
            let display = name.clone().unwrap_or_else(|| "End Loop".to_string());
            // Placeholder UUID — will be remapped by build_workflow_from_graph().
            Ok((
                NodeType::EndLoop(EndLoopParams {
                    loop_id: uuid::Uuid::nil(),
                }),
                display,
            ))
        }
    }
}
