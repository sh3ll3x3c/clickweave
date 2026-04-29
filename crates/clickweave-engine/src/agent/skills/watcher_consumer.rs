//! Consumer task that drains [`SkillWatcher`] events and reflects them
//! into the shared [`SkillIndex`] + on-disk store.
//!
//! Locked decision D52 says external edits flip `edited_by_user = true`
//! on the affected skill. The watcher emits the events; this consumer
//! is what fires the flag. Self-writes (the store's own
//! atomic-rename) are filtered out via
//! [`SkillStore::was_recently_written`] so the store does not poke its
//! own `edited_by_user` whenever the runner re-emits a skill.
//!
//! Phase 2 owns the consumer as a free-standing task; Phase 3 will
//! spawn it inside the runner so it runs alongside the agent loop.

#![allow(dead_code)]

use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::warn;

use super::index::SkillIndex;
use super::store::SkillStore;
use super::watcher::SkillFileEvent;

pub struct WatcherConsumer {
    index: Arc<RwLock<SkillIndex>>,
    store: Arc<SkillStore>,
    rx: mpsc::Receiver<SkillFileEvent>,
}

impl WatcherConsumer {
    pub fn spawn(
        index: Arc<RwLock<SkillIndex>>,
        store: Arc<SkillStore>,
        rx: mpsc::Receiver<SkillFileEvent>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut consumer = WatcherConsumer { index, store, rx };
            consumer.run().await;
        })
    }

    async fn run(&mut self) {
        while let Some(event) = self.rx.recv().await {
            self.handle(event);
        }
    }

    fn handle(&self, event: SkillFileEvent) {
        match event {
            SkillFileEvent::Created(path) | SkillFileEvent::Modified(path) => {
                if self.store.was_recently_written(&path) {
                    return;
                }
                match self.store.read_skill(&path) {
                    Ok(mut skill) => {
                        if !skill.edited_by_user {
                            skill.edited_by_user = true;
                            if let Err(err) = self.store.write_skill(&skill) {
                                warn!(?path, ?err, "skill watcher: persist edited_by_user failed");
                            }
                        }
                        self.index.write().upsert(skill);
                    }
                    Err(err) => warn!(?path, ?err, "skill watcher: parse failed"),
                }
            }
            SkillFileEvent::Deleted(path) => {
                if let Ok(skill) = self.store.read_skill(&path) {
                    self.index.write().remove(&skill.id, skill.version);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::episodic::HashedShingleEmbedder;
    use crate::agent::skills::types::*;
    use chrono::Utc;
    use std::time::Duration;

    fn fixture(id: &str, version: u32, edited: bool) -> Skill {
        Skill {
            id: id.into(),
            version,
            state: SkillState::Confirmed,
            scope: SkillScope::ProjectLocal,
            name: id.into(),
            description: String::new(),
            tags: vec![],
            subgoal_text: "subgoal".into(),
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
            edited_by_user: edited,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            produced_node_ids: vec![],
            body: format!("# {id}\n"),
        }
    }

    #[tokio::test]
    async fn external_modify_flips_edited_by_user() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let store = Arc::new(SkillStore::new(dir.clone()));
        let path = store.write_skill(&fixture("a", 1, false)).unwrap();
        // The store's recently-written tolerance is 100ms — wait it
        // out so the synthesized event below counts as external.
        tokio::time::sleep(Duration::from_millis(150)).await;

        let index = Arc::new(RwLock::new(SkillIndex::empty(Arc::new(
            HashedShingleEmbedder::default(),
        ))));
        let (tx, rx) = mpsc::channel::<SkillFileEvent>(8);
        let handle = WatcherConsumer::spawn(index.clone(), store.clone(), rx);

        tx.send(SkillFileEvent::Modified(path.clone()))
            .await
            .unwrap();
        // Drop the sender so the consumer's `rx.recv` returns None and
        // the spawned task ends — keeps the test from hanging.
        drop(tx);
        handle.await.unwrap();

        let on_disk = store.read_skill(&path).unwrap();
        assert!(
            on_disk.edited_by_user,
            "external edit should flip edited_by_user"
        );
        let in_index = index.read().get("a", 1).expect("indexed");
        assert!(in_index.edited_by_user);
    }

    #[tokio::test]
    async fn self_write_does_not_flip_edited_by_user() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let store = Arc::new(SkillStore::new(dir.clone()));
        let path = store.write_skill(&fixture("b", 1, false)).unwrap();
        // Within the recently-written tolerance, the consumer should
        // skip the event entirely — no flip, no upsert side effects.

        let index = Arc::new(RwLock::new(SkillIndex::empty(Arc::new(
            HashedShingleEmbedder::default(),
        ))));
        let (tx, rx) = mpsc::channel::<SkillFileEvent>(8);
        let handle = WatcherConsumer::spawn(index.clone(), store.clone(), rx);

        tx.send(SkillFileEvent::Modified(path.clone()))
            .await
            .unwrap();
        drop(tx);
        handle.await.unwrap();

        let on_disk = store.read_skill(&path).unwrap();
        assert!(
            !on_disk.edited_by_user,
            "self-write should not flip edited_by_user"
        );
    }

    #[tokio::test]
    async fn delete_event_removes_from_index() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let store = Arc::new(SkillStore::new(dir.clone()));
        let path = store.write_skill(&fixture("c", 1, false)).unwrap();

        let index = Arc::new(RwLock::new(SkillIndex::empty(Arc::new(
            HashedShingleEmbedder::default(),
        ))));
        index.write().upsert(fixture("c", 1, false));
        assert!(index.read().get("c", 1).is_some());

        let (tx, rx) = mpsc::channel::<SkillFileEvent>(8);
        let handle = WatcherConsumer::spawn(index.clone(), store.clone(), rx);

        // Pre-condition: file still exists so the consumer can read
        // its (id, version) before we ask it to drop the index entry.
        tx.send(SkillFileEvent::Deleted(path.clone()))
            .await
            .unwrap();
        drop(tx);
        handle.await.unwrap();

        assert!(index.read().get("c", 1).is_none());
    }

    #[tokio::test]
    async fn modify_on_already_edited_skip_skip_redundant_write() {
        // If a skill already has `edited_by_user = true`, an external
        // modify should not re-write the file. We approximate "no
        // re-write" by checking the recently-written tracker hasn't
        // been bumped post-event.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let store = Arc::new(SkillStore::new(dir.clone()));
        let path = store.write_skill(&fixture("d", 1, true)).unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!store.was_recently_written(&path));

        let index = Arc::new(RwLock::new(SkillIndex::empty(Arc::new(
            HashedShingleEmbedder::default(),
        ))));
        let (tx, rx) = mpsc::channel::<SkillFileEvent>(8);
        let handle = WatcherConsumer::spawn(index.clone(), store.clone(), rx);

        tx.send(SkillFileEvent::Modified(path.clone()))
            .await
            .unwrap();
        drop(tx);
        handle.await.unwrap();

        assert!(
            !store.was_recently_written(&path),
            "consumer should not have written the file again"
        );
    }
}
