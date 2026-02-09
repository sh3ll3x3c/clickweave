use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde_json::Value;
use uuid::Uuid;

use crate::{Artifact, ArtifactKind, NodeRun, RunStatus, TraceEvent, TraceLevel};

/// Manages on-disk storage for node run artifacts and trace data.
///
/// Directory layout:
/// ```text
/// <project>/.clickweave/runs/<workflow_id>/<node_id>/<run_id>/
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

    pub fn run_dir(&self, node_id: Uuid, run_id: Uuid) -> PathBuf {
        self.base_path
            .join(node_id.to_string())
            .join(run_id.to_string())
    }

    pub(crate) fn now_millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn create_run(&self, node_id: Uuid, trace_level: TraceLevel) -> Result<NodeRun> {
        let run_id = Uuid::new_v4();
        let dir = self.run_dir(node_id, run_id);
        std::fs::create_dir_all(dir.join("artifacts")).context("Failed to create run directory")?;

        let run = NodeRun {
            run_id,
            node_id,
            started_at: Self::now_millis(),
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
        let dir = self.run_dir(run.node_id, run.run_id);
        std::fs::create_dir_all(&dir).context("Failed to create run directory")?;

        let json = serde_json::to_string_pretty(run).context("Failed to serialize run")?;
        std::fs::write(dir.join("run.json"), json).context("Failed to write run.json")?;
        Ok(())
    }

    pub fn append_event(&self, run: &NodeRun, event: &TraceEvent) -> Result<()> {
        let dir = self.run_dir(run.node_id, run.run_id);
        let events_path = dir.join("events.jsonl");

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
        let dir = self.run_dir(run.node_id, run.run_id);
        let artifact_path = dir.join("artifacts").join(filename);

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
        let node_dir = self.base_path.join(node_id.to_string());
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
        let run_json = self.run_dir(node_id, run_id).join("run.json");
        Self::read_run_json(&run_json)
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
        let events_path = storage.run_dir(node_id, run.run_id).join("events.jsonl");
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
}
