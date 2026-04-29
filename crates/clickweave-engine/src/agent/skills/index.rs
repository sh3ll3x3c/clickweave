//! In-memory skill index.
//!
//! Built once per agent run from the on-disk store and held behind an
//! `Arc<RwLock<_>>` shared between the runner, the file watcher
//! consumer, and the LLM-proposal task. Two lookup paths matter:
//! `(id, version)` for replay dispatch and `subgoal_signature` for
//! retrieval at `push_subgoal` boundaries.
//!
//! Phase 2 lands the build + lookup surface with a placeholder scoring
//! formula (`1.0` on signature match, `0.0` otherwise). Phase 3
//! replaces the scorer with the rich cross-tier merge that consumes
//! the embedder field on the index.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tracing::warn;

use super::store::SkillStore;
use super::types::{
    ApplicabilitySignature, RetrievedSkill, Skill, SkillContext, SkillError, SkillState,
    SubgoalSignature,
};
use crate::agent::episodic::HashedShingleEmbedder;

pub struct SkillIndex {
    by_id: HashMap<(String, u32), Arc<Skill>>,
    by_subgoal_signature: HashMap<SubgoalSignature, Vec<(String, u32)>>,
    embedder: Arc<HashedShingleEmbedder>,
    project_dir: PathBuf,
    global_dir: Option<PathBuf>,
}

impl std::fmt::Debug for SkillIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkillIndex")
            .field("len", &self.by_id.len())
            .field("project_dir", &self.project_dir)
            .field("global_dir", &self.global_dir)
            .finish()
    }
}

impl SkillIndex {
    pub fn build(
        ctx: &SkillContext,
        embedder: Arc<HashedShingleEmbedder>,
    ) -> Result<Self, SkillError> {
        let mut idx = Self::empty_with_paths(
            embedder,
            ctx.project_skills_dir.clone(),
            ctx.global_skills_dir.clone(),
        );
        if ctx.project_skills_dir.exists() {
            idx.load_dir(&ctx.project_skills_dir);
        }
        if let Some(global) = ctx.global_skills_dir.as_ref()
            && global.exists()
        {
            idx.load_dir(global);
        }
        Ok(idx)
    }

    pub fn empty(embedder: Arc<HashedShingleEmbedder>) -> Self {
        Self::empty_with_paths(embedder, PathBuf::new(), None)
    }

    fn empty_with_paths(
        embedder: Arc<HashedShingleEmbedder>,
        project_dir: PathBuf,
        global_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            by_id: HashMap::new(),
            by_subgoal_signature: HashMap::new(),
            embedder,
            project_dir,
            global_dir,
        }
    }

    fn load_dir(&mut self, dir: &PathBuf) {
        let store = SkillStore::new(dir.clone());
        match store.list_files() {
            Ok(paths) => {
                for path in paths {
                    match store.read_skill(&path) {
                        Ok(skill) => self.upsert(skill),
                        Err(err) => {
                            warn!(?path, ?err, "skill index: skipping malformed skill file");
                        }
                    }
                }
            }
            Err(err) => warn!(?dir, ?err, "skill index: list_files failed"),
        }
    }

    pub fn get(&self, id: &str, version: u32) -> Option<Arc<Skill>> {
        self.by_id.get(&(id.to_string(), version)).cloned()
    }

    pub fn upsert(&mut self, skill: Skill) {
        let key = (skill.id.clone(), skill.version);
        // Drop the previous (id, version)'s reverse-index entry so a
        // re-upsert (e.g. after the watcher consumer flips
        // edited_by_user) does not double-list under the same signature.
        let prev_sig = self.by_id.get(&key).map(|s| s.subgoal_signature.clone());
        if let Some(sig) = prev_sig {
            self.remove_subgoal_pointer(&sig, &key);
        }
        self.by_subgoal_signature
            .entry(skill.subgoal_signature.clone())
            .or_default()
            .push(key.clone());
        self.by_id.insert(key, Arc::new(skill));
    }

    pub fn remove(&mut self, id: &str, version: u32) {
        let key = (id.to_string(), version);
        if let Some(prev) = self.by_id.remove(&key) {
            let sig = prev.subgoal_signature.clone();
            self.remove_subgoal_pointer(&sig, &key);
        }
    }

    fn remove_subgoal_pointer(&mut self, sig: &SubgoalSignature, key: &(String, u32)) {
        if let Some(entries) = self.by_subgoal_signature.get_mut(sig) {
            entries.retain(|k| k != key);
            if entries.is_empty() {
                self.by_subgoal_signature.remove(sig);
            }
        }
    }

    /// Phase 2 lookup: return up to `k` skills whose `subgoal_signature`
    /// matches and whose state is `Confirmed` or `Promoted` (drafts are
    /// not retrieval-eligible). Scoring is a placeholder (`1.0` on
    /// match) — Phase 3's retrieval module replaces this with the rich
    /// formula and supplies cross-tier merging.
    pub fn lookup(
        &self,
        subgoal_sig: &SubgoalSignature,
        _applicability_sig: &ApplicabilitySignature,
        k: usize,
    ) -> Vec<RetrievedSkill> {
        if k == 0 {
            return Vec::new();
        }
        let Some(keys) = self.by_subgoal_signature.get(subgoal_sig) else {
            return Vec::new();
        };
        let mut out: Vec<RetrievedSkill> = keys
            .iter()
            .filter_map(|key| self.by_id.get(key))
            .filter(|skill| matches!(skill.state, SkillState::Confirmed | SkillState::Promoted))
            .map(|skill| RetrievedSkill {
                skill: skill.clone(),
                score: 1.0,
            })
            .collect();
        out.truncate(k);
        out
    }

    pub fn mark_invoked(&mut self, id: &str, version: u32, when: DateTime<Utc>) {
        let key = (id.to_string(), version);
        if let Some(entry) = self.by_id.get_mut(&key) {
            // `Arc<Skill>` shares state with retrieval consumers; clone
            // the inner value, mutate, and replace the Arc. Cheap
            // relative to the cost of the disk write that follows.
            let mut updated = (**entry).clone();
            updated.stats.last_invoked_at = Some(when);
            *entry = Arc::new(updated);
        }
    }

    pub fn skills_in_state(&self, state: SkillState) -> Vec<Arc<Skill>> {
        self.by_id
            .values()
            .filter(|skill| skill.state == state)
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn embedder(&self) -> &Arc<HashedShingleEmbedder> {
        &self.embedder
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::skills::types::{
        ApplicabilityHints, ApplicabilitySignature, OutcomePredicate, Skill, SkillScope, SkillStats,
    };

    fn skill_with(id: &str, version: u32, sig: &str, state: SkillState) -> Skill {
        Skill {
            id: id.into(),
            version,
            state,
            scope: SkillScope::ProjectLocal,
            name: id.into(),
            description: String::new(),
            tags: vec![],
            subgoal_text: format!("subgoal for {id}"),
            subgoal_signature: SubgoalSignature(sig.into()),
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
            created_at: Utc::now(),
            updated_at: Utc::now(),
            produced_node_ids: vec![],
            body: String::new(),
        }
    }

    #[test]
    fn empty_index_returns_no_candidates() {
        let idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        let sig = SubgoalSignature("missing".into());
        let app_sig = ApplicabilitySignature("appsig".into());
        assert!(idx.lookup(&sig, &app_sig, 5).is_empty());
    }

    #[test]
    fn build_over_temp_dir_loads_all_files() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().to_path_buf();
        let store = SkillStore::new(project_dir.clone());
        store
            .write_skill(&skill_with("a", 1, "sig-a", SkillState::Confirmed))
            .unwrap();
        store
            .write_skill(&skill_with("b", 1, "sig-b", SkillState::Confirmed))
            .unwrap();
        store
            .write_skill(&skill_with("c", 1, "sig-c", SkillState::Draft))
            .unwrap();

        let ctx = SkillContext {
            enabled: true,
            project_skills_dir: project_dir,
            global_skills_dir: None,
            project_id: "p".into(),
        };
        let idx = SkillIndex::build(&ctx, Arc::new(HashedShingleEmbedder::default())).unwrap();
        assert_eq!(idx.len(), 3);
        assert!(idx.get("a", 1).is_some());
        assert!(idx.get("b", 1).is_some());
        assert!(idx.get("c", 1).is_some());
    }

    #[test]
    fn lookup_excludes_draft_state() {
        let mut idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        idx.upsert(skill_with("draft", 1, "sig-a", SkillState::Draft));
        idx.upsert(skill_with("conf", 1, "sig-a", SkillState::Confirmed));

        let hits = idx.lookup(
            &SubgoalSignature("sig-a".into()),
            &ApplicabilitySignature("appsig".into()),
            5,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].skill.id, "conf");
    }

    #[test]
    fn lookup_respects_k() {
        let mut idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        for i in 0..5 {
            idx.upsert(skill_with(
                &format!("s{i}"),
                1,
                "sig-shared",
                SkillState::Confirmed,
            ));
        }
        let hits = idx.lookup(
            &SubgoalSignature("sig-shared".into()),
            &ApplicabilitySignature("appsig".into()),
            2,
        );
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn promoted_skills_are_retrievable() {
        let mut idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        idx.upsert(skill_with("p", 1, "sig", SkillState::Promoted));
        let hits = idx.lookup(
            &SubgoalSignature("sig".into()),
            &ApplicabilitySignature("appsig".into()),
            5,
        );
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn remove_drops_the_skill_and_its_signature_pointer() {
        let mut idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        idx.upsert(skill_with("a", 1, "sig", SkillState::Confirmed));
        idx.remove("a", 1);
        assert!(idx.get("a", 1).is_none());
        let hits = idx.lookup(
            &SubgoalSignature("sig".into()),
            &ApplicabilitySignature("appsig".into()),
            5,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn upsert_with_changed_signature_repoints_reverse_index() {
        let mut idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        idx.upsert(skill_with("a", 1, "sig-old", SkillState::Confirmed));
        // Same (id, version) re-insert under a new signature — the
        // reverse-index should drop the old entry, not double-list.
        idx.upsert(skill_with("a", 1, "sig-new", SkillState::Confirmed));

        assert!(
            idx.lookup(
                &SubgoalSignature("sig-old".into()),
                &ApplicabilitySignature("appsig".into()),
                5,
            )
            .is_empty()
        );
        assert_eq!(
            idx.lookup(
                &SubgoalSignature("sig-new".into()),
                &ApplicabilitySignature("appsig".into()),
                5,
            )
            .len(),
            1,
        );
    }

    #[test]
    fn skills_in_state_filters_correctly() {
        let mut idx = SkillIndex::empty(Arc::new(HashedShingleEmbedder::default()));
        idx.upsert(skill_with("d1", 1, "s", SkillState::Draft));
        idx.upsert(skill_with("c1", 1, "s", SkillState::Confirmed));
        idx.upsert(skill_with("p1", 1, "s", SkillState::Promoted));

        assert_eq!(idx.skills_in_state(SkillState::Draft).len(), 1);
        assert_eq!(idx.skills_in_state(SkillState::Confirmed).len(), 1);
        assert_eq!(idx.skills_in_state(SkillState::Promoted).len(), 1);
    }
}
