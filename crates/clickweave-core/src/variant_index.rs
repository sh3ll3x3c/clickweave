use serde::{Deserialize, Serialize};
use std::path::Path;

/// One-line summary of a run variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantEntry {
    pub execution_dir: String,
    pub diverged_at_step: Option<usize>,
    pub divergence_summary: String,
    pub success: bool,
}

/// Lightweight variant index — always loaded into agent context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VariantIndex {
    pub entries: Vec<VariantEntry>,
}

impl VariantIndex {
    /// Load from JSONL file.
    pub fn load(path: &Path) -> Self {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        let entries = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Self { entries }
    }

    /// Load from JSONL file, dropping entries whose `execution_dir`
    /// no longer exists under `workflow_dir`.
    ///
    /// This is the preferred loader for the agent context path: it
    /// enforces privacy at read-time regardless of how or why the
    /// referenced execution directory went away (startup retention
    /// sweep, manual cleanup, partial crash, schema migration).
    ///
    /// This is a pure read — it does NOT rewrite the file. On-disk
    /// compaction is delegated to `storage::cleanup_expired_runs`,
    /// which is the single writer for workflow-level index
    /// mutations so two threads cannot race on the same temp file.
    /// Stale lines not removed by the sweep (legacy orphans, manual
    /// cleanup) remain on disk but are invisible to agent context
    /// through this filter.
    pub fn load_existing(index_path: &Path, workflow_dir: &Path) -> Self {
        let content = match std::fs::read_to_string(index_path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        let entries = content
            .lines()
            .filter_map(|line| serde_json::from_str::<VariantEntry>(line).ok())
            .filter(|entry| workflow_dir.join(&entry.execution_dir).is_dir())
            .collect();
        Self { entries }
    }

    /// Append entry to JSONL file.
    pub fn append(path: &Path, entry: &VariantEntry) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::storage::append_jsonl(path, entry)
    }

    /// Format as compact text for agent context.
    pub fn as_context_text(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Past run variants".to_string()];
        for entry in &self.entries {
            let status = if entry.success { "ok" } else { "failed" };
            let diverged = entry
                .diverged_at_step
                .map(|s| format!(" (diverged at step {})", s))
                .unwrap_or_default();
            lines.push(format!(
                "- {}: {} [{}]{}",
                entry.execution_dir, entry.divergence_summary, status, diverged
            ));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("variant_index.jsonl");
        let entry = VariantEntry {
            execution_dir: "2026-04-10_14-00-00_abc".to_string(),
            diverged_at_step: Some(3),
            divergence_summary: "Modal appeared".to_string(),
            success: true,
        };
        VariantIndex::append(&path, &entry).unwrap();
        let loaded = VariantIndex::load(&path);
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].divergence_summary, "Modal appeared");
    }

    #[test]
    fn load_missing_file_returns_default() {
        let loaded = VariantIndex::load(std::path::Path::new("/nonexistent/path.jsonl"));
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn as_context_text_empty() {
        let index = VariantIndex::default();
        assert_eq!(index.as_context_text(), "");
    }

    #[test]
    fn as_context_text_formats_entries() {
        let index = VariantIndex {
            entries: vec![
                VariantEntry {
                    execution_dir: "2026-04-10_14-00-00_abc".to_string(),
                    diverged_at_step: Some(3),
                    divergence_summary: "Modal appeared".to_string(),
                    success: true,
                },
                VariantEntry {
                    execution_dir: "2026-04-10_15-00-00_def".to_string(),
                    diverged_at_step: None,
                    divergence_summary: "Followed reference trajectory".to_string(),
                    success: false,
                },
            ],
        };
        let text = index.as_context_text();
        assert!(text.starts_with("## Past run variants"));
        assert!(text.contains("[ok]"));
        assert!(text.contains("[failed]"));
        assert!(text.contains("(diverged at step 3)"));
    }

    #[test]
    fn append_multiple_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("variant_index.jsonl");

        for i in 0..3 {
            let entry = VariantEntry {
                execution_dir: format!("exec_{}", i),
                diverged_at_step: None,
                divergence_summary: format!("Run {}", i),
                success: i % 2 == 0,
            };
            VariantIndex::append(&path, &entry).unwrap();
        }

        let loaded = VariantIndex::load(&path);
        assert_eq!(loaded.entries.len(), 3);
        assert_eq!(loaded.entries[0].execution_dir, "exec_0");
        assert_eq!(loaded.entries[2].execution_dir, "exec_2");
    }

    #[test]
    fn load_existing_drops_entries_whose_execution_dir_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let workflow_dir = dir.path();
        let path = workflow_dir.join("variant_index.jsonl");

        // Only the first entry has a matching dir on disk. The
        // other two — even though they are valid JSON — reference
        // directories the load path must not surface.
        std::fs::create_dir_all(workflow_dir.join("kept_exec")).unwrap();
        for name in ["kept_exec", "ghost_exec", "gone_exec"] {
            let entry = VariantEntry {
                execution_dir: name.to_string(),
                diverged_at_step: None,
                divergence_summary: name.to_string(),
                success: true,
            };
            VariantIndex::append(&path, &entry).unwrap();
        }

        let loaded = VariantIndex::load_existing(&path, workflow_dir);
        assert_eq!(
            loaded.entries.len(),
            1,
            "only entries whose execution_dir exists should survive",
        );
        assert_eq!(loaded.entries[0].execution_dir, "kept_exec");

        // `load_existing` is a pure read — stale lines are invisible
        // to the in-memory view but remain on disk. On-disk compaction
        // is delegated to `storage::cleanup_expired_runs` so two
        // threads never race to rewrite the same file.
        let raw = VariantIndex::load(&path);
        assert_eq!(
            raw.entries.len(),
            3,
            "the on-disk file must not be rewritten by load_existing",
        );
    }

    #[test]
    fn load_existing_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let loaded =
            VariantIndex::load_existing(&dir.path().join("variant_index.jsonl"), dir.path());
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn load_existing_leaves_file_unchanged_when_every_entry_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let workflow_dir = dir.path();
        let path = workflow_dir.join("variant_index.jsonl");

        for name in ["gone_a", "gone_b"] {
            let entry = VariantEntry {
                execution_dir: name.to_string(),
                diverged_at_step: None,
                divergence_summary: name.to_string(),
                success: true,
            };
            VariantIndex::append(&path, &entry).unwrap();
        }
        let before = std::fs::read_to_string(&path).unwrap();

        let loaded = VariantIndex::load_existing(&path, workflow_dir);
        assert!(loaded.entries.is_empty());
        assert!(
            path.exists(),
            "on-disk file must persist — rewriting would race the startup sweep"
        );
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "load_existing must not rewrite the file");
    }

    #[test]
    fn load_existing_does_not_rewrite_when_nothing_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let workflow_dir = dir.path();
        let path = workflow_dir.join("variant_index.jsonl");
        std::fs::create_dir_all(workflow_dir.join("kept")).unwrap();

        let entry = VariantEntry {
            execution_dir: "kept".to_string(),
            diverged_at_step: None,
            divergence_summary: "ok".to_string(),
            success: true,
        };
        VariantIndex::append(&path, &entry).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        let _ = VariantIndex::load_existing(&path, workflow_dir);

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "clean file must not be rewritten");
    }
}
