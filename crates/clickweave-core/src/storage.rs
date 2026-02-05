use crate::{Artifact, ArtifactKind, NodeRun, RunStatus, TraceEvent, TraceLevel};
use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

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

    fn now_millis() -> u64 {
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
            events: vec![],
            artifacts: vec![],
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

        use std::io::Write;
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
            overlays: vec![],
        };

        Ok(artifact)
    }

    pub fn load_runs_for_node(&self, node_id: Uuid) -> Result<Vec<NodeRun>> {
        let node_dir = self.base_path.join(node_id.to_string());
        if !node_dir.exists() {
            return Ok(vec![]);
        }

        let mut runs = Vec::new();
        let entries = std::fs::read_dir(&node_dir).context("Failed to read node run directory")?;

        for entry in entries {
            let entry = entry?;
            let run_json = entry.path().join("run.json");
            if run_json.exists() {
                let data = std::fs::read_to_string(&run_json).context("Failed to read run.json")?;
                let run: NodeRun =
                    serde_json::from_str(&data).context("Failed to parse run.json")?;
                runs.push(run);
            }
        }

        runs.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        Ok(runs)
    }

    pub fn load_run(&self, node_id: Uuid, run_id: Uuid) -> Result<NodeRun> {
        let run_json = self.run_dir(node_id, run_id).join("run.json");
        let data = std::fs::read_to_string(&run_json).context("Failed to read run.json")?;
        let run: NodeRun = serde_json::from_str(&data).context("Failed to parse run.json")?;
        Ok(run)
    }
}
