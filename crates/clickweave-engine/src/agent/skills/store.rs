//! Filesystem-backed skill store.
//!
//! Skill files live on disk as `<slug>-v<N>.md`. Writes go through an
//! atomic `<slug>-v<N>.md.tmp` → rename so a partial file is never
//! visible to readers (or the file watcher). The store records the
//! timestamp of every successful write so the file watcher can skip
//! self-write events when flipping `edited_by_user` on external edits.
//!
//! Files are immutable once they appear under their final name: a
//! diverged action sketch lands as a fresh `(id, version + 1)` file
//! through `write_skill`. In-app rename / delete go through their own
//! helpers and update both the on-disk file and the recently-written
//! cache used by the watcher.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::frontmatter::{emit_skill_md, parse_skill_md};
use super::types::{Skill, SkillError};

const RECENT_WRITE_TOLERANCE: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveReport {
    pub moved: usize,
}

#[derive(Debug)]
pub struct SkillStore {
    dir: PathBuf,
    last_written: Mutex<HashMap<PathBuf, Instant>>,
}

impl SkillStore {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            last_written: Mutex::new(HashMap::new()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn list_files(&self) -> io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        if !self.dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn read_skill(&self, path: &Path) -> Result<Skill, SkillError> {
        let contents = fs::read_to_string(path)?;
        parse_skill_md(&contents)
    }

    pub fn write_skill(&self, skill: &Skill) -> Result<PathBuf, SkillError> {
        if !self.dir.exists() {
            fs::create_dir_all(&self.dir)?;
        }
        let final_path = self.dir.join(filename_for(skill));
        let tmp_path = self.dir.join(format!("{}.tmp", filename_for(skill)));
        let contents = emit_skill_md(skill)?;
        fs::write(&tmp_path, contents)?;
        fs::rename(&tmp_path, &final_path)?;
        self.record_write(&final_path);
        Ok(final_path)
    }

    pub fn delete_skill(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)?;
        self.record_write(path);
        Ok(())
    }

    /// In-app rename: write the new file under its current `(id,
    /// version)` filename, then drop the old file. The two writes are
    /// not transactional, but the watcher consumer treats both events
    /// as self-writes via `was_recently_written`.
    pub fn rename_skill_in_place(
        &self,
        old_path: &Path,
        skill: &Skill,
    ) -> Result<PathBuf, SkillError> {
        let new_path = self.write_skill(skill)?;
        if old_path != new_path && old_path.exists() {
            fs::remove_file(old_path)?;
            self.record_write(old_path);
        }
        Ok(new_path)
    }

    /// True if the store wrote (or deleted) `path` within the past
    /// `RECENT_WRITE_TOLERANCE`. The watcher consumer uses this to skip
    /// self-write events that would otherwise flip `edited_by_user`.
    pub fn was_recently_written(&self, path: &Path) -> bool {
        let mut guard = self.last_written.lock();
        // Opportunistic GC of stale entries — the table never grows
        // unbounded as long as the watcher drains regularly.
        guard.retain(|_, ts| ts.elapsed() <= RECENT_WRITE_TOLERANCE * 4);
        guard
            .get(path)
            .is_some_and(|ts| ts.elapsed() <= RECENT_WRITE_TOLERANCE)
    }

    fn record_write(&self, path: &Path) {
        self.last_written
            .lock()
            .insert(path.to_path_buf(), Instant::now());
    }
}

pub fn filename_for(skill: &Skill) -> String {
    format!("{}-v{}.md", slugify(&skill.id), skill.version)
}

pub fn move_skills_to_project(
    app_data_skills_root: &Path,
    project_uuid: &str,
    project_path: &Path,
) -> Result<MoveReport, SkillError> {
    let src = app_data_skills_root.join(project_uuid);
    if !src.exists() {
        return Ok(MoveReport { moved: 0 });
    }

    let moved = count_files(&src)?;
    let dest = project_path.join(".clickweave").join("skills");
    fs::create_dir_all(&dest)?;

    match fs::rename(&src, &dest) {
        Ok(()) => {}
        Err(_) => {
            copy_dir_with_integrity_check(&src, &dest)?;
            fs::remove_dir_all(&src)?;
        }
    }

    Ok(MoveReport { moved })
}

fn copy_dir_with_integrity_check(src: &Path, dest: &Path) -> Result<(), SkillError> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            copy_dir_with_integrity_check(&src_path, &dest_path)?;
        } else if file_type.is_file() {
            let bytes = fs::read(&src_path)?;
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&dest_path, &bytes)?;
            let written = fs::read(&dest_path)?;
            if written != bytes {
                return Err(SkillError::InvalidFrontmatter(format!(
                    "copied skill file integrity check failed for {}",
                    dest_path.display()
                )));
            }
        }
    }
    Ok(())
}

fn count_files(dir: &Path) -> Result<usize, SkillError> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            count += count_files(&entry.path())?;
        } else if file_type.is_file() {
            count += 1;
        }
    }
    Ok(count)
}

pub fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash && !out.is_empty() {
            out.push('-');
            last_was_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_lowercases_and_collapses_non_alphanumerics() {
        assert_eq!(slugify("Open Vesna's Chat!"), "open-vesna-s-chat");
        assert_eq!(slugify("multi   spaces"), "multi-spaces");
        assert_eq!(slugify("trailing!!!"), "trailing");
    }

    #[test]
    fn filename_for_combines_slug_and_version() {
        let skill = sample_skill_minimal("open-vesna-chat", 3);
        assert_eq!(filename_for(&skill), "open-vesna-chat-v3.md");
    }

    #[test]
    fn move_skills_to_project_moves_unsaved_skill_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data_skills_root = tmp.path().join("app-data").join("skills");
        let workflow_id = "550e8400-e29b-41d4-a716-446655440000";
        let src = app_data_skills_root.join(workflow_id);
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("alpha-v1.md"), b"alpha").unwrap();
        fs::write(src.join("nested").join("beta-v1.md"), b"beta").unwrap();

        let project = tmp.path().join("saved-project");
        let report = move_skills_to_project(&app_data_skills_root, workflow_id, &project).unwrap();

        assert_eq!(report, MoveReport { moved: 2 });
        assert!(!src.exists());
        assert_eq!(
            fs::read(project.join(".clickweave/skills/alpha-v1.md")).unwrap(),
            b"alpha"
        );
        assert_eq!(
            fs::read(project.join(".clickweave/skills/nested/beta-v1.md")).unwrap(),
            b"beta"
        );
    }

    #[test]
    fn move_skills_to_project_is_noop_when_unsaved_dir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let report = move_skills_to_project(
            &tmp.path().join("app-data").join("skills"),
            "missing",
            &tmp.path().join("saved-project"),
        )
        .unwrap();

        assert_eq!(report, MoveReport { moved: 0 });
        assert!(!tmp.path().join("saved-project/.clickweave/skills").exists());
    }

    fn sample_skill_minimal(id: &str, version: u32) -> Skill {
        use crate::agent::skills::types::*;
        Skill {
            id: id.into(),
            version,
            state: SkillState::Draft,
            scope: SkillScope::ProjectLocal,
            name: "test".into(),
            description: "desc".into(),
            tags: vec![],
            subgoal_text: "open chat".into(),
            subgoal_signature: SubgoalSignature("sig".into()),
            applicability: ApplicabilityHints {
                apps: vec![],
                hosts: vec![],
                signature: ApplicabilitySignature("appsig".into()),
            },
            parameter_schema: vec![],
            action_sketch: vec![],
            outputs: vec![],
            outcome_predicate: OutcomePredicate::SubgoalCompleted {
                post_state_world_model_signature: None,
            },
            provenance: vec![],
            stats: SkillStats::default(),
            edited_by_user: false,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            produced_node_ids: vec![],
            body: String::new(),
        }
    }
}
