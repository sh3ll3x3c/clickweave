use super::WorkflowExecutor;
use clickweave_core::{
    ClickParams, FocusMethod, FocusWindowParams, NodeRun, NodeType, ScreenshotMode,
    TakeScreenshotParams, tool_mapping,
};
use clickweave_llm::ChatBackend;
use clickweave_mcp::{McpClient, ToolCallResult};
use serde_json::Value;

impl<C: ChatBackend> WorkflowExecutor<C> {
    pub(crate) async fn execute_deterministic(
        &self,
        node_type: &NodeType,
        mcp: &McpClient,
        mut node_run: Option<&mut NodeRun>,
    ) -> Result<Value, String> {
        if let NodeType::AppDebugKitOp(p) = node_type {
            self.log(format!("AppDebugKit operation: {}", p.operation_name));
            let args = if p.parameters.is_null() {
                None
            } else {
                Some(p.parameters.clone())
            };
            self.record_event(
                node_run.as_deref(),
                "tool_call",
                serde_json::json!({"name": p.operation_name, "args": args}),
            );
            let result = mcp
                .call_tool(&p.operation_name, args)
                .await
                .map_err(|e| format!("AppDebugKit op {} failed: {}", p.operation_name, e))?;
            Self::check_tool_error(&result, &p.operation_name)?;
            let result_text = Self::extract_result_text(&result);
            self.record_event(
                node_run.as_deref(),
                "tool_result",
                serde_json::json!({
                    "name": p.operation_name,
                    "text": Self::truncate_for_trace(&result_text, 8192),
                    "text_len": result_text.len(),
                }),
            );
            let parsed: Value =
                serde_json::from_str(&result_text).unwrap_or(if result_text.is_empty() {
                    Value::Null
                } else {
                    Value::String(result_text)
                });
            return Ok(parsed);
        }

        if let NodeType::McpToolCall(p) = node_type
            && p.tool_name.is_empty()
        {
            return Err("McpToolCall has empty tool_name".to_string());
        }

        let resolved_click;
        let effective = if let NodeType::Click(p) = node_type
            && p.target.is_some()
            && p.x.is_none()
        {
            resolved_click = self.resolve_click_target(mcp, p, &mut node_run).await?;
            &resolved_click
        } else {
            node_type
        };

        let resolved_fw;
        let effective = if let NodeType::FocusWindow(p) = effective
            && p.method == FocusMethod::AppName
            && p.value.is_some()
        {
            let user_input = p.value.as_deref().unwrap();
            let app = self
                .resolve_app_name(user_input, mcp, node_run.as_deref())
                .await?;
            *self.focused_app.write().unwrap_or_else(|e| e.into_inner()) = Some(app.name.clone());
            resolved_fw = NodeType::FocusWindow(FocusWindowParams {
                method: FocusMethod::Pid,
                value: Some(app.pid.to_string()),
                bring_to_front: p.bring_to_front,
            });
            &resolved_fw
        } else {
            effective
        };

        let resolved_ss;
        let effective = if let NodeType::TakeScreenshot(p) = effective
            && p.target.is_some()
            && p.mode == ScreenshotMode::Window
        {
            let user_input = p.target.as_deref().unwrap();
            let app = self
                .resolve_app_name(user_input, mcp, node_run.as_deref())
                .await?;
            resolved_ss = NodeType::TakeScreenshot(TakeScreenshotParams {
                mode: p.mode,
                target: Some(app.name.clone()),
                include_ocr: p.include_ocr,
            });
            &resolved_ss
        } else {
            effective
        };

        let invocation = tool_mapping::node_type_to_tool_invocation(effective)
            .map_err(|e| format!("Tool mapping failed: {}", e))?;
        let tool_name = &invocation.name;

        self.log(format!("Calling MCP tool: {}", tool_name));
        let args = self.resolve_image_paths(Some(invocation.arguments));

        self.record_event(
            node_run.as_deref(),
            "tool_call",
            serde_json::json!({"name": tool_name, "args": args}),
        );
        let result = mcp
            .call_tool(tool_name, args)
            .await
            .map_err(|e| format!("MCP tool {} failed: {}", tool_name, e))?;

        Self::check_tool_error(&result, tool_name)?;

        let images = self.save_result_images(&result, "result", &mut node_run);
        let result_text = Self::extract_result_text(&result);

        self.record_event(
            node_run.as_deref(),
            "tool_result",
            serde_json::json!({
                "name": tool_name,
                "text": Self::truncate_for_trace(&result_text, 8192),
                "text_len": result_text.len(),
                "image_count": images.len(),
            }),
        );

        self.log(format!(
            "Tool result: {} chars, {} images",
            result_text.len(),
            images.len()
        ));

        // Return parsed result for variable extraction
        let parsed: Value =
            serde_json::from_str(&result_text).unwrap_or(if result_text.is_empty() {
                Value::Null
            } else {
                Value::String(result_text)
            });
        Ok(parsed)
    }

    fn check_tool_error(result: &ToolCallResult, tool_name: &str) -> Result<(), String> {
        if result.is_error == Some(true) {
            let error_text = Self::extract_result_text(result);
            return Err(format!(
                "MCP tool {} returned error: {}",
                tool_name, error_text
            ));
        }
        Ok(())
    }

    async fn resolve_click_target(
        &self,
        mcp: &McpClient,
        params: &ClickParams,
        node_run: &mut Option<&mut NodeRun>,
    ) -> Result<NodeType, String> {
        let target = params
            .target
            .as_deref()
            .ok_or("resolve_click_target called with no target")?;

        let mut find_args = serde_json::json!({"text": target});
        let scoped_app = self
            .focused_app
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(ref app_name) = scoped_app {
            find_args["app_name"] = serde_json::Value::String(app_name.clone());
        }

        match &scoped_app {
            Some(app) => self.log(format!("Resolving click target: '{}' in '{}'", target, app)),
            None => self.log(format!(
                "Resolving click target: '{}' (screen-wide)",
                target
            )),
        }

        let find_result = mcp
            .call_tool("find_text", Some(find_args))
            .await
            .map_err(|e| format!("find_text for '{}' failed: {}", target, e))?;

        Self::check_tool_error(&find_result, "find_text")?;

        let result_text = Self::extract_result_text(&find_result);
        let matches: Vec<Value> = serde_json::from_str(&result_text).unwrap_or_default();

        let best = matches.first().ok_or_else(|| {
            format!(
                "Could not find text '{}' on screen (find_text returned: {})",
                target,
                truncate_for_error(&result_text, 120),
            )
        })?;

        let x = best["x"]
            .as_f64()
            .ok_or_else(|| format!("find_text match for '{}' missing 'x' coordinate", target))?;
        let y = best["y"]
            .as_f64()
            .ok_or_else(|| format!("find_text match for '{}' missing 'y' coordinate", target))?;
        let matched_text = best["text"].as_str().unwrap_or(target);

        self.log(format!(
            "Resolved target '{}' -> ({}, {}) from '{}'",
            target, x, y, matched_text
        ));

        self.record_event(
            node_run.as_deref(),
            "target_resolved",
            serde_json::json!({
                "target": target,
                "x": x,
                "y": y,
                "matched_text": matched_text,
                "app_name": scoped_app,
            }),
        );

        Ok(NodeType::Click(ClickParams {
            target: params.target.clone(),
            x: Some(x),
            y: Some(y),
            button: params.button,
            click_count: params.click_count,
        }))
    }
}

fn truncate_for_error(s: &str, max_len: usize) -> &str {
    match s.char_indices().nth(max_len) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
