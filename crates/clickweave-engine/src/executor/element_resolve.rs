use super::WorkflowExecutor;
use clickweave_core::NodeRun;
use clickweave_llm::{ChatBackend, Message};
use serde_json::Value;
use tracing::debug;

/// Extract the `available_elements` array from a find_text response.
///
/// When find_text returns no matches, the response is two content blocks
/// joined by `\n`: `[]\n{"available_elements": ["Multiply", "Divide", ...]}`.
pub(crate) fn parse_available_elements(result_text: &str) -> Option<Vec<String>> {
    let obj_start = result_text.find("{\"available_elements\"")?;
    let json_str = &result_text[obj_start..];
    let parsed: Value = serde_json::from_str(json_str).ok()?;
    let arr = parsed.get("available_elements")?.as_array()?;
    let elements: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();
    if elements.is_empty() {
        None
    } else {
        Some(elements)
    }
}

impl<C: ChatBackend> WorkflowExecutor<C> {
    /// Resolve a user-provided element name (e.g. "x", "÷") to the correct
    /// accessibility element name (e.g. "Multiply", "Divide") by asking the
    /// orchestrator LLM to match against the available elements list.
    /// Results are cached so repeated references to the same target only
    /// incur one LLM call.
    pub(crate) async fn resolve_element_name(
        &self,
        target: &str,
        available_elements: &[String],
        app_name: Option<&str>,
        node_run: Option<&NodeRun>,
    ) -> Result<String, String> {
        let cache_key = (target.to_string(), app_name.map(|s| s.to_string()));

        if let Some(cached) = self
            .element_cache
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&cache_key)
            .cloned()
        {
            debug!(target = target, resolved_name = %cached, "element_cache hit");
            self.log(format!(
                "Element resolved (cached): \"{}\" -> \"{}\"",
                target, cached
            ));
            return Ok(cached);
        }

        let app_context = match app_name {
            Some(name) => format!(" in app \"{}\"", name),
            None => String::new(),
        };

        let elements_list = available_elements
            .iter()
            .map(|e| format!("- {}", e))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are resolving a UI element name. The user wants to interact with: \"{target}\"{app_context}\n\
             \n\
             Available UI elements:\n\
             {elements_list}\n\
             \n\
             Which element name matches what the user means? Return ONLY a JSON object:\n\
             {{\"name\": \"<exact element name from the list above>\"}}\n\
             \n\
             IMPORTANT: The name MUST be an exact match from the list above.\n\
             Common mappings: \u{00d7} = Multiply, \u{00f7} = Divide, \u{2212} = Subtract, AC = All Clear.\n\
             If no element is a plausible match, return:\n\
             {{\"name\": null}}"
        );

        let messages = vec![Message::user(prompt)];
        let response = self
            .agent
            .chat(messages, None)
            .await
            .map_err(|e| format!("LLM error during element resolution: {}", e))?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| "No response from LLM during element resolution".to_string())?;

        let raw_text = choice
            .message
            .content_text()
            .ok_or_else(|| "LLM returned empty content during element resolution".to_string())?;

        let json_text =
            super::app_resolve::extract_json_object(super::app_resolve::strip_code_block(raw_text))
                .ok_or_else(|| {
                    format!("No JSON object found in LLM response (raw: {})", raw_text)
                })?;

        let parsed: Value = serde_json::from_str(json_text).map_err(|e| {
            format!(
                "Failed to parse LLM response as JSON: {} (raw: {})",
                e, raw_text
            )
        })?;

        let name = parsed["name"].as_str().ok_or_else(|| {
            format!(
                "Element \"{}\" not found in available elements (LLM found no match)",
                target
            )
        })?;

        // Post-validate: ensure the LLM returned a name that actually appears in the list.
        if !available_elements.iter().any(|e| e == name) {
            return Err(format!(
                "Element \"{}\" not found (resolved name \"{}\" not in available elements list)",
                target, name
            ));
        }

        self.record_event(
            node_run,
            "element_resolved",
            serde_json::json!({
                "target": target,
                "resolved_name": name,
                "app_name": app_name,
            }),
        );

        self.log(format!("Element resolved: \"{}\" -> \"{}\"", target, name));

        self.element_cache
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(cache_key, name.to_string());

        Ok(name.to_string())
    }

    /// Remove a cached element resolution so the next attempt re-resolves via LLM.
    pub(crate) fn evict_element_cache(&self, target: &str, app_name: Option<&str>) {
        let cache_key = (target.to_string(), app_name.map(|s| s.to_string()));
        if self
            .element_cache
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&cache_key)
            .is_some()
        {
            debug!(target = target, app_name = ?app_name, "evicted element_cache entry");
            self.log(format!("Element cache evicted for \"{}\"", target));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_available_elements_from_find_text_response() {
        let input = "[]\n{\"available_elements\":[\"Calculator\",\"Multiply\",\"Divide\"]}";
        assert_eq!(
            parse_available_elements(input),
            Some(vec![
                "Calculator".to_string(),
                "Multiply".to_string(),
                "Divide".to_string(),
            ])
        );
    }

    #[test]
    fn parse_available_elements_matches_only() {
        let input = "[{\"text\":\"2\",\"x\":100,\"y\":200}]";
        assert_eq!(parse_available_elements(input), None);
    }

    #[test]
    fn parse_available_elements_empty_string() {
        assert_eq!(parse_available_elements(""), None);
    }

    #[test]
    fn parse_available_elements_just_empty_array() {
        assert_eq!(parse_available_elements("[]"), None);
    }
}
