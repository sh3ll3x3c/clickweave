use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::{Artifact, ArtifactKind, NodeRun, RunStatus, TraceEvent, TraceLevel};

/// Formats a run directory name as `YYYY-MM-DD_HH-MM-SS_<short_uuid>`.
fn format_run_dirname(started_at_ms: u64, run_id: Uuid) -> String {
    let ts = i64::try_from(started_at_ms).ok();
    let dt = ts
        .and_then(DateTime::from_timestamp_millis)
        .unwrap_or_default();
    let short_id = &run_id.to_string()[..12];
    format!("{}_{short_id}", dt.format("%Y-%m-%d_%H-%M-%S"))
}

/// Manages on-disk storage for node run artifacts and trace data.
///
/// Directory layout:
/// ```text
/// <project>/.clickweave/runs/<workflow_id>/<node_id>/<YYYY-MM-DD_HH-MM-SS_shortid>/
///   run.json
///   events.jsonl
///   artifacts/
///     before.png
///     after.png
///     toolcall_0_screenshot.png
///     toolcall_0_ocr.json
/// ```
pub struct RunStorage {
    base_path: PathBuf,
}

impl RunStorage {
    pub fn new(project_path: &Path, workflow_id: Uuid) -> Self {
        Self {
            base_path: project_path
                .join(".clickweave")
                .join("runs")
                .join(workflow_id.to_string()),
        }
    }

    fn node_dir(&self, node_id: Uuid) -> PathBuf {
        self.base_path.join(node_id.to_string())
    }

    /// Deterministic path for a run whose metadata is known.
    pub fn run_dir(&self, run: &NodeRun) -> PathBuf {
        self.node_dir(run.node_id)
            .join(format_run_dirname(run.started_at, run.run_id))
    }

    /// Finds an existing run directory by scanning the node directory.
    ///
    /// Matches the new datetime-prefixed format first, then falls back to
    /// the legacy bare-UUID format for backward compatibility.
    pub fn find_run_dir(&self, node_id: Uuid, run_id: Uuid) -> Result<PathBuf> {
        let node_dir = self.node_dir(node_id);
        if !node_dir.exists() {
            anyhow::bail!("Node directory not found for node {node_id}");
        }

        let run_str = run_id.to_string();
        let short_suffix = format!("_{}", &run_str[..12]);

        for entry in std::fs::read_dir(&node_dir).context("Failed to read node directory")? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(&short_suffix) || *name == run_str {
                return Ok(entry.path());
            }
        }

        anyhow::bail!("Run directory not found for run {run_id}")
    }

    pub(crate) fn now_millis() -> u64 {
        Utc::now().timestamp_millis() as u64
    }

    pub fn create_run(&self, node_id: Uuid, trace_level: TraceLevel) -> Result<NodeRun> {
        let run_id = Uuid::new_v4();
        let started_at = Self::now_millis();
        let dir = self
            .node_dir(node_id)
            .join(format_run_dirname(started_at, run_id));
        std::fs::create_dir_all(dir.join("artifacts")).context("Failed to create run directory")?;

        let run = NodeRun {
            run_id,
            node_id,
            started_at,
            ended_at: None,
            status: RunStatus::Ok,
            trace_level,
            events: Vec::new(),
            artifacts: Vec::new(),
            observed_summary: None,
        };

        self.save_run(&run)?;
        Ok(run)
    }

    pub fn save_run(&self, run: &NodeRun) -> Result<()> {
        let dir = self.run_dir(run);
        std::fs::create_dir_all(&dir).context("Failed to create run directory")?;

        let json = serde_json::to_string_pretty(run).context("Failed to serialize run")?;
        std::fs::write(dir.join("run.json"), json).context("Failed to write run.json")?;
        Ok(())
    }

    pub fn append_event(&self, run: &NodeRun, event: &TraceEvent) -> Result<()> {
        let events_path = self.run_dir(run).join("events.jsonl");

        let mut line = serde_json::to_string(event).context("Failed to serialize event")?;
        line.push('\n');

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&events_path)
            .context("Failed to open events.jsonl")?;
        file.write_all(line.as_bytes())
            .context("Failed to write event")?;

        Ok(())
    }

    pub fn save_artifact(
        &self,
        run: &NodeRun,
        kind: ArtifactKind,
        filename: &str,
        data: &[u8],
        metadata: Value,
    ) -> Result<Artifact> {
        let artifact_path = self.run_dir(run).join("artifacts").join(filename);

        std::fs::write(&artifact_path, data).context("Failed to write artifact")?;

        let artifact = Artifact {
            artifact_id: Uuid::new_v4(),
            kind,
            path: artifact_path.to_string_lossy().to_string(),
            metadata,
            overlays: Vec::new(),
        };

        Ok(artifact)
    }

    pub fn load_runs_for_node(&self, node_id: Uuid) -> Result<Vec<NodeRun>> {
        let node_dir = self.node_dir(node_id);
        if !node_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&node_dir).context("Failed to read node run directory")?;
        let mut runs = Vec::new();

        for entry in entries {
            let run_json = entry?.path().join("run.json");
            if run_json.exists() {
                runs.push(Self::read_run_json(&run_json)?);
            }
        }

        runs.sort_by_key(|r| r.started_at);
        Ok(runs)
    }

    pub fn load_run(&self, node_id: Uuid, run_id: Uuid) -> Result<NodeRun> {
        let run_dir = self.find_run_dir(node_id, run_id)?;
        Self::read_run_json(&run_dir.join("run.json"))
    }

    fn read_run_json(path: &Path) -> Result<NodeRun> {
        let data = std::fs::read_to_string(path).context("Failed to read run.json")?;
        serde_json::from_str(&data).context("Failed to parse run.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_storage() -> (RunStorage, PathBuf, Uuid, Uuid) {
        let workflow_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let dir = std::env::temp_dir()
            .join("clickweave_test")
            .join(Uuid::new_v4().to_string());
        let storage = RunStorage::new(&dir, workflow_id);
        (storage, dir, workflow_id, node_id)
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_create_and_load_run() {
        let (storage, dir, _, node_id) = temp_storage();

        let run = storage
            .create_run(node_id, crate::TraceLevel::Minimal)
            .expect("create run");
        assert_eq!(run.node_id, node_id);
        assert_eq!(run.status, crate::RunStatus::Ok);

        let loaded = storage.load_run(node_id, run.run_id).expect("load run");
        assert_eq!(loaded.run_id, run.run_id);
        assert_eq!(loaded.node_id, node_id);

        cleanup(&dir);
    }

    #[test]
    fn test_save_and_load_run() {
        let (storage, dir, _, node_id) = temp_storage();

        let mut run = storage
            .create_run(node_id, crate::TraceLevel::Full)
            .expect("create run");
        run.status = crate::RunStatus::Failed;
        run.ended_at = Some(RunStorage::now_millis());
        storage.save_run(&run).expect("save run");

        let loaded = storage.load_run(node_id, run.run_id).expect("load run");
        assert_eq!(loaded.status, crate::RunStatus::Failed);
        assert!(loaded.ended_at.is_some());

        cleanup(&dir);
    }

    #[test]
    fn test_append_event() {
        let (storage, dir, _, node_id) = temp_storage();

        let run = storage
            .create_run(node_id, crate::TraceLevel::Minimal)
            .expect("create run");

        let event = TraceEvent {
            timestamp: RunStorage::now_millis(),
            event_type: "test_event".to_string(),
            payload: serde_json::json!({"key": "value"}),
        };
        storage.append_event(&run, &event).expect("append event");

        // Verify the events.jsonl file exists and has content
        let events_path = storage.run_dir(&run).join("events.jsonl");
        let content = std::fs::read_to_string(&events_path).expect("read events");
        assert!(content.contains("test_event"));

        cleanup(&dir);
    }

    #[test]
    fn test_save_artifact() {
        let (storage, dir, _, node_id) = temp_storage();

        let run = storage
            .create_run(node_id, crate::TraceLevel::Full)
            .expect("create run");

        let data = b"fake image data";
        let artifact = storage
            .save_artifact(
                &run,
                ArtifactKind::Screenshot,
                "test.png",
                data,
                Value::Null,
            )
            .expect("save artifact");

        assert_eq!(artifact.kind, ArtifactKind::Screenshot);
        assert!(artifact.path.contains("test.png"));

        // Verify file exists
        assert!(std::path::Path::new(&artifact.path).exists());

        cleanup(&dir);
    }

    #[test]
    fn test_load_runs_for_node() {
        let (storage, dir, _, node_id) = temp_storage();

        // Create multiple runs
        storage
            .create_run(node_id, crate::TraceLevel::Minimal)
            .expect("create run 1");
        storage
            .create_run(node_id, crate::TraceLevel::Minimal)
            .expect("create run 2");

        let runs = storage.load_runs_for_node(node_id).expect("load runs");
        assert_eq!(runs.len(), 2);

        // Should be sorted by started_at
        assert!(runs[0].started_at <= runs[1].started_at);

        cleanup(&dir);
    }

    #[test]
    fn test_load_runs_for_nonexistent_node() {
        let (storage, dir, _, _) = temp_storage();
        let random_id = Uuid::new_v4();

        let runs = storage.load_runs_for_node(random_id).expect("load runs");
        assert!(runs.is_empty());

        cleanup(&dir);
    }

    #[test]
    fn test_format_run_dirname_produces_expected_format() {
        // 2026-02-13 16:30:00 UTC in milliseconds
        let ts_ms = 1_771_000_200_000u64;
        let run_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        let dirname = format_run_dirname(ts_ms, run_id);
        assert_eq!(dirname, "2026-02-13_16-30-00_550e8400-e29");
    }

    #[test]
    fn test_find_run_dir_locates_created_run() {
        let (storage, dir, _, node_id) = temp_storage();

        let run = storage
            .create_run(node_id, crate::TraceLevel::Minimal)
            .expect("create run");

        let found = storage
            .find_run_dir(node_id, run.run_id)
            .expect("find run dir");
        assert_eq!(found, storage.run_dir(&run));

        cleanup(&dir);
    }
}
