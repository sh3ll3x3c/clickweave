use clickweave_core::SkillRun;
use clickweave_core::TraceEvent;
use clickweave_core::storage::RunStorage;
use uuid::Uuid;

/// List all persisted run records for `skill_id`, sorted oldest-first.
pub fn list_runs(storage: &RunStorage, skill_id: &str) -> Vec<SkillRun> {
    storage.load_runs_for_skill(skill_id).unwrap_or_else(|e| {
        tracing::warn!(skill_id, error = %e, "Failed to load runs for skill");
        Vec::new()
    })
}

/// Load trace events for a specific skill run.
///
/// When `section_id` is `Some`, the events are currently returned unfiltered
/// (section-scoped slicing is out of scope for Phase 1). The parameter is
/// accepted so callers do not need to change their call sites when
/// per-section filtering lands.
pub fn load_run_events(
    storage: &RunStorage,
    skill_id: &str,
    run_id: Uuid,
    _section_id: Option<&str>,
) -> anyhow::Result<Vec<TraceEvent>> {
    let events_dir = storage.skill_run_events_dir(skill_id, run_id);
    let events_path = events_dir.join("events.jsonl");

    if !events_path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&events_path)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", events_path.display()))?;

    let mut events = Vec::new();
    for (lineno, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<TraceEvent>(line) {
            Ok(event) => events.push(event),
            Err(e) => {
                tracing::warn!(
                    path = %events_path.display(),
                    line = lineno + 1,
                    error = %e,
                    "Skipping unparseable trace event"
                );
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickweave_core::{TraceEventKind, storage::append_jsonl};

    fn make_storage(dir: &std::path::Path) -> RunStorage {
        RunStorage::new(dir, "test-wf")
    }

    #[test]
    fn list_runs_returns_empty_for_unknown_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());
        let runs = list_runs(&storage, "skl_nonexistent");
        assert!(runs.is_empty());
    }

    #[test]
    fn list_runs_returns_persisted_records() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());

        // Create two run records.
        let r1 = storage.create_skill_run("skl_abc").unwrap();
        let r2 = storage.create_skill_run("skl_abc").unwrap();

        let runs = list_runs(&storage, "skl_abc");
        assert_eq!(runs.len(), 2);
        // Both IDs must be present.
        let ids: Vec<_> = runs.iter().map(|r| r.run_id).collect();
        assert!(ids.contains(&r1.run_id));
        assert!(ids.contains(&r2.run_id));
    }

    #[test]
    fn load_run_events_returns_empty_when_no_events_file() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());
        let run = storage.create_skill_run("skl_noevents").unwrap();

        let events = load_run_events(&storage, "skl_noevents", run.run_id, None).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn load_run_events_parses_fixture_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = make_storage(tmp.path());
        let run = storage.create_skill_run("skl_evts").unwrap();

        // Write a fixture events.jsonl.
        let events_dir = storage.skill_run_events_dir("skl_evts", run.run_id);
        std::fs::create_dir_all(&events_dir).unwrap();
        let event = TraceEvent {
            timestamp: 42,
            event_type: TraceEventKind::ToolCall,
            payload: serde_json::json!({"tool": "click"}),
        };
        append_jsonl(&events_dir.join("events.jsonl"), &event).unwrap();

        let loaded = load_run_events(&storage, "skl_evts", run.run_id, None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].timestamp, 42);
        assert_eq!(loaded[0].event_type, TraceEventKind::ToolCall);
    }
}
