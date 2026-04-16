//! Privacy settings helpers shared between app startup (run-trace
//! cleanup) and the per-run `store_traces` kill switch.
//!
//! The UI persists privacy settings through `tauri-plugin-store` into a
//! `settings.json` file under the app config dir. For the macOS /
//! Windows / Linux app-data-dir conventions this project uses, that
//! path coincides with `app_data_dir()` — the same root `runs/` lives
//! under — so the helpers here read the raw JSON directly instead of
//! pulling in the plugin runtime before the Tauri event loop is up.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Default retention window when the UI has not written a value yet or
/// the JSON file is missing. Matches the UI default in
/// `ui/src/store/settings.ts`.
pub const DEFAULT_TRACE_RETENTION_DAYS: u64 = 30;

/// Privacy fields the Rust side cares about at startup.
/// Mirrors the subset of `PersistedSettings` declared in the UI.
///
/// Field names are camelCase to match how the UI's Tauri plugin-store
/// serialises them in `settings.json`. Only fields the Rust side reads
/// at startup are modelled here — the `storeTraces` kill switch is
/// shipped per-run through `RunRequest` / `AgentRunRequest`, so it
/// lives with the IPC payloads rather than here.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPrivacy {
    #[serde(default)]
    pub trace_retention_days: Option<u64>,
}

/// Location of the plugin-store's `settings.json` on disk.
fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("settings.json")
}

/// Read the privacy-related subset of persisted settings from disk.
///
/// Missing file, unreadable file, or malformed JSON all resolve to the
/// default (empty) struct — the setting falls through to the caller's
/// compiled-in default. Failing closed to "no cleanup" is the safe
/// behaviour when settings can't be parsed.
pub fn load_privacy_settings(app_data_dir: &Path) -> PersistedPrivacy {
    let path = settings_path(app_data_dir);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return PersistedPrivacy::default();
    };
    serde_json::from_str(&raw).unwrap_or_else(|e| {
        tracing::warn!(
            path = %path.display(),
            error = %e,
            "Failed to parse settings.json for privacy lookup — using defaults",
        );
        PersistedPrivacy::default()
    })
}

/// Invoke `cleanup_expired_runs` for the app-data runs directory with
/// the persisted retention window. Silent best-effort — any I/O error
/// is logged and swallowed so app startup is never blocked by trace
/// housekeeping.
pub fn sweep_expired_app_data_runs(app_data_dir: &Path) {
    let privacy = load_privacy_settings(app_data_dir);
    let retention_days = privacy
        .trace_retention_days
        .unwrap_or(DEFAULT_TRACE_RETENTION_DAYS);
    if retention_days == 0 {
        tracing::debug!("Trace retention disabled (0 days) — skipping cleanup sweep");
        return;
    }
    let runs_root = app_data_dir.join("runs");
    let now = chrono::Utc::now();
    match clickweave_core::storage::cleanup_expired_runs(&runs_root, retention_days, now) {
        Ok(removed) if removed.is_empty() => {
            tracing::debug!(
                runs_root = %runs_root.display(),
                retention_days,
                "Trace cleanup found no expired execution dirs",
            );
        }
        Ok(removed) => {
            tracing::info!(
                runs_root = %runs_root.display(),
                retention_days,
                removed_count = removed.len(),
                "Expired run traces cleaned up",
            );
        }
        Err(e) => {
            tracing::warn!(
                runs_root = %runs_root.display(),
                error = %e,
                "Trace cleanup sweep failed",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        std::env::temp_dir()
            .join("clickweave_privacy_test")
            .join(uuid::Uuid::new_v4().to_string())
    }

    #[test]
    fn load_privacy_settings_missing_file_returns_default() {
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let p = load_privacy_settings(&dir);
        assert!(p.trace_retention_days.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_privacy_settings_malformed_json_returns_default() {
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), b"not json").unwrap();
        let p = load_privacy_settings(&dir);
        assert!(p.trace_retention_days.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_privacy_settings_reads_retention_camel_case_field() {
        let dir = tmp();
        std::fs::create_dir_all(&dir).unwrap();
        let payload = serde_json::json!({
            "traceRetentionDays": 7,
            "somethingElse": "ignored",
        });
        std::fs::write(dir.join("settings.json"), payload.to_string()).unwrap();
        let p = load_privacy_settings(&dir);
        assert_eq!(p.trace_retention_days, Some(7));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
