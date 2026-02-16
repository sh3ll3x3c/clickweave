use super::planner::fetch_mcp_tool_schemas;
use super::types::*;
use clickweave_llm::planner::conversation::{ConversationSession, RunContext};

fn format_run_context(ctx: &RunContext) -> String {
    let mut lines = vec![format!("Execution: {}", ctx.execution_dir)];
    for nr in &ctx.node_results {
        let mut line = format!("  - {} [{}]", nr.node_name, nr.status);
        if let Some(err) = &nr.error {
            line.push_str(&format!(": {}", err));
        }
        lines.push(line);
    }
    lines.join("\n")
}

#[tauri::command]
#[specta::specta]
pub async fn assistant_chat(
    request: AssistantChatRequest,
) -> Result<AssistantChatResponse, String> {
    let tools = fetch_mcp_tool_schemas(&request.mcp_command).await?;
    let config = request.planner.into_llm_config(None);
    let session = ConversationSession {
        messages: request.history,
        summary: request.summary,
        summary_cutoff: request.summary_cutoff,
    };
    let run_context_text = request.run_context.as_ref().map(format_run_context);

    let result = clickweave_llm::planner::assistant_chat(
        &request.workflow,
        &request.user_message,
        &session,
        run_context_text.as_deref(),
        config,
        &tools,
        request.allow_ai_transforms,
        request.allow_agent_steps,
        (request.max_repair_attempts as usize).min(10),
    )
    .await
    .map_err(|e| format!("Assistant chat failed: {}", e))?;

    let patch = result.patch.map(|p| WorkflowPatch {
        added_nodes: p.added_nodes,
        removed_node_ids: p.removed_node_ids.iter().map(|id| id.to_string()).collect(),
        updated_nodes: p.updated_nodes,
        added_edges: p.added_edges,
        removed_edges: p.removed_edges,
        warnings: p.warnings,
    });

    let new_cutoff = if result.new_summary.is_some() {
        session.current_cutoff(None)
    } else {
        request.summary_cutoff
    };

    Ok(AssistantChatResponse {
        assistant_message: result.message,
        patch,
        new_summary: result.new_summary,
        summary_cutoff: new_cutoff,
        warnings: result.warnings,
    })
}
