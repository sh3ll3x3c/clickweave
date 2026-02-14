use super::planner::fetch_mcp_tool_schemas;
use super::types::*;
use clickweave_llm::planner::conversation::{
    ChatEntry, ChatRole, ConversationSession, NodeResult, PatchSummary, RunContext,
};

fn dto_to_session(
    history: &[ChatEntryDto],
    summary: Option<String>,
    summary_cutoff: usize,
) -> ConversationSession {
    let messages = history
        .iter()
        .map(|e| ChatEntry {
            role: match e.role.as_str() {
                "user" => ChatRole::User,
                _ => ChatRole::Assistant,
            },
            content: e.content.clone(),
            timestamp: e.timestamp,
            patch_summary: e.patch_summary.as_ref().map(|ps| PatchSummary {
                added: ps.added as usize,
                removed: ps.removed as usize,
                updated: ps.updated as usize,
                description: ps.description.clone(),
            }),
            run_context: e.run_context.as_ref().map(|rc| RunContext {
                execution_dir: rc.execution_dir.clone(),
                node_results: rc
                    .node_results
                    .iter()
                    .map(|nr| NodeResult {
                        node_name: nr.node_name.clone(),
                        status: nr.status.clone(),
                        error: nr.error.clone(),
                    })
                    .collect(),
            }),
        })
        .collect();

    ConversationSession {
        messages,
        summary,
        summary_cutoff,
    }
}

fn format_run_context(ctx: &RunContextDto) -> String {
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
    let session = dto_to_session(&request.history, request.summary, request.summary_cutoff);
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
