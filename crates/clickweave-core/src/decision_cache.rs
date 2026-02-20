use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

/// Caches LLM decisions made during Test mode so they can be replayed in Run mode
/// without repeating the LLM calls.
///
/// Stored as `decisions.json` alongside the workflow's run directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecisionCache {
    pub version: u32,
    pub workflow_id: Uuid,
    /// Keyed by `"target\0app_name"` (NUL separator cannot appear in UI text).
    pub click_disambiguation: HashMap<String, ClickDisambiguation>,
    /// Keyed by `"target\0app_name"`.
    pub element_resolution: HashMap<String, ElementResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickDisambiguation {
    pub target: String,
    pub app_name: Option<String>,
    pub chosen_text: String,
    pub chosen_role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElementResolution {
    pub target: String,
    pub resolved_name: String,
}

/// Build a cache key from a target and optional app name.
/// Uses NUL as separator since it cannot appear in UI element text.
pub fn cache_key(target: &str, app_name: Option<&str>) -> String {
    match app_name {
        Some(app) => format!("{}\0{}", target, app),
        None => target.to_string(),
    }
}

impl DecisionCache {
    pub fn new(workflow_id: Uuid) -> Self {
        Self {
            version: 1,
            workflow_id,
            ..Default::default()
        }
    }

    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&data).ok()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create cache dir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize cache: {}", e))?;
        std::fs::write(path, json).map_err(|e| format!("Failed to write cache: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_with_app() {
        assert_eq!(cache_key("2", Some("Calculator")), "2\0Calculator");
    }

    #[test]
    fn cache_key_without_app() {
        assert_eq!(cache_key("Submit", None), "Submit");
    }

    #[test]
    fn round_trip_save_load() {
        let dir = std::env::temp_dir()
            .join("clickweave_test_cache")
            .join(Uuid::new_v4().to_string());
        let path = dir.join("decisions.json");

        let mut cache = DecisionCache::new(Uuid::new_v4());
        cache.click_disambiguation.insert(
            cache_key("2", Some("Calculator")),
            ClickDisambiguation {
                target: "2".to_string(),
                app_name: Some("Calculator".to_string()),
                chosen_text: "2".to_string(),
                chosen_role: "AXButton".to_string(),
            },
        );
        cache.element_resolution.insert(
            cache_key("×", Some("Calculator")),
            ElementResolution {
                target: "×".to_string(),
                resolved_name: "Multiply".to_string(),
            },
        );

        cache.save(&path).expect("save");
        let loaded = DecisionCache::load(&path).expect("load");

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.click_disambiguation.len(), 1);
        assert_eq!(loaded.element_resolution.len(), 1);

        let disambig = loaded
            .click_disambiguation
            .get(&cache_key("2", Some("Calculator")))
            .unwrap();
        assert_eq!(disambig.chosen_text, "2");
        assert_eq!(disambig.chosen_role, "AXButton");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_nonexistent_returns_none() {
        assert!(DecisionCache::load(std::path::Path::new("/nonexistent/path.json")).is_none());
    }
}
