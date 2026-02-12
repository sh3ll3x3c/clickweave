use super::WorkflowExecutor;
use clickweave_core::{NodeRun, NodeType, tool_mapping};
use clickweave_llm::ChatBackend;
use clickweave_mcp::McpClient;

impl<C: ChatBackend> WorkflowExecutor<C> {
    pub(crate) async fn execute_deterministic(
        &self,
        node_type: &NodeType,
        mcp: &McpClient,
        mut node_run: Option<&mut NodeRun>,
    ) -> Result<(), String> {
        // Handle non-tool node types
        if let NodeType::AppDebugKitOp(p) = node_type {
            self.log(format!(
                "AppDebugKit operation: {} (not yet fully implemented)",
                p.operation_name
            ));
            return Ok(());
        }
        if matches!(node_type, NodeType::AiStep(_)) {
            return Ok(());
        }

        let invocation = tool_mapping::node_type_to_tool_invocation(node_type)
            .map_err(|e| format!("Tool mapping failed: {}", e))?;
        let tool_name = &invocation.name;

        if let NodeType::McpToolCall(p) = node_type
            && p.tool_name.is_empty()
        {
            return Err("McpToolCall has empty tool_name".to_string());
        }

        self.record_event(
            node_run.as_deref(),
            "tool_call",
            serde_json::json!({"name": tool_name}),
        );

        self.log(format!("Calling MCP tool: {}", tool_name));
        let args = self.resolve_image_paths(Some(invocation.arguments));
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
}
