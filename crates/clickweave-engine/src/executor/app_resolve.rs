use super::{ResolvedApp, WorkflowExecutor};
use clickweave_core::NodeRun;
use clickweave_llm::{ChatBackend, Message};
use clickweave_mcp::McpClient;
use serde_json::Value;
use tracing::debug;

impl<C: ChatBackend> WorkflowExecutor<C> {
    /// Resolve a user-provided app name (e.g. "chrome", "my editor") to a concrete
    /// running application by asking the orchestrator LLM to match against the
    /// live list of apps and windows.  Results are cached so repeated references
    /// to the same user string only incur one LLM call.
    pub(crate) async fn resolve_app_name(
        &self,
        user_input: &str,
        mcp: &McpClient,
        node_run: Option<&NodeRun>,
    ) -> Result<ResolvedApp, String> {
        // Check cache first
        if let Some(cached) = self.app_cache.borrow().get(user_input).cloned() {
            debug!(
                user_input,
                resolved_name = %cached.name,
                "app_cache hit"
            );
            self.log(format!(
                "App resolved (cached): \"{}\" -> {} (pid {})",
                user_input, cached.name, cached.pid
            ));
            return Ok(cached);
        }

        // Fetch live app and window lists from the MCP server
        let apps_result = mcp
            .call_tool("list_apps", None)
            .map_err(|e| format!("Failed to list apps: {}", e))?;
        let windows_result = mcp
            .call_tool("list_windows", None)
            .map_err(|e| format!("Failed to list windows: {}", e))?;

        let apps_text = Self::extract_result_text(&apps_result);
        let windows_text = Self::extract_result_text(&windows_result);

        // Build the LLM prompt
        let prompt = format!(
            "You are resolving an application name. The user wrote: \"{user_input}\"\n\
             \n\
             Running apps:\n\
             {apps_text}\n\
             \n\
             Visible windows:\n\
             {windows_text}\n\
             \n\
             Which application does the user mean? Return ONLY a JSON object:\n\
             {{\"name\": \"<exact app name>\", \"pid\": <pid>}}\n\
             \n\
             If no match found, return:\n\
             {{\"name\": null, \"pid\": null}}"
        );

        let messages = vec![Message::user(prompt)];
        let response = self
            .agent
            .chat(messages, None)
            .await
            .map_err(|e| format!("LLM error during app resolution: {}", e))?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| "No response from LLM during app resolution".to_string())?;

        let raw_text = choice
            .message
            .content_text()
            .ok_or_else(|| "LLM returned empty content during app resolution".to_string())?;

        // Strip markdown code fences if present
        let json_text = strip_code_block(raw_text);

        let parsed: Value = serde_json::from_str(json_text).map_err(|e| {
            format!(
                "Failed to parse LLM response as JSON: {} (raw: {})",
                e, raw_text
            )
        })?;

        let name = parsed.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            format!(
                "LLM could not resolve app for \"{}\": name is null or missing",
                user_input
            )
        })?;

        let pid = parsed.get("pid").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

        let resolved = ResolvedApp {
            name: name.to_string(),
            pid,
        };

        // Record trace event
        self.record_event(
            node_run,
            "app_resolved",
            serde_json::json!({
                "user_input": user_input,
                "resolved_name": resolved.name,
                "resolved_pid": resolved.pid,
            }),
        );

        self.log(format!(
            "App resolved: \"{}\" -> {} (pid {})",
            user_input, resolved.name, resolved.pid
        ));

        // Cache the result
        self.app_cache
            .borrow_mut()
            .insert(user_input.to_string(), resolved.clone());

        Ok(resolved)
    }

    /// Remove a cached app resolution, e.g. when a focus attempt fails and we
    /// want to re-resolve on retry.
    pub(crate) fn evict_app_cache(&self, user_input: &str) {
        let removed = self.app_cache.borrow_mut().remove(user_input).is_some();
        if removed {
            debug!(user_input, "evicted app_cache entry");
            self.log(format!("App cache evicted for \"{}\"", user_input));
        }
    }
}

/// Strip optional markdown code fences (```json ... ``` or ``` ... ```)
/// so we can parse the inner JSON.
fn strip_code_block(text: &str) -> &str {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Skip optional language tag on the first line
        let rest = rest
            .strip_prefix("json")
            .or_else(|| rest.strip_prefix("JSON"))
            .unwrap_or(rest);
        let rest = rest.trim_start_matches(['\r', '\n']);
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim();
        }
        return rest.trim();
    }
    trimmed
}
