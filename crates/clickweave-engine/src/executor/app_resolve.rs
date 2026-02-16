use super::{ResolvedApp, WorkflowExecutor};
use clickweave_core::{FocusMethod, NodeRun, NodeType};
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
        if let Some(cached) = self
            .app_cache
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(user_input)
            .cloned()
        {
            debug!(user_input, resolved_name = %cached.name, "app_cache hit");
            self.log(format!(
                "App resolved (cached): \"{}\" -> {} (pid {})",
                user_input, cached.name, cached.pid
            ));
            return Ok(cached);
        }

        let apps_result = mcp
            .call_tool(
                "list_apps",
                Some(serde_json::json!({"user_apps_only": true})),
            )
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;
        let windows_result = mcp
            .call_tool("list_windows", None)
            .await
            .map_err(|e| format!("Failed to list windows: {}", e))?;

        let apps_text = Self::extract_result_text(&apps_result);
        let windows_text = Self::extract_result_text(&windows_result);

        // Short-circuit: if no apps are running, don't ask the LLM — it will hallucinate.
        let apps_trimmed = apps_text.trim();
        if apps_trimmed.is_empty() || apps_trimmed == "[]" || apps_trimmed == "No apps found" {
            return Err(format!(
                "App \"{}\" is not running (no matching apps found). \
                 Use launch_app to start it first.",
                user_input
            ));
        }

        let prompt = format!(
            "You are resolving an application name. The user wrote: \"{user_input}\"\n\
             \n\
             Running apps:\n\
             {apps_text}\n\
             \n\
             Visible windows:\n\
             {windows_text}\n\
             \n\
             Which running application does the user mean? Return ONLY a JSON object:\n\
             {{\"name\": \"<exact app name from the list above>\", \"pid\": <pid>}}\n\
             \n\
             IMPORTANT: The name MUST be an exact match from the Running apps list above.\n\
             Do NOT guess or invent app names. Do NOT return an unrelated app.\n\
             If no running app is a plausible match, return:\n\
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

        let json_text = extract_json_object(strip_code_block(raw_text))
            .ok_or_else(|| format!("No JSON object found in LLM response (raw: {})", raw_text))?;

        let parsed: Value = serde_json::from_str(json_text).map_err(|e| {
            format!(
                "Failed to parse LLM response as JSON: {} (raw: {})",
                e, raw_text
            )
        })?;

        let name = parsed["name"].as_str().ok_or_else(|| {
            format!(
                "App \"{}\" is not running (LLM found no match). \
                 Use launch_app to start it first.",
                user_input
            )
        })?;

        // Post-validate: ensure the LLM returned a name that actually appears in the app list.
        if !apps_text.contains(name) {
            return Err(format!(
                "App \"{}\" is not running (resolved name \"{}\" not found in app list). \
                 Use launch_app to start it first.",
                user_input, name
            ));
        }

        let pid = parsed["pid"].as_i64().ok_or_else(|| {
            format!(
                "LLM resolved name \"{}\" for \"{}\" but returned no PID",
                name, user_input
            )
        })? as i32;

        let resolved = ResolvedApp {
            name: name.to_string(),
            pid,
        };

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

        self.app_cache
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(user_input.to_string(), resolved.clone());

        Ok(resolved)
    }

    /// Remove a cached app resolution so the next attempt re-resolves via LLM.
    pub(crate) fn evict_app_cache(&self, user_input: &str) {
        if self
            .app_cache
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(user_input)
            .is_some()
        {
            debug!(user_input, "evicted app_cache entry");
            self.log(format!("App cache evicted for \"{}\"", user_input));
        }
    }

    /// Evict any app-name and element-name cache entries associated with a
    /// node type, so that retries re-resolve via LLM.
    pub(crate) fn evict_caches_for_node(&self, node_type: &NodeType) {
        let key = match node_type {
            NodeType::FocusWindow(p) if p.method == FocusMethod::AppName => p.value.as_deref(),
            NodeType::TakeScreenshot(p) => p.target.as_deref(),
            _ => None,
        };
        if let Some(key) = key {
            self.evict_app_cache(key);
        }

        // Evict element cache before clearing focused_app, so the cache key
        // still contains the correct app name.
        let (element_target, explicit_app) = match node_type {
            NodeType::Click(p) => (p.target.as_deref(), None),
            NodeType::FindText(p) => (Some(p.search_text.as_str()), None),
            NodeType::McpToolCall(p) if p.tool_name == "find_text" => (
                p.arguments.get("text").and_then(|v| v.as_str()),
                p.arguments.get("app_name").and_then(|v| v.as_str()),
            ),
            _ => (None, None),
        };
        if let Some(target) = element_target {
            // Prefer explicit app_name from call args; fall back to focused_app.
            let app_name = explicit_app.map(|s| s.to_string()).or_else(|| {
                self.focused_app
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
            });
            self.evict_element_cache(target, app_name.as_deref());
        }

        if matches!(node_type, NodeType::FocusWindow(_)) {
            *self.focused_app.write().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
}

/// Extract the first top-level `{…}` JSON object from `text`, ignoring any
/// leading or trailing prose the LLM may have added around it.
pub(crate) fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;
    for (i, ch) in text[start..].char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        if in_string {
            match ch {
                '\\' => escape_next = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Strip optional markdown code fences (```` ```json ... ``` ```` or ```` ``` ... ``` ````)
/// so we can parse the inner JSON.
pub(crate) fn strip_code_block(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    // Skip any language tag on the opening fence line
    let rest = match rest.find('\n') {
        Some(pos) => &rest[pos + 1..],
        None => rest,
    };
    rest.strip_suffix("```").unwrap_or(rest).trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_code_block_bare_json() {
        let input = r#"{"name": "Foo", "pid": 1}"#;
        assert_eq!(strip_code_block(input), input);
    }

    #[test]
    fn strip_code_block_with_json_fence() {
        let input = "```json\n{\"name\": \"Foo\", \"pid\": 1}\n```";
        assert_eq!(strip_code_block(input), r#"{"name": "Foo", "pid": 1}"#);
    }

    #[test]
    fn strip_code_block_with_plain_fence() {
        let input = "```\n{\"name\": \"Bar\", \"pid\": 42}\n```";
        assert_eq!(strip_code_block(input), r#"{"name": "Bar", "pid": 42}"#);
    }

    #[test]
    fn strip_code_block_with_extra_whitespace() {
        let input = "  \n```json\n  {\"name\": \"Baz\", \"pid\": 7}  \n```\n  ";
        assert_eq!(strip_code_block(input), r#"{"name": "Baz", "pid": 7}"#);
    }

    #[test]
    fn strip_code_block_uppercase_json_tag() {
        let input = "```JSON\n{\"name\": \"Qux\", \"pid\": 99}\n```";
        assert_eq!(strip_code_block(input), r#"{"name": "Qux", "pid": 99}"#);
    }

    #[test]
    fn strip_code_block_missing_closing_fence() {
        let input = "```json\n{\"name\": \"Open\", \"pid\": 5}";
        assert_eq!(strip_code_block(input), r#"{"name": "Open", "pid": 5}"#);
    }

    #[test]
    fn strip_code_block_multiline_json() {
        let input = "```json\n{\n  \"name\": \"Multi\",\n  \"pid\": 3\n}\n```";
        let expected = "{\n  \"name\": \"Multi\",\n  \"pid\": 3\n}";
        assert_eq!(strip_code_block(input), expected);
    }

    #[test]
    fn strip_code_block_arbitrary_language_tag() {
        let input = "```text\n{\"name\": \"Any\", \"pid\": 10}\n```";
        assert_eq!(strip_code_block(input), r#"{"name": "Any", "pid": 10}"#);
    }

    #[test]
    fn strip_code_block_only_whitespace_around_bare_json() {
        let input = "   {\"name\": \"Trim\", \"pid\": 0}   ";
        assert_eq!(strip_code_block(input), r#"{"name": "Trim", "pid": 0}"#);
    }

    #[test]
    fn extract_json_object_clean() {
        let input = r#"{"name": "Calculator", "pid": 70392}"#;
        assert_eq!(extract_json_object(input), Some(input));
    }

    #[test]
    fn extract_json_object_with_trailing_prose() {
        let input = r#"{"name": "Calculator", "pid": 70392}

The user's query specifically mentions the application name as "Calculator"."#;
        assert_eq!(
            extract_json_object(input),
            Some(r#"{"name": "Calculator", "pid": 70392}"#)
        );
    }

    #[test]
    fn extract_json_object_with_leading_prose() {
        let input = r#"Here is the result:
{"name": "Safari", "pid": 1234}"#;
        assert_eq!(
            extract_json_object(input),
            Some(r#"{"name": "Safari", "pid": 1234}"#)
        );
    }

    #[test]
    fn extract_json_object_with_nested_braces_in_string() {
        let input = r#"{"name": "App {v2}", "pid": 42} trailing"#;
        assert_eq!(
            extract_json_object(input),
            Some(r#"{"name": "App {v2}", "pid": 42}"#)
        );
    }

    #[test]
    fn extract_json_object_with_escaped_quotes() {
        let input = r#"{"name": "say \"hello\"", "pid": 7} extra"#;
        assert_eq!(
            extract_json_object(input),
            Some(r#"{"name": "say \"hello\"", "pid": 7}"#)
        );
    }

    #[test]
    fn extract_json_object_no_object() {
        assert_eq!(extract_json_object("no json here"), None);
    }

    #[test]
    fn extract_json_object_unclosed() {
        assert_eq!(extract_json_object("{\"name\": \"bad\""), None);
    }
}
