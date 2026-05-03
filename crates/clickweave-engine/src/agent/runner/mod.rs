//! State-spine agent runner.
//!
//! This module implements the single-pass ReAct loop over a harness-owned
//! `WorldModel` + `TaskState`. Each LLM turn produces an `AgentTurn`:
//! 0..N typed task-state mutations followed by exactly one action.
//!
//! Phase 2c: the runner type is built up incrementally across a series of
//! tasks, alongside its tests. Phase 3 landed the cutover, replacing the
//! legacy runner with this state-spine module.

#![allow(dead_code)] // Phase 2c: live wiring lands in Phase 3 cutover.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use clickweave_core::cdp::CdpFindElementMatch;
use clickweave_llm::{ChatBackend, DynChatBackend, Message};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::agent::context::{CompactBudget, compact};
use crate::agent::permissions::{
    PermissionAction, PermissionPolicy, ToolAnnotations, evaluate as evaluate_permission,
};
use crate::agent::phase::{self, PhaseSignals};
use crate::agent::prompt::{
    UserTurnMessageInput, build_system_prompt, build_system_prompt_with_header,
    build_user_turn_message_from_input,
};
use crate::agent::recovery::{RecoveryAction, recovery_strategy};
use crate::agent::skills::{
    RecordedStep, RetrievedSkill, SkillContext, SkillFrame, SkillIndex, SkillStore,
    SubgoalSignature,
};
use crate::agent::task_state::{Milestone, SubgoalId, TaskState, TaskStateMutation};
use crate::agent::types::{
    AgentCommand, AgentConfig, AgentEvent, AgentState, AgentStep, ApprovalRequest, RunnerOutput,
    StepOutcome, TerminalReason, WorldModelDiff,
};
use crate::agent::world_model::{
    CdpElementInventorySummary, InvalidationEvent, ObservedElement, WorldModel,
};
use crate::executor::Mcp;

mod cdp_lifecycle;
mod focus;
mod progress;
mod tool_classification;

pub(crate) use focus::FocusSkipReason;
#[cfg(test)]
pub(crate) use progress::{NO_ACTION_MUTATION_ONLY_PREFIX, STALE_CDP_UID_PREFIX};
pub(crate) use progress::{NO_PROGRESS_WARNING_PREFIX, UNVERIFIED_SIDE_EFFECT_PREFIX};
pub(crate) use tool_classification::{
    diff_world_model_signatures, extract_result_text, is_observation_tool,
};
#[cfg(test)]
pub(crate) use tool_classification::{is_ax_dispatch_tool, is_state_transition_tool};

use focus::{
    AX_DISPATCH_TOOLSET, CDP_DISPATCH_TOOLSET, RunningAppInfo, force_background_launch_app,
    is_coordinate_primitive, launch_app_has_launch_only_args, mcp_has_toolset,
};
use progress::{
    ACTION_CYCLE_WINDOW, ActionProgressSignature, LastActionProgress,
    NO_ACTION_MUTATION_ONLY_REASON, REPEAT_ACTION_THRESHOLD, TEXT_SUBMIT_SEARCH_THRESHOLD,
    TextSubmitSearchProgress, UNVERIFIED_SIDE_EFFECT_COMPLETION_BLOCKED_REASON,
    build_action_cycle_nudge, build_no_progress_nudge, build_post_text_submit_nudge,
    build_stale_cdp_uid_nudge, build_unverified_side_effect_nudge, cdp_find_elements_has_matches,
    combine_with_side_effect_nudge, detect_repeated_action_cycle,
    guard_completion_after_unverified_side_effect, is_send_submit_cdp_search,
    is_stale_cdp_uid_error, is_text_composition_tool, is_unverified_side_effect_action,
    reset_no_progress_tracking, stable_no_progress_context_signature,
};
use tool_classification::{
    APP_LIFECYCLE_TOOLS, CDP_NAVIGATION_TOOLS, FOCUS_CHANGING_TOOLS, OBSERVATION_TOOLS,
    brief_summarize_args, build_annotations_index,
};

#[derive(Debug, Default)]
pub(crate) struct CdpPageObservation {
    pub page_url: String,
    pub page_fingerprint: String,
    pub inventory: Vec<CdpElementInventorySummary>,
}

struct RunLoopContext {
    messages: Vec<Message>,
    tools: Vec<Value>,
    advertised_tool_names: Vec<String>,
    annotations_by_tool: HashMap<String, ToolAnnotations>,
    budget: CompactBudget,
}

#[derive(Default)]
struct RunLoopTrackers {
    previous_result: Option<String>,
    last_failure: Option<(String, Value, String)>,
    last_action: Option<LastActionProgress>,
    recent_actions: VecDeque<ActionProgressSignature>,
    pending_text_submit_search: Option<TextSubmitSearchProgress>,
}

enum LoopStepFlow {
    Continue,
    Break,
    Dispatch,
}

/// The one action an `AgentTurn` must carry (D10).
///
/// `ToolCall` usually dispatches to MCP; harness-local observation pseudo-tools
/// such as `get_current_datetime` are intercepted by `McpToolExecutor`.
/// `AgentDone` / `AgentReplan` are harness-local pseudo-tools that never reach
/// MCP.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentAction {
    ToolCall {
        tool_name: String,
        arguments: serde_json::Value,
        tool_call_id: String,
    },
    AgentDone {
        summary: String,
    },
    AgentReplan {
        reason: String,
    },
    /// Replay a procedural skill listed in the previous turn's
    /// `<applicable_skills>` block. The harness expands the skill's
    /// recorded action sketch through the same dispatch helper as live
    /// tool calls so the safety surface is identical.
    InvokeSkill {
        skill_id: String,
        version: u32,
        parameters: serde_json::Value,
    },
}

/// Batched single-pass agent output: task-state mutations followed by one
/// action. Mutations apply in order before the action dispatches.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentTurn {
    pub mutations: Vec<TaskStateMutation>,
    pub action: AgentAction,
}

/// State-spine runner. Owns the harness-side `WorldModel` + `TaskState` and
/// drives a single-pass ReAct loop: observe -> render -> decide -> apply ->
/// dispatch -> continuity -> invalidate.
///
/// Phase 2c: the struct carries a superset of fields — the minimum the new
/// control loop exercises plus compatibility fields needed by the public
/// `run_agent_workflow` seam. Fields the live tests don't touch are covered by
/// the module-wide `#![allow(dead_code)]`.
pub struct StateRunner {
    // --- Core state-spine fields ---
    pub world_model: WorldModel,
    pub task_state: TaskState,
    pub step_index: usize,
    pub consecutive_errors: usize,
    pub last_replan_step: Option<usize>,
    pub pending_events: Vec<InvalidationEvent>,

    // --- Compatibility fields ---
    // Carried so the public seam can change without silently dropping
    // what callers rely on today.
    pub config: AgentConfig,
    pub state: AgentState,
    pub workflow: clickweave_core::Workflow,
    pub last_node_id: Option<uuid::Uuid>,
    pub recent_destructive_tools: Vec<String>,

    // --- Collaborators (builder-style) ---
    pub storage: Option<std::sync::Arc<std::sync::Mutex<clickweave_core::storage::RunStorage>>>,
    pub run_id: uuid::Uuid,
    /// Live event channel. When `None` the runner runs silently.
    pub event_tx: Option<mpsc::Sender<RunnerOutput>>,
    /// Approval-gate channel pair. When `None` no prompt fires (the
    /// permission policy is consulted in isolation).
    pub approval_gate: Option<crate::agent::approval::ApprovalGate>,
    /// Optional VLM backend used to verify `agent_done`. Stored as
    /// `Arc<dyn DynChatBackend>` per D-PR1 so primary and VLM backends
    /// can be different concrete types without polluting `StateRunner`'s
    /// generics.
    pub vision: Option<Arc<dyn DynChatBackend>>,
    /// Permission policy consulted before every non-observation tool
    /// call. Default policy denies nothing and asks for nothing —
    /// matches the legacy behaviour.
    pub permissions: PermissionPolicy,
    /// Directory for completion-verification artifacts (PNG + JSON).
    /// `None` disables artifact persistence.
    pub verification_artifacts_dir: Option<PathBuf>,
    /// Monotonic counter feeding the `completion_verification_<n>.{png,json}`
    /// filename ordinal so repeated `verify_completion` calls within the
    /// same run do not overwrite each other. Mirrors the legacy
    /// `AgentRunner::verification_count` field.
    pub verification_count: u32,

    // --- CDP lifecycle bookkeeping (Task 3a.6) ---
    /// Shared CDP connection state — identical to the legacy field on the
    /// old `AgentRunner`. Populated when [`Self::auto_connect_cdp`]
    /// succeeds and consulted by [`Self::should_skip_focus_window`] and
    /// `verify_completion` so the completion-verification screenshot
    /// targets the right window.
    pub(crate) cdp_state: crate::cdp_lifecycle::CdpState,
    /// Per-app `kind` hint learned from a structured MCP response
    /// (`focus_window` / `launch_app` with `{"kind": "..."}`). Populated
    /// before the CDP decision runs so subsequent `focus_window` calls
    /// can be suppressed when AX / CDP dispatch is available. Mirrors
    /// the legacy `AgentRunner::known_app_kinds` field.
    pub(crate) known_app_kinds: HashMap<String, String>,

    /// World-model field signatures captured by the top-level `run` loop
    /// before it mirrors the observe-phase CDP results into
    /// `world_model.elements` / `world_model.cdp_page`. When `Some`,
    /// `run_turn` uses this as the baseline for its `WorldModelChanged`
    /// diff so direct-observation writes (which happen outside
    /// `run_turn`) still surface in `changed_fields`. When `None`, the
    /// test/unit caller path is in effect and `run_turn` falls back to
    /// snapshotting signatures itself immediately before `observe()`.
    pub(crate) turn_pre_signatures: Option<Vec<(&'static str, Option<usize>)>>,

    // --- Spec 2 episodic-memory fields (Phase 3) ---
    pub(crate) episodic_ctx: crate::agent::episodic::EpisodicContext,
    pub(crate) episodic_store: Option<std::sync::Arc<crate::agent::episodic::SqliteEpisodicStore>>,
    pub(crate) episodic_global: Option<std::sync::Arc<crate::agent::episodic::SqliteEpisodicStore>>,
    pub(crate) episodic_writer: Option<crate::agent::episodic::EpisodicWriter>,
    pub(crate) recovering_snapshot: Option<crate::agent::episodic::types::RecoveringEntrySnapshot>,
    pub(crate) recovery_actions_accumulator: Vec<crate::agent::episodic::types::CompactAction>,
    pub(crate) last_failed_tool_name: Option<String>,
    pub(crate) last_failed_error_kind: Option<String>,
    /// Cached events.jsonl path for the active execution; resolved
    /// lazily when retrieval needs to populate
    /// `RecoveringEntrySnapshot::events_jsonl_ref`.
    pub(crate) episodic_events_ref: Option<String>,
    /// Authoritative gate for D24 run-start retrieval: set true the
    /// first time `try_retrieve_episodic` reaches its trigger-decision
    /// slot, regardless of whether retrieval returned hits. Decoupled
    /// from `step_index` so synthetic-skip / policy-deny / approval-reject
    /// paths cannot let `step_index == 0` re-fire
    /// run-start retrieval after the run has already taken actions.
    pub(crate) episodic_run_start_retrieved: bool,

    // --- Spec 3 procedural-skills fields (Phase 3) ---
    /// Boundary metadata threaded in from the Tauri layer (project +
    /// global skills directories, project id, master enable flag). Phase
    /// 3 reads these to construct the `SkillIndex` and gate
    /// extraction / retrieval. A `disabled` context turns every skill
    /// hook into a no-op.
    pub(crate) skill_ctx: SkillContext,
    /// Per-run skill index, shared with the file-watcher consumer.
    /// Built once at runner construction and rebuilt across runs only
    /// (never mid-run — file events flip individual entries via the
    /// watcher consumer).
    pub(crate) skill_index: Arc<parking_lot::RwLock<SkillIndex>>,
    /// On-disk store backing `skill_index`. Carried as an `Arc` so the
    /// extractor (Phase 3) and watcher consumer (Phase 2) can share the
    /// recently-written-tolerance table without duplicating writes.
    pub(crate) skill_store: Arc<SkillStore>,
    /// Optional in-memory accumulator of every successful tool call this
    /// run, keyed by step. Drained by `maybe_extract_skill` at every
    /// `CompleteSubgoal` boundary against the `[push_idx..]` window.
    /// Cleared at run-terminal so the runner can in theory be reused.
    pub(crate) recorded_steps: Vec<RecordedStep>,
    /// Snapshot of `world_model` taken just after `observe()` at the
    /// top of the current loop iteration. Used as the `world_model_pre`
    /// when a successful tool dispatch produces a `RecordedStep`.
    pub(crate) pre_dispatch_snapshot: Option<crate::agent::step_record::WorldModelSnapshot>,
    /// Stack of `recorded_steps.len()` indices captured at every
    /// `PushSubgoal` mutation. Each `CompleteSubgoal` pops the top so
    /// the extractor can address the action sketch by step range
    /// (`recorded_steps[push_idx..]`). Mirrors `task_state.subgoal_stack`
    /// in depth.
    pub(crate) push_idx_stack: Vec<usize>,
    /// Stack of subgoal signatures captured at `PushSubgoal` time. The
    /// extractor must key the skill by the state that made the subgoal
    /// applicable, not by the later post-completion world model.
    pub(crate) push_signature_stack: Vec<SubgoalSignature>,
    /// `SubgoalId`s generated by the most recent batch of mutations.
    /// Populated inside `apply_mutations` and consumed by the retrieval
    /// hook in the outer loop. Cleared on every fresh batch — it must
    /// not span turns.
    pub(crate) last_pushed_subgoal_ids: Vec<SubgoalId>,
    /// `(push_idx, milestone)` queue drained by
    /// `write_subgoal_completed_records` so each completed-subgoal
    /// extraction has both the action-sketch start index and the
    /// milestone payload available without re-walking
    /// `task_state.milestones`.
    pub(crate) completed_subgoal_extraction_queue:
        Vec<(usize, Milestone, SubgoalSignature, Vec<uuid::Uuid>)>,
    /// Workflow node ids emitted via `AgentEvent::NodeAdded`, tracked
    /// per active subgoal frame. A produced node belongs to every open
    /// frame so nested subgoals keep their local lineage while parent
    /// subgoals still include all nodes produced during their lifetime.
    pub(crate) produced_node_ids_stack: Vec<Vec<uuid::Uuid>>,
    /// Top-k applicable skills surfaced for the next user turn.
    /// Populated by the retrieval hook on `push_subgoal`, consumed +
    /// cleared by `build_user_turn_message`'s caller at the next
    /// iteration.
    pub(crate) pending_applicable_skills: Vec<RetrievedSkill>,
    /// Optional eval-only override for the stable system-prompt header.
    /// Production callers leave this as `None` and use the file-backed
    /// default in `prompts/agent_system.md`.
    pub(crate) agent_system_prompt_override: Option<String>,
    /// Frame held while the runner is waiting on an LLM fallback turn
    /// during a skill replay. Phase 3 always leaves this `None`; Phase
    /// 4 lands the real consumer.
    pub(crate) suspended_skill_frame: Option<SkillFrame>,
    /// Join handle for the file-watcher consumer task spawned at run
    /// start. Aborted at run-terminal so the consumer doesn't outlive
    /// the runner. `None` when skills are disabled or the watcher
    /// failed to spawn.
    pub(crate) skill_watcher_handle: Option<tokio::task::JoinHandle<()>>,
}

impl StateRunner {
    pub fn new(goal: String, config: AgentConfig) -> Self {
        Self::new_with_episodic(
            goal,
            config,
            crate::agent::episodic::EpisodicContext::disabled(),
        )
    }

    /// Construct a runner with an explicit Spec 2 [`EpisodicContext`].
    ///
    /// Production callers go through this constructor; the legacy
    /// [`Self::new`] is preserved for the many integration tests that
    /// don't care about episodic memory and pass the disabled context
    /// implicitly.
    ///
    /// SQLite stores are opened here (they don't need the event channel
    /// or run_id), but the [`EpisodicWriter`] is deferred to
    /// [`Self::with_episodic_writer`] so it can capture the channel +
    /// run_id seeded by [`Self::with_events`] / [`Self::with_run_id`]
    /// — without those the writer's emitted events would fail the
    /// frontend's stale-run filter.
    pub fn new_with_episodic(
        goal: String,
        config: AgentConfig,
        episodic_ctx: crate::agent::episodic::EpisodicContext,
    ) -> Self {
        Self::new_with_episodic_and_skills(goal, config, episodic_ctx, SkillContext::disabled())
    }

    /// Construct a runner with both an explicit Spec 2 [`EpisodicContext`]
    /// and an explicit Spec 3 [`SkillContext`].
    ///
    /// Production callers go through this constructor once Phase 3 lands
    /// the Tauri-layer wiring; the legacy [`Self::new`] and
    /// [`Self::new_with_episodic`] are preserved for the many integration
    /// tests that don't exercise skills (they pass the disabled context
    /// implicitly).
    ///
    /// When `skill_ctx.enabled == true`, the constructor builds the
    /// `SkillIndex` from the on-disk store. When disabled (or when the
    /// build fails), the runner stores an empty index — extraction +
    /// retrieval become no-ops and the runner still runs end-to-end.
    pub fn new_with_episodic_and_skills(
        goal: String,
        config: AgentConfig,
        episodic_ctx: crate::agent::episodic::EpisodicContext,
        skill_ctx: SkillContext,
    ) -> Self {
        let workflow = clickweave_core::Workflow::default();
        let state = AgentState::new(workflow.clone());

        let (episodic_store, episodic_global) = if episodic_ctx.enabled && config.episodic_enabled {
            use crate::agent::episodic::SqliteEpisodicStore;
            let weights = config.episodic_score_weights.into();
            let halflife = config.episodic_decay_halflife_days;
            let wl = SqliteEpisodicStore::new_with_config(
                    &episodic_ctx.workflow_local_path,
                    crate::agent::episodic::EpisodeScope::WorkflowLocal,
                    weights,
                    halflife,
                    config.episodic_max_per_scope_workflow,
                )
                .map(std::sync::Arc::new)
                .map_err(|e| {
                    tracing::warn!(error = %e, "episodic: failed to open workflow-local store; disabling");
                    e
                })
                .ok();
            let global = episodic_ctx
                .global_path
                .as_ref()
                .and_then(|p| {
                    SqliteEpisodicStore::new_with_config(
                        p,
                        crate::agent::episodic::EpisodeScope::Global,
                        weights,
                        halflife,
                        config.episodic_max_per_scope_global,
                    )
                    .ok()
                })
                .map(std::sync::Arc::new);
            (wl, global)
        } else {
            (None, None)
        };

        // Spec 3: build the skill index when enabled. Failure to build
        // (e.g. unreadable directory entry) drops to an empty index so
        // the runner still runs — skills are best-effort by design.
        let embedder =
            std::sync::Arc::new(crate::agent::episodic::HashedShingleEmbedder::default());
        let skill_index = if skill_ctx.enabled {
            match SkillIndex::build(&skill_ctx, embedder.clone()) {
                Ok(idx) => idx,
                Err(err) => {
                    tracing::warn!(?err, "skills: index build failed; running with empty index");
                    SkillIndex::empty(embedder.clone())
                }
            }
        } else {
            SkillIndex::empty(embedder.clone())
        };
        let skill_store =
            std::sync::Arc::new(SkillStore::new(skill_ctx.project_skills_dir.clone()));
        let skill_index = std::sync::Arc::new(parking_lot::RwLock::new(skill_index));

        Self {
            world_model: WorldModel::default(),
            task_state: TaskState::new(goal),
            step_index: 0,
            consecutive_errors: 0,
            last_replan_step: None,
            pending_events: Vec::new(),
            config,
            state,
            workflow,
            last_node_id: None,
            recent_destructive_tools: Vec::new(),
            storage: None,
            run_id: uuid::Uuid::new_v4(),
            event_tx: None,
            approval_gate: None,
            vision: None,
            permissions: PermissionPolicy::default(),
            verification_artifacts_dir: None,
            verification_count: 0,
            cdp_state: crate::cdp_lifecycle::CdpState::new(),
            known_app_kinds: HashMap::new(),
            turn_pre_signatures: None,
            episodic_ctx,
            episodic_store,
            episodic_global,
            episodic_writer: None,
            recovering_snapshot: None,
            recovery_actions_accumulator: Vec::new(),
            last_failed_tool_name: None,
            last_failed_error_kind: None,
            episodic_events_ref: None,
            episodic_run_start_retrieved: false,
            skill_ctx,
            skill_index,
            skill_store,
            recorded_steps: Vec::new(),
            pre_dispatch_snapshot: None,
            push_idx_stack: Vec::new(),
            push_signature_stack: Vec::new(),
            last_pushed_subgoal_ids: Vec::new(),
            completed_subgoal_extraction_queue: Vec::new(),
            produced_node_ids_stack: Vec::new(),
            pending_applicable_skills: Vec::new(),
            agent_system_prompt_override: None,
            suspended_skill_frame: None,
            skill_watcher_handle: None,
        }
    }

    pub fn with_run_id(mut self, run_id: uuid::Uuid) -> Self {
        self.run_id = run_id;
        self
    }

    /// Override the stable system-prompt header. Intended for the eval
    /// harness and prompt-optimization experiments only; production runs
    /// use the checked-in default prompt file.
    pub fn with_agent_system_prompt_override(mut self, prompt: impl Into<String>) -> Self {
        self.agent_system_prompt_override = Some(prompt.into());
        self
    }

    /// Attach a shared `RunStorage` so boundary `StepRecord`s land in the
    /// execution-level `events.jsonl`. Storage is optional: the runner
    /// still runs end-to-end without a handle — snapshots just become
    /// no-ops.
    pub fn with_storage(
        mut self,
        storage: std::sync::Arc<std::sync::Mutex<clickweave_core::storage::RunStorage>>,
    ) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Attach a live event channel. Events emitted by the runner are
    /// forwarded through this sender; `None` runs silently.
    pub fn with_events(mut self, tx: mpsc::Sender<RunnerOutput>) -> Self {
        self.event_tx = Some(tx);
        self
    }

    /// Attach an approval-gate channel. The runner sends
    /// `(ApprovalRequest, oneshot::Sender<bool>)` pairs through it and
    /// waits for a reply before dispatching every approval-gated tool.
    pub fn with_approval(
        mut self,
        request_tx: mpsc::Sender<(ApprovalRequest, oneshot::Sender<bool>)>,
    ) -> Self {
        self.approval_gate = Some(crate::agent::approval::ApprovalGate { request_tx });
        self
    }

    /// Attach a VLM backend used to verify `agent_done` against a fresh
    /// screenshot (D-PR1: stored as `Arc<dyn DynChatBackend>` so the
    /// VLM can be a different concrete backend from the primary).
    pub fn with_vision(mut self, vlm: Arc<dyn DynChatBackend>) -> Self {
        self.vision = Some(vlm);
        self
    }

    /// Replace the default permission policy.
    pub fn with_permissions(mut self, policy: PermissionPolicy) -> Self {
        self.permissions = policy;
        self
    }

    /// Set the directory where completion-verification artifacts are
    /// persisted (PNG screenshot + JSON metadata).
    pub fn with_verification_artifacts_dir(mut self, dir: PathBuf) -> Self {
        self.verification_artifacts_dir = Some(dir);
        self
    }

    /// Spawn the [`EpisodicWriter`] tied to this runner.
    ///
    /// MUST be called after [`Self::with_events`] and [`Self::with_run_id`]:
    /// the writer captures both at spawn so emitted `EpisodeWritten` /
    /// `EpisodePromoted` events carry the live `run_id` and pass the
    /// frontend's stale-run filter. Calling before either silently
    /// skips the writer (so episodic stays best-effort and the agent
    /// run still proceeds — D32).
    pub fn with_episodic_writer(mut self) -> Self {
        if !self.episodic_active() {
            return self;
        }
        let event_tx = self.event_tx.clone();
        // Pass the configured store knobs through to the writer so
        // its workflow-local + global stores honour the same
        // weights / half-life / per-scope caps the runner-side
        // retrieval stores were opened with. The default `spawn`
        // path opens both stores via `SqliteEpisodicStore::new`,
        // which hard-codes the cap to 500.
        let store_config = crate::agent::episodic::store::EpisodicStoreConfig {
            score_weights: self.config.episodic_score_weights.into(),
            decay_halflife_days: self.config.episodic_decay_halflife_days,
            max_per_scope_workflow: self.config.episodic_max_per_scope_workflow,
            max_per_scope_global: self.config.episodic_max_per_scope_global,
        };
        match crate::agent::episodic::EpisodicWriter::spawn_with_config(
            self.episodic_ctx.clone(),
            store_config,
            event_tx,
            self.run_id,
        ) {
            Ok(w) => self.episodic_writer = Some(w),
            Err(e) => tracing::warn!(error = %e, "episodic: failed to spawn writer"),
        }
        self
    }

    /// Whether the episodic memory layer is wired up and active for
    /// this runner. Cheap, side-effect-free; safe to call from hot
    /// paths.
    pub(crate) fn episodic_active(&self) -> bool {
        self.config.episodic_enabled && self.episodic_ctx.enabled && self.episodic_store.is_some()
    }

    /// Resolve the active execution's `events.jsonl` path through
    /// `RunStorage`, caching the result so repeated calls don't take
    /// the storage mutex repeatedly.
    pub(crate) fn current_events_jsonl_ref(&mut self) -> Option<String> {
        if let Some(cached) = &self.episodic_events_ref {
            return Some(cached.clone());
        }
        let storage = self.storage.as_ref()?;
        let guard = storage.lock().ok()?;
        let exec_dir = guard.execution_dir_name()?;
        let path = guard.base_path().join(exec_dir).join("events.jsonl");
        let s = path.to_string_lossy().into_owned();
        self.episodic_events_ref = Some(s.clone());
        Some(s)
    }

    /// Clone the episodic writer's channel sender, if a writer is active.
    ///
    /// The returned sender shares the same worker task as the writer owned
    /// by this runner — no second SQLite connection is opened. Callers can
    /// enqueue `WriteRequest`s (including `PromotePass`) on it even after
    /// `run` has consumed and dropped the runner, as long as they hold the
    /// sender clone. Dropping the clone releases the channel once the
    /// runner's own copy is also gone, allowing the worker to exit.
    ///
    /// Returns `None` when episodic is disabled or the writer was not yet
    /// spawned.
    pub(crate) fn writer_sender(
        &self,
    ) -> Option<tokio::sync::mpsc::Sender<crate::agent::episodic::types::WriteRequest>> {
        self.episodic_writer.as_ref().map(|w| w.sender())
    }

    /// Write a boundary `StepRecord` through the shared `RunStorage` handle.
    /// Silently no-ops when no storage is attached or the lock is poisoned
    /// — persistence is best-effort, never fatal.
    pub fn write_step_record(&self, record: &crate::agent::step_record::StepRecord) {
        if let Some(s) = &self.storage
            && let Ok(guard) = s.lock()
        {
            let _ = guard.append_agent_event(record);
        }
    }

    #[cfg(test)]
    pub fn new_for_test(goal: String) -> Self {
        // Test fixtures historically assumed the legacy `allow_focus_window =
        // true` default — flipping the production default to `false` would
        // otherwise force every focus-window unit test to opt back in. The
        // production-default behavior is covered explicitly by
        // `default_config_disables_focus_window_via_policy` below.
        let config = AgentConfig {
            allow_focus_window: true,
            ..AgentConfig::default()
        };
        Self::new(goal, config)
    }

    /// Test-only constructor that wires an enabled `SkillContext` at a
    /// caller-provided directory. Used by Phase 3 e2e tests that need
    /// to drive the extractor + retrieval loop without spinning up a
    /// full Tauri + Mcp + LLM stack.
    #[cfg(test)]
    pub(crate) fn new_for_test_with_skills(goal: String, skills_dir: std::path::PathBuf) -> Self {
        let config = AgentConfig {
            allow_focus_window: true,
            ..AgentConfig::default()
        };
        let skill_ctx = SkillContext {
            enabled: true,
            project_skills_dir: skills_dir,
            global_skills_dir: None,
            project_id: "test".into(),
        };
        Self::new_with_episodic_and_skills(
            goal,
            config,
            crate::agent::episodic::EpisodicContext::disabled(),
            skill_ctx,
        )
    }

    /// Borrow the current CDP bookkeeping. Used by `verify_completion`
    /// to target the screenshot scope at the connected window, and
    /// (in tests) to assert CDP auto-connect side effects.
    pub(crate) fn cdp_state(&self) -> &crate::cdp_lifecycle::CdpState {
        &self.cdp_state
    }

    /// Seed `known_app_kinds` directly. Test-only — the live flow
    /// populates this via [`Self::record_app_kind`] from the MCP
    /// response shape.
    #[cfg(test)]
    pub(crate) fn record_app_kind_for_test(&mut self, app_name: &str, kind: &str) {
        self.known_app_kinds
            .insert(app_name.to_string(), kind.to_string());
    }

    /// Seed the active CDP connection identity. Test-only — the live
    /// flow populates this through [`Self::auto_connect_cdp`].
    #[cfg(test)]
    pub(crate) fn set_cdp_connected_for_test(&mut self, app_name: &str, pid: i32) {
        self.cdp_state.set_connected(app_name, pid);
    }

    /// Test-only seed for the `(app_kind, cdp_connected)` state the
    /// runner would otherwise reach only after `launch_app` →
    /// `auto_connect_cdp` → `on_cdp_connected`. Used by integration
    /// tests that want to exercise the post-CDP-connect focus_window
    /// skip path without the full quit/relaunch/connect choreography.
    /// Port of the legacy `AgentRunner::seed_cdp_live_for_test` for
    /// 3a.7.b test migration.
    #[cfg(test)]
    pub(crate) fn seed_cdp_live_for_test(&mut self, app_name: &str, kind: &str) {
        self.record_app_kind(app_name, kind);
        self.cdp_state.set_connected(app_name, 0);
    }

    /// Public-for-tests view of `cdp_state`. Keeps the field private
    /// outside the module while letting integration tests assert the
    /// post-tool auto-connect bookkeeping.
    #[cfg(test)]
    pub(crate) fn cdp_state_for_test(&self) -> &crate::cdp_lifecycle::CdpState {
        &self.cdp_state
    }

    /// Test-only entry point into the selected-page snapshot helper so
    /// the agent-vs-executor parity suite can exercise exactly the code
    /// path the live run would hit, rather than poking fields. Ported
    /// from the legacy `AgentRunner::snapshot_selected_page_url_for_test`
    /// for 3a.7.a test migration.
    #[cfg(test)]
    pub(crate) async fn snapshot_selected_page_url_for_test(
        &mut self,
        app_name: &str,
        pid: i32,
        mcp: &(impl crate::executor::Mcp + ?Sized),
    ) {
        crate::cdp_lifecycle::snapshot_selected_page_url(mcp, &mut self.cdp_state, app_name, pid)
            .await;
    }

    pub fn queue_invalidation(&mut self, e: InvalidationEvent) {
        self.pending_events.push(e);
    }

    /// Spec 2: run an episodic-memory retrieval if the trigger conditions
    /// hold (run-start or `Recovering` entry). On `Recovering` entry,
    /// also captures the [`RecoveringEntrySnapshot`] for the eventual
    /// write at the matching `Recovering -> Executing` exit.
    ///
    /// `prev_phase_at_top` is the phase as it was at the top of the
    /// outer-loop iteration before `observe()` ran, so the
    /// `Exploring/Executing -> Recovering` transition is detectable.
    pub(crate) async fn try_retrieve_episodic(
        &mut self,
        prev_phase_at_top: crate::agent::phase::Phase,
    ) -> Vec<crate::agent::episodic::RetrievedEpisode> {
        use crate::agent::episodic::signature::compute_pre_state_signature;
        use crate::agent::episodic::{
            EpisodicStore as _, RetrievalQuery, RetrievalTrigger, RetrievedEpisode,
        };
        use crate::agent::phase::Phase;

        if !self.episodic_active() {
            return Vec::new();
        }
        let store = match &self.episodic_store {
            Some(s) => s.clone(),
            None => return Vec::new(),
        };

        // D24: run-start retrieval fires once per run, full stop.
        // `episodic_run_start_retrieved` is the authoritative gate (not
        // `step_index == 0`, which lied on synthetic-skip / policy-deny /
        // approval-reject paths because none of those
        // ticked the counter). Marked consumed on first reach so a
        // zero-hit retrieval still counts as "the run-start slot was
        // used" and can never fire a second time.
        let trigger = if !self.episodic_run_start_retrieved {
            self.episodic_run_start_retrieved = true;
            RetrievalTrigger::RunStart
        } else if prev_phase_at_top != Phase::Recovering
            && self.task_state.phase == Phase::Recovering
        {
            RetrievalTrigger::RecoveringEntry
        } else {
            return Vec::new();
        };

        let active_slots: Vec<crate::agent::task_state::WatchSlotName> =
            self.task_state.watch_slots.iter().map(|s| s.name).collect();
        let sig = compute_pre_state_signature(&self.world_model, &active_slots);

        // Capture snapshot at retrieval time so the eventual
        // write uses the same signature.
        if matches!(trigger, RetrievalTrigger::RecoveringEntry) {
            use crate::agent::episodic::types::{RecoveringEntrySnapshot, TriggeringError};
            use crate::agent::step_record::WorldModelSnapshot;
            let events_ref = self.current_events_jsonl_ref();
            let snap = WorldModelSnapshot::from_world_model(&self.world_model);
            self.recovering_snapshot = Some(RecoveringEntrySnapshot {
                entered_at_step: self.step_index,
                world_model_at_entry: snap,
                task_state_at_entry: self.task_state.clone(),
                triggering_error: TriggeringError {
                    failed_tool: self.last_failed_tool_name.clone().unwrap_or_default(),
                    error_kind: self.last_failed_error_kind.clone().unwrap_or_default(),
                    consecutive_errors_at_entry: self.consecutive_errors as u32,
                    step_index: self.step_index,
                },
                workflow_hash: self.episodic_ctx.workflow_hash.clone(),
                pre_state_signature: sig.clone(),
                active_watch_slots: active_slots.clone(),
                events_jsonl_ref: events_ref,
            });
            self.recovery_actions_accumulator.clear();
        }

        let subgoal_owned = self.task_state.subgoal_stack.last().map(|s| s.text.clone());
        let goal_owned = self.task_state.goal.clone();
        let workflow_hash = self.episodic_ctx.workflow_hash.clone();
        let now = chrono::Utc::now();

        let q = RetrievalQuery {
            trigger,
            pre_state_signature: &sig,
            goal: &goal_owned,
            subgoal_text: subgoal_owned.as_deref(),
            workflow_hash: &workflow_hash,
            now,
        };

        let k_each = self.config.retrieved_episodes_k.max(1) * 2;
        let mut wl_hits: Vec<RetrievedEpisode> =
            store.retrieve(&q, k_each).await.unwrap_or_default();

        let g_cap = self.config.episodic_global_cap_per_retrieval.max(1) * 2;
        let mut g_hits: Vec<RetrievedEpisode> = match &self.episodic_global {
            Some(g) => g.retrieve(&q, g_cap).await.unwrap_or_default(),
            None => Vec::new(),
        };

        for h in &mut wl_hits {
            h.score_breakdown.final_score *= self.config.episodic_workflow_priority_multiplier;
        }
        g_hits.truncate(self.config.episodic_global_cap_per_retrieval);

        let mut merged: Vec<RetrievedEpisode> = wl_hits.into_iter().chain(g_hits).collect();
        merged.sort_by(|a, b| {
            crate::agent::episodic::embedder::nan_safe_desc(
                a.score_breakdown.final_score,
                b.score_breakdown.final_score,
            )
        });
        merged.truncate(self.config.retrieved_episodes_k);

        // Emit `EpisodesRetrieved` whenever the retrieval pass returned
        // at least one candidate. Frontends use this to surface the
        // `<retrieved_recoveries>` block before the LLM call lands.
        if !merged.is_empty() {
            use crate::agent::episodic::EpisodeScope;
            let workflow_count = merged
                .iter()
                .filter(|r| matches!(r.scope, EpisodeScope::WorkflowLocal))
                .count();
            let global_count = merged.len() - workflow_count;
            let event = AgentEvent::EpisodesRetrieved {
                run_id: self.run_id,
                trigger,
                count: merged.len(),
                episode_ids: merged
                    .iter()
                    .map(|r| r.episode.episode_id.clone())
                    .collect(),
                scope_breakdown: crate::agent::types::ScopeBreakdown {
                    workflow: workflow_count,
                    global: global_count,
                },
            };
            self.emit_event(event).await;
        }

        merged
    }

    /// Apply any pending invalidation events and re-infer the phase from
    /// structural signals.
    pub fn observe(&mut self) {
        let events = std::mem::take(&mut self.pending_events);
        self.world_model.apply_events(events);
        self.task_state.phase = phase::infer(&PhaseSignals {
            stack_depth: self.task_state.subgoal_stack.len(),
            consecutive_errors: self.consecutive_errors,
            last_replan_step: self.last_replan_step,
            current_step: self.step_index,
        });
    }

    /// Apply the batch of task-state mutations from an `AgentTurn`, in
    /// order. Invalid mutations become warnings but do not abort the pass —
    /// subsequent mutations and the action still run. Matches the
    /// error-path table in the spec.
    ///
    /// PushSubgoal / CompleteSubgoal route through the per-mutation
    /// helpers on `TaskState` so the runner can capture the generated
    /// `SubgoalId` (Spec 3 retrieval hook) and the matching push-side
    /// `recorded_steps` index (Spec 3 extractor) without re-walking
    /// the mutation slice. `last_pushed_subgoal_ids` is cleared at the
    /// top of every batch — the retrieval hook reads it once per turn.
    pub fn apply_mutations(&mut self, muts: &[TaskStateMutation]) -> Vec<String> {
        let mut warnings = Vec::new();
        self.last_pushed_subgoal_ids.clear();

        for m in muts {
            match m {
                TaskStateMutation::PushSubgoal { text } => {
                    self.push_idx_stack.push(self.recorded_steps.len());
                    self.push_signature_stack.push(
                        crate::agent::skills::signature::compute_subgoal_signature(
                            text,
                            &self.world_model,
                        ),
                    );
                    let id = self.task_state.apply_push_subgoal(text, self.step_index);
                    self.last_pushed_subgoal_ids.push(id);
                    self.produced_node_ids_stack.push(Vec::new());
                }
                TaskStateMutation::CompleteSubgoal { summary } => {
                    let push_idx = self.push_idx_stack.pop().unwrap_or(0);
                    let push_sig = self.push_signature_stack.pop();
                    let produced_node_ids = self.produced_node_ids_stack.pop().unwrap_or_default();
                    match self
                        .task_state
                        .apply_complete_subgoal(summary, self.step_index)
                    {
                        Ok(milestone) => {
                            let pre_state_sig = push_sig.unwrap_or_else(|| {
                                crate::agent::skills::signature::compute_subgoal_signature(
                                    &milestone.text,
                                    &self.world_model,
                                )
                            });
                            self.completed_subgoal_extraction_queue.push((
                                push_idx,
                                milestone,
                                pre_state_sig,
                                produced_node_ids,
                            ));
                        }
                        Err(e) => warnings.push(format!("{}", e)),
                    }
                }
                other => {
                    if let Err(e) = self.task_state.apply(other, self.step_index) {
                        warnings.push(format!("{}", e));
                    }
                }
            }
        }
        warnings
    }

    fn record_produced_node_id(&mut self, node_id: uuid::Uuid) {
        for produced_node_ids in &mut self.produced_node_ids_stack {
            produced_node_ids.push(node_id);
        }
    }

    /// Rewrite raw AX uid references in a workflow node into replay-stable
    /// `AxTarget::Descriptor` payloads using the current
    /// `last_native_ax_snapshot` body. Port of the legacy
    /// `enrich_ax_descriptor` helper — D15 moves the source of truth off
    /// the transcript onto `WorldModel`.
    ///
    /// No-op when no native AX snapshot has been captured yet, when the
    /// node type is not an AX dispatch variant, when the target is already
    /// a `Descriptor`, or when the uid is not present in the snapshot.
    pub fn enrich_ax_descriptor(&self, node_type: &mut clickweave_core::NodeType) {
        use clickweave_core::{AxTarget, NodeType};

        let Some(ax) = &self.world_model.last_native_ax_snapshot else {
            return;
        };

        let target: &mut AxTarget = match node_type {
            NodeType::AxClick(p) => &mut p.target,
            NodeType::AxSetValue(p) => &mut p.target,
            NodeType::AxSelect(p) => &mut p.target,
            _ => return,
        };

        let uid = match target {
            AxTarget::ResolvedUid(uid) if !uid.is_empty() => uid.clone(),
            _ => return,
        };

        let parsed = crate::agent::world_model::parse_ax_snapshot(&ax.value.ax_tree_text);
        let Some(entry) = parsed.into_iter().find(|e| e.uid == uid) else {
            return;
        };
        *target = AxTarget::Descriptor {
            role: entry.role,
            name: entry.name.unwrap_or_default(),
            parent_name: entry.parent_name,
        };
    }

    /// Build a workflow node for the executed tool call. Returns the UUID of
    /// the new node, or `None` when the tool is observation-only, when
    /// workflow-graph building is disabled via `config.build_workflow`, or
    /// when the tool-to-[`clickweave_core::NodeType`] mapping fails.
    ///
    /// On success the node is pushed onto `state.workflow.nodes`, an
    /// `AgentEvent::NodeAdded` fires, and — when a prior node exists —
    /// an edge from the previous node to this one is pushed onto
    /// `state.workflow.edges` with a matching `AgentEvent::EdgeAdded`. The
    /// first node in a run is chained from `state.last_node_id`, which the
    /// top-level loop seeds from the caller-provided `anchor_node_id` so the
    /// first tool call is linked to the prior workflow graph when one is
    /// supplied. Every node is stamped with `source_run_id: self.run_id`.
    ///
    /// Port of the legacy `AgentRunner::add_workflow_node`.
    pub async fn add_workflow_node(
        &mut self,
        tool_name: &str,
        arguments: &Value,
        known_tools: &[Value],
        annotations_by_tool: &HashMap<String, ToolAnnotations>,
    ) -> Option<uuid::Uuid> {
        use clickweave_core::{Node, Position, tool_mapping::tool_invocation_to_node_type};

        if !self.config.build_workflow {
            return None;
        }
        if is_observation_tool(tool_name, annotations_by_tool) {
            return None;
        }

        let mut node_type = match tool_invocation_to_node_type(tool_name, arguments, known_tools) {
            Ok(nt) => nt,
            Err(e) => {
                warn!(
                    error = %e,
                    tool = tool_name,
                    "state-spine: could not map tool to workflow node type — workflow graph will be incomplete"
                );
                self.emit_event(AgentEvent::Warning {
                    message: format!("Failed to map tool '{}' to workflow node: {}", tool_name, e),
                })
                .await;
                return None;
            }
        };

        // AX dispatch descriptor enrichment. The tool-mapping inbound path
        // writes `AxTarget::ResolvedUid(uid)`; upgrade to `Descriptor`
        // against the most recent native AX snapshot so the node replays
        // correctly after a fresh snapshot (different generation id).
        self.enrich_ax_descriptor(&mut node_type);

        let position = Position {
            x: 0.0,
            y: (self.state.workflow.nodes.len() as f32) * 120.0,
        };
        let node = Node::new(node_type, position, tool_name, "").with_run_id(self.run_id);
        let node_id = node.id;

        // Emit the live NodeAdded event before mutating the workflow so
        // subscribers observe creation order that matches the event stream.
        self.emit_event(AgentEvent::NodeAdded {
            node: Box::new(node.clone()),
        })
        .await;
        self.state.workflow.nodes.push(node);

        // Chain from the previous node (or the caller-supplied anchor on the
        // first iteration).
        if let Some(prev_id) = self.state.last_node_id {
            let edge = clickweave_core::Edge {
                from: prev_id,
                to: node_id,
            };
            self.emit_event(AgentEvent::EdgeAdded { edge: edge.clone() })
                .await;
            self.state.workflow.edges.push(edge);
        }

        self.state.last_node_id = Some(node_id);
        Some(node_id)
    }

    /// Queue invalidation events that the just-executed tool implies for
    /// the world model. Pure-observation tools (`take_ax_snapshot`,
    /// `take_screenshot`, `cdp_find_elements`, etc.) are no-ops here;
    /// state-transition tools queue the matching event so the next
    /// `observe()` call drops fields that the tool may have invalidated.
    ///
    /// Categories:
    /// - **Focus shift** (`focus_window`): drops focused-app, window list,
    ///   element surface, modal/dialog, screenshot, AX snapshot.
    /// - **App lifecycle** (`launch_app`, `quit_app`): same as focus shift.
    /// - **CDP navigation** (`cdp_navigate`, `cdp_new_page`,
    ///   `cdp_select_page`): drops the CDP page state, element surface,
    ///   and modal/dialog presence.
    ///
    /// Snapshot-staleness invalidation is event-driven from a separate
    /// top-of-loop hook (`queue_snapshot_stale_if_aged`), since it
    /// depends on the current step counter, not the tool that just ran.
    pub fn queue_invalidations_for_tool_success(&mut self, tool_name: &str, arguments: &Value) {
        if FOCUS_CHANGING_TOOLS.contains(&tool_name) {
            self.queue_invalidation(InvalidationEvent::FocusChanging {
                tool: tool_name.to_string(),
            });
        }
        if APP_LIFECYCLE_TOOLS.contains(&tool_name) {
            self.queue_invalidation(InvalidationEvent::AppLifecycle {
                tool: tool_name.to_string(),
            });
        }
        if CDP_NAVIGATION_TOOLS.contains(&tool_name) {
            let new_url = arguments
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            self.queue_invalidation(InvalidationEvent::CdpNavigation { new_url });
        }
    }

    /// Queue per-snapshot `SnapshotStale` events for any snapshot
    /// (`last_native_ax_snapshot` or `last_screenshot`) whose own age
    /// has crossed its `ttl_steps`. Called at the top of every loop
    /// iteration before `observe()` so the apply-events pass drops
    /// bodies that have aged out without the LLM re-capturing.
    ///
    /// One event per stale field — never a shared `age_steps` value
    /// across both fields. A fresh screenshot must not be invalidated
    /// just because the AX snapshot is stale.
    pub fn queue_snapshot_stale_if_aged(&mut self) {
        use crate::agent::world_model::SnapshotKind;
        if let Some(ax) = &self.world_model.last_native_ax_snapshot
            && let Some(ttl) = ax.ttl_steps
        {
            let age = (self.step_index.saturating_sub(ax.written_at)) as u32;
            if age > ttl {
                self.queue_invalidation(InvalidationEvent::SnapshotStale {
                    kind: SnapshotKind::NativeAx,
                    age_steps: age,
                });
            }
        }
        if let Some(ss) = &self.world_model.last_screenshot
            && let Some(ttl) = ss.ttl_steps
        {
            let age = (self.step_index.saturating_sub(ss.written_at)) as u32;
            if age > ttl {
                self.queue_invalidation(InvalidationEvent::SnapshotStale {
                    kind: SnapshotKind::Screenshot,
                    age_steps: age,
                });
            }
        }
    }

    /// After a successful tool call, refresh the world model's identity
    /// fields that the tool just captured. Non-snapshot tools are no-ops.
    pub fn update_continuity_after_tool_success(&mut self, tool_name: &str, body: &str) {
        use crate::agent::world_model::{
            AxSnapshotData, Fresh, FreshnessSource, ObservedElement, ScreenshotRef,
            parse_ax_snapshot, parse_ocr_matches,
        };
        match tool_name {
            "take_ax_snapshot" => {
                let parsed = parse_ax_snapshot(body);
                let snapshot_id = parsed
                    .first()
                    .map(|e| e.uid.clone())
                    .unwrap_or_else(|| format!("ax-{}", self.step_index));
                self.world_model.last_native_ax_snapshot = Some(Fresh {
                    value: AxSnapshotData {
                        snapshot_id,
                        element_count: parsed.len(),
                        captured_at_step: self.step_index,
                        ax_tree_text: body.to_string(),
                    },
                    written_at: self.step_index,
                    source: FreshnessSource::DirectObservation,
                    ttl_steps: Some(8),
                });
                // Mirror parsed AX elements into the source-agnostic
                // element surface so the renderer prints them alongside
                // (or instead of) CDP elements. Native-only paths
                // depend on this — without it the LLM never sees the
                // a-prefixed uid vocabulary in `<world_model>`.
                if !parsed.is_empty() {
                    let observed: Vec<ObservedElement> =
                        parsed.into_iter().map(ObservedElement::Ax).collect();
                    self.world_model.elements = Some(Fresh {
                        value: observed,
                        written_at: self.step_index,
                        source: FreshnessSource::DirectObservation,
                        ttl_steps: Some(8),
                    });
                }
            }
            "take_screenshot" => {
                let id = serde_json::from_str::<serde_json::Value>(body)
                    .ok()
                    .and_then(|v| {
                        v.get("screenshot_id")
                            .and_then(|s| s.as_str())
                            .map(String::from)
                    })
                    .unwrap_or_else(|| format!("ss-{}", self.step_index));
                self.world_model.last_screenshot = Some(Fresh {
                    value: ScreenshotRef {
                        screenshot_id: id,
                        captured_at_step: self.step_index,
                    },
                    written_at: self.step_index,
                    source: FreshnessSource::DirectObservation,
                    ttl_steps: Some(8),
                });
            }
            "find_text" => {
                // OCR results from `find_text` populate the
                // source-agnostic element surface as `ObservedElement::Ocr`
                // when the response is parseable. Parse failures are
                // tolerated silently — `find_text` has multiple legacy
                // body shapes, so a non-OCR-shaped body is normal.
                if let Ok(matches) = parse_ocr_matches(body)
                    && !matches.is_empty()
                {
                    let observed: Vec<ObservedElement> =
                        matches.into_iter().map(ObservedElement::Ocr).collect();
                    self.world_model.elements = Some(Fresh {
                        value: observed,
                        written_at: self.step_index,
                        source: FreshnessSource::DirectObservation,
                        ttl_steps: Some(2),
                    });
                }
            }
            _ => {}
        }
    }

    /// Fetch compact CDP page inventory from the current page via MCP.
    ///
    /// This deliberately calls `cdp_summarize_page`, not
    /// `cdp_find_elements`: the top-of-loop observation should tell the model
    /// which page and element categories exist without injecting a transient
    /// page-wide DOM list into every prompt. Explicit target candidates enter
    /// the transcript only when the agent asks for `cdp_find_elements`, and
    /// ambiguous matches can be expanded with `cdp_get_element_context`.
    pub(crate) async fn fetch_cdp_page_summary<M: Mcp + ?Sized>(
        &mut self,
        mcp: &M,
    ) -> CdpPageObservation {
        if !mcp.has_tool("cdp_summarize_page") {
            // No CDP surface this turn — clear the sticky URL so the
            // next-turn state-block mirror does not render a stale page.
            self.state.current_url = String::new();
            return CdpPageObservation::default();
        }
        match mcp
            .call_tool("cdp_summarize_page", Some(serde_json::json!({})))
            .await
        {
            Ok(result) if result.is_error != Some(true) => {
                let text = crate::cdp_lifecycle::extract_text(&result);
                match serde_json::from_str::<clickweave_core::cdp::CdpPageSummaryResponse>(&text) {
                    Ok(parsed) => {
                        self.state.current_url = parsed.page_url.clone();
                        let page_fingerprint = crate::agent::transition::page_inventory_fingerprint(
                            &parsed.page_url,
                            &parsed.inventory,
                        );
                        return CdpPageObservation {
                            page_url: parsed.page_url,
                            page_fingerprint,
                            inventory: parsed
                                .inventory
                                .into_iter()
                                .map(CdpElementInventorySummary::from)
                                .collect(),
                        };
                    }
                    Err(parse_err) => {
                        tracing::debug!(
                            error = %parse_err,
                            "state-spine: failed to parse cdp_summarize_page response"
                        );
                        self.emit_event(AgentEvent::Warning {
                            message: format!(
                                "cdp_summarize_page response failed to parse: {} — continuing without CDP page summary",
                                parse_err
                            ),
                        })
                        .await;
                        // Parse failure — clear the sticky URL so a later
                        // turn does not keep rendering the previous page.
                        self.state.current_url = String::new();
                    }
                }
            }
            Ok(_) => {
                // MCP returned `is_error=true` or a non-Ok result — treat
                // as "no fresh observation" and drop the sticky URL.
                self.state.current_url = String::new();
            }
            Err(e) => {
                tracing::debug!(error = %e, "state-spine: cdp_summarize_page call failed");
                self.state.current_url = String::new();
            }
        }
        CdpPageObservation::default()
    }

    /// Build a terminal `StepRecord` for a completed / halted run. Used by
    /// the control loop on run-end boundaries and by integration tests.
    pub fn build_step_record(
        &self,
        boundary_kind: crate::agent::step_record::BoundaryKind,
        action_taken: serde_json::Value,
        outcome: serde_json::Value,
    ) -> crate::agent::step_record::StepRecord {
        use crate::agent::step_record::{StepRecord, WorldModelSnapshot};
        StepRecord {
            step_index: self.step_index,
            boundary_kind,
            world_model_snapshot: WorldModelSnapshot::from_world_model(&self.world_model),
            task_state_snapshot: self.task_state.clone(),
            action_taken,
            outcome,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Persist one `BoundaryKind::SubgoalCompleted` record per
    /// milestone appended during the current turn. Called from the
    /// outer loop in [`Self::run`] right after the mutation apply
    /// counts a positive `outer_milestones_appended` — before any
    /// early-exit branch (synthetic focus skip / live policy-deny /
    /// live approval-reject), so the boundary record fires whether or
    /// not the action eventually goes through `run_turn`. Records the
    /// turn's batched mutations as `action_taken` so the subgoal
    /// summaries are recoverable from `events.jsonl` without a
    /// separate transcript lookup. Emits one
    /// `AgentEvent::BoundaryRecordWritten` per persisted record.
    async fn write_subgoal_completed_records(&mut self, count: usize, turn: &AgentTurn) {
        let action_taken =
            serde_json::to_value(&turn.mutations).unwrap_or_else(|_| serde_json::json!([]));
        let milestone_start = self.task_state.milestones.len().saturating_sub(count);
        for i in 0..count {
            let milestone_text = self
                .task_state
                .milestones
                .get(milestone_start + i)
                .map(|m| m.text.clone());
            self.persist_boundary_record(
                crate::agent::step_record::BoundaryKind::SubgoalCompleted,
                action_taken.clone(),
                serde_json::json!({"kind": "subgoal_completed"}),
                milestone_text,
            )
            .await;
        }

        // Spec 3: drain the extraction queue populated by
        // `apply_mutations`. Each completed-subgoal milestone has both
        // its push-side `recorded_steps` index, the milestone payload,
        // and the node lineage for that subgoal frame available without
        // re-walking `task_state.milestones`.
        let queue = std::mem::take(&mut self.completed_subgoal_extraction_queue);
        if !queue.is_empty() && self.skill_ctx.enabled && self.config.skills_enabled {
            let workflow_hash = self.episodic_ctx.workflow_hash.clone();
            let run_id = self.run_id;
            let step_index = self.state.steps.len();

            for (push_idx, milestone, pre_state_sig, produced_node_ids) in queue {
                let action_sequence = if push_idx < self.recorded_steps.len() {
                    self.recorded_steps[push_idx..].to_vec()
                } else {
                    Vec::new()
                };
                match crate::agent::skills::extractor::maybe_extract_skill(
                    &milestone,
                    &action_sequence,
                    pre_state_sig,
                    &self.world_model,
                    &self.skill_index,
                    &self.skill_store,
                    &self.skill_ctx,
                    run_id,
                    &workflow_hash,
                    step_index,
                    &produced_node_ids,
                )
                .await
                {
                    Ok(crate::agent::skills::MaybeExtracted::Inserted {
                        skill_id,
                        version,
                        ..
                    })
                    | Ok(crate::agent::skills::MaybeExtracted::Merged {
                        skill_id, version, ..
                    }) => {
                        let (state, scope) = self
                            .skill_index
                            .read()
                            .get(&skill_id, version)
                            .map(|s| (s.state, s.scope))
                            .unwrap_or((
                                crate::agent::skills::SkillState::Draft,
                                crate::agent::skills::SkillScope::ProjectLocal,
                            ));
                        self.emit_event(AgentEvent::SkillExtracted {
                            run_id: self.run_id,
                            skill_id,
                            version,
                            state,
                            scope,
                        })
                        .await;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        tracing::warn!(?err, "skills: extraction failed; continuing");
                    }
                }
            }
        }
    }

    /// Persist one `BoundaryKind::RecoverySucceeded` record on the exact
    /// `Recovering -> Executing` transition (D8). Called from
    /// [`Self::run`] when a tool success cleared the consecutive-error
    /// streak. `action_taken` / `outcome` record the successful turn so
    /// Spec 2's episodic memory can reason about what resolved the
    /// recovery. Emits one `AgentEvent::BoundaryRecordWritten` (D17).
    async fn write_recovery_succeeded_record(&self, turn: &AgentTurn, outcome: &TurnOutcome) {
        let action_taken =
            serde_json::to_value(&turn.action).unwrap_or_else(|_| serde_json::json!({}));
        let outcome_json = match outcome {
            TurnOutcome::ToolSuccess {
                tool_name,
                tool_body,
            } => serde_json::json!({
                "kind": "tool_success",
                "tool_name": tool_name,
                "body_len": tool_body.len(),
            }),
            // RecoverySucceeded is only written on ToolSuccess; the other
            // variants never reach this path (see `run()`'s guard).
            _ => serde_json::json!({"kind": "tool_success"}),
        };
        self.persist_boundary_record(
            crate::agent::step_record::BoundaryKind::RecoverySucceeded,
            action_taken,
            outcome_json,
            None,
        )
        .await;
    }

    /// Persist the single `BoundaryKind::Terminal` record at run end (D8).
    /// Called exactly once from [`Self::run`] after the control loop has
    /// populated `state.terminal_reason`. Encodes the terminal reason into
    /// the outcome payload so the record is self-describing without a
    /// cross-reference to the rest of `events.jsonl`. Emits one
    /// `AgentEvent::BoundaryRecordWritten` (D17).
    async fn write_terminal_record(&self) {
        let terminal_reason = self.state.terminal_reason.as_ref();
        let outcome_json = terminal_reason
            .map(|tr| serde_json::to_value(tr).unwrap_or_else(|_| serde_json::json!({})))
            .unwrap_or_else(|| serde_json::json!({"kind": "unknown"}));
        // Best-effort action_taken: a minimal projection of the last
        // recorded step (tool_name only — `AgentCommand` itself isn't
        // `Serialize`). Falls back to the outcome for zero-step runs.
        let action_taken = self
            .state
            .steps
            .last()
            .map(|step| {
                serde_json::json!({
                    "tool_name": step.command.tool_name_or_unknown(),
                    "step_index": step.index,
                })
            })
            .unwrap_or_else(|| outcome_json.clone());
        self.persist_boundary_record(
            crate::agent::step_record::BoundaryKind::Terminal,
            action_taken,
            outcome_json,
            None,
        )
        .await;
    }

    /// Shared body for the three `write_*_record` boundary paths: build
    /// the `StepRecord`, persist via `RunStorage`, and emit the matching
    /// `BoundaryRecordWritten` event.
    async fn persist_boundary_record(
        &self,
        boundary_kind: crate::agent::step_record::BoundaryKind,
        action_taken: serde_json::Value,
        outcome: serde_json::Value,
        milestone_text: Option<String>,
    ) {
        let record = self.build_step_record(boundary_kind.clone(), action_taken, outcome);
        self.write_step_record(&record);
        self.emit_event(AgentEvent::BoundaryRecordWritten {
            run_id: self.run_id,
            boundary_kind,
            step_index: record.step_index,
            milestone_text,
        })
        .await;
    }
}

/// Outcome of a single `StateRunner::run_turn` call — what the caller needs
/// to drive the next iteration.
#[derive(Debug, Clone)]
pub enum TurnOutcome {
    /// Tool call was dispatched; `tool_body` is the successful result text.
    ToolSuccess {
        tool_name: String,
        tool_body: String,
    },
    /// Tool call was dispatched; tool returned an error.
    ToolError { tool_name: String, error: String },
    /// Agent signaled completion.
    Done { summary: String },
    /// Agent requested replan.
    Replan { reason: String },
}

/// Executes an MCP tool call and returns either its successful body or an
/// error message. Integration tests stub this with a deterministic sequence;
/// Phase 3 cutover will bind it to the real `McpClient`.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, String>;
}

impl StateRunner {
    /// Apply one `AgentTurn` in the state-spine control flow:
    ///
    /// 1. Apply mutations in order (errors become warnings, not fatal).
    /// 2. Observe (absorb any queued invalidation events + re-infer phase).
    /// 3. Dispatch the action:
    ///     - `ToolCall`: call the executor, update continuity on success,
    ///       queue `ToolFailed` and bump `consecutive_errors` on error.
    ///     - `AgentDone` / `AgentReplan`: return the terminal outcome.
    /// 4. Advance `step_index`.
    ///
    /// Integration tests drive this with deterministic `AgentTurn`s; Phase 3
    /// is wrapped by the LLM loop + compaction in [`Self::run_inner`].
    ///
    /// Return tuple: `(outcome, warnings, milestones_appended)`.
    /// `milestones_appended` counts `CompleteSubgoal` mutations that
    /// successfully popped a subgoal off the stack during this turn.
    /// In the live runner the outer loop applies mutations *before*
    /// calling `run_turn` (so `run_turn` receives an action-only turn
    /// and the count returned here is `0`); the count is meaningful
    /// for integration tests that drive `run_turn` directly with
    /// non-empty mutation batches.
    pub async fn run_turn<E: ToolExecutor + ?Sized>(
        &mut self,
        turn: &AgentTurn,
        executor: &E,
    ) -> (TurnOutcome, Vec<String>, usize) {
        // 1. Apply mutations first — phase inference reads the stack/watch state.
        //    Count successful `CompleteSubgoal` mutations by diffing the
        //    milestones vec length (each `CompleteSubgoal` that passes
        //    validation appends exactly one `Milestone`; see
        //    `TaskState::apply`). Milestones don't shrink during normal
        //    operation, so the delta is an exact count of new milestones.
        let milestones_before = self.task_state.milestones.len();
        let warnings = self.apply_mutations(&turn.mutations);
        let milestones_appended = self
            .task_state
            .milestones
            .len()
            .saturating_sub(milestones_before);

        // 1a. Emit `TaskStateChanged` once per turn when `apply_mutations`
        //     had anything to apply (D17). The event reflects the full
        //     post-mutation state so subscribers never have to reassemble
        //     it from the warnings vec.
        if !turn.mutations.is_empty() {
            self.emit_event(AgentEvent::TaskStateChanged {
                run_id: self.run_id,
                task_state: self.task_state.clone(),
            })
            .await;
        }

        // 2. Observe: snapshot field signatures → drain pending events +
        //    re-infer phase → compute diff → emit `WorldModelChanged` (D17).
        //    If `run()` captured signatures before its observe-phase
        //    mirror (`fetch_cdp_page_summary` → `world_model.cdp_page`)
        //    use that baseline so direct-observation writes also surface
        //    in `changed_fields`; otherwise (unit/test callers) fall back
        //    to snapshotting here.
        let pre_signatures = self
            .turn_pre_signatures
            .take()
            .unwrap_or_else(|| self.world_model.field_signatures());
        let prev_phase = self.task_state.phase;
        self.observe();
        if prev_phase != self.task_state.phase {
            self.emit_event(AgentEvent::TaskStateChanged {
                run_id: self.run_id,
                task_state: self.task_state.clone(),
            })
            .await;
        }
        let post_signatures = self.world_model.field_signatures();
        let diff = diff_world_model_signatures(&pre_signatures, &post_signatures);
        self.emit_event(AgentEvent::WorldModelChanged {
            run_id: self.run_id,
            diff,
        })
        .await;

        // 3. Dispatch action.
        let outcome = match &turn.action {
            AgentAction::ToolCall {
                tool_name,
                arguments,
                ..
            } => match executor.call_tool(tool_name, arguments).await {
                Ok(body) => {
                    self.update_continuity_after_tool_success(tool_name, &body);
                    self.queue_invalidations_for_tool_success(tool_name, arguments);
                    self.consecutive_errors = 0;
                    TurnOutcome::ToolSuccess {
                        tool_name: tool_name.clone(),
                        tool_body: body,
                    }
                }
                Err(error) => {
                    self.consecutive_errors += 1;
                    let stale_cdp_uid = is_stale_cdp_uid_error(tool_name, &error);
                    if stale_cdp_uid {
                        self.world_model.elements = None;
                    }
                    let error = if stale_cdp_uid {
                        build_stale_cdp_uid_nudge(&error)
                    } else {
                        error
                    };
                    self.queue_invalidation(InvalidationEvent::ToolFailed {
                        tool: tool_name.clone(),
                    });
                    TurnOutcome::ToolError {
                        tool_name: tool_name.clone(),
                        error,
                    }
                }
            },
            AgentAction::AgentDone { summary } => TurnOutcome::Done {
                summary: summary.clone(),
            },
            AgentAction::AgentReplan { reason } => {
                self.last_replan_step = Some(self.step_index);
                TurnOutcome::Replan {
                    reason: reason.clone(),
                }
            }
            AgentAction::InvokeSkill {
                skill_id,
                version,
                parameters,
            } => {
                // Phase 4: validate the skill exists + parameter
                // shape + emit `SkillInvoked`. The per-step expansion
                // (Task 4.3 follow-up) hasn't landed yet, so this arm
                // returns a replan that names the resolved skill so
                // the next LLM turn has a clear breadcrumb. Errors at
                // lookup / validation time produce an `InvalidArgs`-
                // shaped replan instead of panicking so a malformed
                // `invoke_skill` call can't take the run down.
                match self
                    .dispatch_skill(skill_id, *version, parameters.clone())
                    .await
                {
                    Ok(frame) => TurnOutcome::Replan {
                        reason: format!(
                            "skill {}@v{} resolved with {} parameter(s); replay engine pending — falling back to LLM",
                            frame.skill.id,
                            frame.skill.version,
                            frame.params.as_object().map(|m| m.len()).unwrap_or(0),
                        ),
                    },
                    Err(reason) => TurnOutcome::Replan { reason },
                }
            }
        };

        // `step_index` is owned by the outer-loop call sites that record
        // an `AgentStep` (via `advance_recorded_step_index`). `run_turn`
        // intentionally does not advance it — early-continue paths
        // (synthetic focus skip, policy deny, approval reject) record
        // their own steps without going through
        // `run_turn`, and prior to this fix the divergent advancement
        // let `step_index == 0` re-fire D24 run-start retrieval after
        // the run had already taken actions.

        (outcome, warnings, milestones_appended)
    }

    /// Advance the recorded-step counter. Single owner of `step_index`
    /// updates. Call after every `self.state.steps.push(...)` site so
    /// `step_index` matches `state.steps.len()` and the prompt's
    /// rendered step number stays in sync with what the run has
    /// actually executed.
    pub(crate) fn advance_recorded_step_index(&mut self) {
        self.step_index += 1;
    }

    /// Emit a per-step `WorldModelChanged` event for an early-exit step
    /// path that recorded an `AgentStep` without going through
    /// `run_turn`. Live policy-deny, live approval-reject, and the synthetic
    /// `focus_window` skip all record steps but skip `run_turn`
    /// entirely; without this hook, the `turn_pre_signatures` baseline
    /// would be carried into the next iteration and the
    /// `WorldModelChanged` diff would span multiple recorded steps.
    ///
    /// Consumes the current baseline (top-of-loop snapshot) and
    /// re-seeds it with the post-step signatures so the next iteration
    /// sees a fresh baseline keyed to the just-recorded step.
    pub(crate) async fn emit_world_model_changed_for_recorded_step(&mut self) {
        let pre_signatures = self
            .turn_pre_signatures
            .take()
            .unwrap_or_else(|| self.world_model.field_signatures());
        let post_signatures = self.world_model.field_signatures();
        let diff = diff_world_model_signatures(&pre_signatures, &post_signatures);
        self.emit_event(AgentEvent::WorldModelChanged {
            run_id: self.run_id,
            diff,
        })
        .await;
        self.turn_pre_signatures = Some(post_signatures);
    }

    /// Record a permission-policy denial as the current "last failure"
    /// so any subsequent `Recovering`-entry snapshot captures a real
    /// `(failed_tool, error_kind)` pair instead of the empty defaults.
    /// `error_kind` is the stable string `"policy_denied"` so episodic
    /// retrieval can group denied-tool recoveries by failure family
    /// without parsing the human-readable message.
    pub(crate) fn record_policy_deny_failure(&mut self, tool_name: &str) {
        self.last_failed_tool_name = Some(tool_name.to_string());
        self.last_failed_error_kind = Some("policy_denied".to_string());
    }

    /// Mirror of `record_policy_deny_failure`'s clear half. Called by
    /// every recovery-success path (live ToolSuccess in `run_turn`,
    /// synthetic focus-window skip) so a prior
    /// deny / tool-error doesn't bleed into a later Recovering snapshot
    /// after the agent has demonstrably recovered.
    pub(crate) fn clear_last_failure_tracking(&mut self) {
        self.last_failed_tool_name = None;
        self.last_failed_error_kind = None;
    }

    /// Bump the success-side repeat-action tracker for one dispatched
    /// non-observation tool call. Returns the no-progress nudge string
    /// when the streak crosses [`REPEAT_ACTION_THRESHOLD`], `None`
    /// otherwise. Caller installs the nudge into `previous_result` so
    /// the next turn renders it as the observation; the warning event
    /// is emitted here.
    ///
    /// Called by the live `ToolSuccess` arm so repeated live dispatches
    /// contribute to the same streak count.
    async fn track_repeat_action(
        &mut self,
        tool_name: &str,
        tool_arguments: &Value,
        tool_body: &str,
        annotations_by_tool: &HashMap<String, ToolAnnotations>,
        last_action: &mut Option<LastActionProgress>,
        recent_actions: &mut VecDeque<ActionProgressSignature>,
    ) -> Option<String> {
        if is_observation_tool(tool_name, annotations_by_tool) {
            return None;
        }
        let context_signature = stable_no_progress_context_signature(&self.world_model);
        if last_action
            .as_ref()
            .is_some_and(|last| last.context_signature != context_signature)
        {
            *last_action = None;
            recent_actions.clear();
        }
        let signature = ActionProgressSignature {
            tool_name: tool_name.to_string(),
            arguments: tool_arguments.clone(),
            context_signature: context_signature.clone(),
        };
        if recent_actions.len() == ACTION_CYCLE_WINDOW {
            recent_actions.pop_front();
        }
        recent_actions.push_back(signature);
        let same_as_last = matches!(
            last_action.as_ref(),
            Some(last)
                if last.tool_name == tool_name
                    && last.arguments == *tool_arguments
                    && last.context_signature == context_signature
        );
        let count = if same_as_last {
            last_action.as_ref().map(|last| last.count).unwrap_or(0) + 1
        } else {
            1
        };
        *last_action = Some(LastActionProgress {
            tool_name: tool_name.to_string(),
            arguments: tool_arguments.clone(),
            context_signature,
            count,
        });
        if count < REPEAT_ACTION_THRESHOLD {
            if let Some(cycle) = detect_repeated_action_cycle(recent_actions) {
                let cycle_summary = cycle.join(" -> ");
                warn!(
                    cycle = %cycle_summary,
                    "state-spine: repeated action cycle detected — injecting no-progress nudge"
                );
                self.emit_event(AgentEvent::Warning {
                    message: format!(
                        "{}: repeated action cycle `{}`",
                        NO_PROGRESS_WARNING_PREFIX, cycle_summary
                    ),
                })
                .await;
                return Some(build_action_cycle_nudge(&cycle_summary, tool_body));
            }
            return None;
        }
        warn!(
            tool = %tool_name,
            count,
            "state-spine: repeat-action threshold reached — injecting no-progress nudge"
        );
        self.emit_event(AgentEvent::Warning {
            message: format!(
                "{}: `{}` repeated {} turns in a row",
                NO_PROGRESS_WARNING_PREFIX, tool_name, count
            ),
        })
        .await;
        Some(build_no_progress_nudge(tool_name, count, tool_body))
    }

    async fn track_post_text_submit_search(
        &mut self,
        tool_name: &str,
        tool_arguments: &Value,
        tool_body: &str,
        pending: &mut Option<TextSubmitSearchProgress>,
    ) -> Option<String> {
        if is_text_composition_tool(tool_name) {
            *pending = Some(TextSubmitSearchProgress {
                context_signature: stable_no_progress_context_signature(&self.world_model),
                count: 0,
            });
            return None;
        }

        if tool_name != "cdp_find_elements" {
            if !OBSERVATION_TOOLS.contains(&tool_name) {
                *pending = None;
            }
            return None;
        }

        if !is_send_submit_cdp_search(tool_arguments) {
            return None;
        }

        let Some(progress) = pending.as_mut() else {
            return None;
        };
        let context_signature = stable_no_progress_context_signature(&self.world_model);
        if progress.context_signature != context_signature {
            *pending = None;
            return None;
        }

        if cdp_find_elements_has_matches(tool_body) != Some(false) {
            progress.count = 0;
            return None;
        }

        progress.count += 1;
        if progress.count < TEXT_SUBMIT_SEARCH_THRESHOLD {
            return None;
        }

        warn!(
            count = progress.count,
            "state-spine: repeated post-text send search detected — injecting no-progress nudge"
        );
        self.emit_event(AgentEvent::Warning {
            message: format!(
                "{}: repeated send/submit search after composing text",
                NO_PROGRESS_WARNING_PREFIX
            ),
        })
        .await;
        Some(build_post_text_submit_nudge(progress.count, tool_body))
    }
}

/// Result of requesting user approval for a tool action. Shared by both
/// policy evaluation and the live dispatch path.
enum ApprovalResult {
    Approved,
    Rejected,
    Unavailable,
}

/// State of the consecutive-destructive-tool cap after a tool call.
/// Mirrors the legacy `CapStatus` — private to `runner.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapStatus {
    /// Streak is still below the cap — run continues normally.
    Armed,
    /// Cap reached — the caller must emit the cap-hit event and halt.
    CapReached,
}

impl StateRunner {
    fn skill_frame_to_single_step_action(frame: &crate::agent::skills::SkillFrame) -> AgentAction {
        match frame.skill.action_sketch.as_slice() {
            [crate::agent::skills::ActionSketchStep::ToolCall { tool, args, .. }] => {
                match crate::agent::skills::substitution::substitute_value(
                    args,
                    &frame.params,
                    &frame.captured,
                ) {
                    Ok(arguments) => AgentAction::ToolCall {
                        tool_name: tool.clone(),
                        arguments,
                        tool_call_id: format!(
                            "skill-{}-v{}-step-{}",
                            frame.skill.id, frame.skill.version, frame.next_step
                        ),
                    },
                    Err(err) => AgentAction::AgentReplan {
                        reason: format!("skill replay substitution failed: {err}"),
                    },
                }
            }
            [] => AgentAction::AgentReplan {
                reason: format!(
                    "skill {}@v{} has no replay steps",
                    frame.skill.id, frame.skill.version
                ),
            },
            [_] => AgentAction::AgentReplan {
                reason: format!(
                    "skill {}@v{} contains a non-tool replay step; full replay is not available yet",
                    frame.skill.id, frame.skill.version
                ),
            },
            steps => AgentAction::AgentReplan {
                reason: format!(
                    "skill {}@v{} has {} replay steps; full multi-step replay is not available yet",
                    frame.skill.id,
                    frame.skill.version,
                    steps.len()
                ),
            },
        }
    }

    /// Look up the named skill, validate parameters against its
    /// schema, and emit `AgentEvent::SkillInvoked`. Returns the live
    /// [`SkillFrame`] on success or a human-readable replan reason on
    /// failure (unknown skill, draft skill, invalid parameters).
    ///
    /// Phase 4 lands the lookup-and-validate half of `dispatch_skill`.
    /// The per-step expansion through the live dispatch helper —
    /// including sub-skill recursion, the `Loop` arm, and the
    /// LLM-fallback path on divergence — is staged for the follow-up
    /// pass. See the Phase 4 deferred-items list in the handoff for
    /// the resume seam. Until that lands, the outer-loop
    /// `AgentAction::InvokeSkill` arm degrades to a replan whose reason
    /// names the skill that was about to run, so a live invocation
    /// produces a clear bail-out rather than a silent no-op.
    pub(crate) async fn dispatch_skill(
        &mut self,
        skill_id: &str,
        version: u32,
        parameters: serde_json::Value,
    ) -> Result<crate::agent::skills::SkillFrame, String> {
        use crate::agent::skills::replay::{SkillFrame, validate_parameters};
        use crate::agent::skills::types::SkillState;

        let skill = match self.skill_index.read().get(skill_id, version) {
            Some(s) if !matches!(s.state, SkillState::Draft) => s,
            Some(_) => {
                return Err(format!(
                    "skill {skill_id}@v{version} is in draft state and cannot be invoked"
                ));
            }
            None => {
                return Err(format!("unknown skill: {skill_id}@v{version}"));
            }
        };

        let validated_params = match validate_parameters(&parameters, &skill.parameter_schema) {
            Ok(p) => p,
            Err(e) => return Err(format!("invalid skill parameters: {e}")),
        };

        let parameter_count = validated_params
            .as_object()
            .map(|m| m.len() as u32)
            .unwrap_or(0);
        self.emit_event(AgentEvent::SkillInvoked {
            run_id: self.run_id,
            skill_id: skill_id.to_string(),
            version,
            parameter_count,
        })
        .await;

        // Stamp `last_invoked_at` so the index reflects the attempt
        // even when the per-step expansion hasn't landed yet.
        self.skill_index
            .write()
            .mark_invoked(skill_id, version, chrono::Utc::now());

        Ok(SkillFrame::new(skill, validated_params))
    }

    /// Best-effort send of an [`AgentEvent`] through the configured
    /// channel. No-op when the channel is unset or closed — event
    /// emission must never fail the run.
    async fn emit_event(&self, event: AgentEvent) {
        let Some(tx) = &self.event_tx else { return };
        if tx.is_closed() {
            return;
        }
        if let Err(e) = tx.send(RunnerOutput::Event(event)).await {
            warn!("state-spine: failed to emit agent event (channel closed): {e}");
        }
    }

    /// Update the consecutive-destructive-call tracker after a successful
    /// tool call, and report whether the cap has now been hit. Port of
    /// the legacy `AgentRunner::maybe_halt_on_destructive_cap`.
    ///
    /// `destructive_hint == Some(true)` increments the streak; anything else
    /// resets it. A cap value of `0` disables the feature entirely, so the
    /// method always returns `CapStatus::Armed` in that case.
    fn maybe_halt_on_destructive_cap(
        &mut self,
        tool_name: &str,
        annotations_by_tool: &HashMap<String, ToolAnnotations>,
    ) -> CapStatus {
        if self.config.consecutive_destructive_cap == 0 {
            return CapStatus::Armed;
        }
        let destructive = annotations_by_tool
            .get(tool_name)
            .and_then(|a| a.destructive_hint)
            .unwrap_or(false);
        if destructive {
            self.state
                .recent_destructive_tools
                .push(tool_name.to_string());
        } else {
            self.state.recent_destructive_tools.clear();
        }
        if self.state.recent_destructive_tools.len() >= self.config.consecutive_destructive_cap {
            CapStatus::CapReached
        } else {
            CapStatus::Armed
        }
    }

    /// Halt the run because the consecutive-destructive cap was reached.
    /// Emits the cap-hit event and sets the terminal reason. Called once
    /// when `maybe_halt_on_destructive_cap` reports `CapStatus::CapReached`.
    /// Clears `recent_destructive_tools` afterwards so state serialization
    /// reflects the drained streak. Port of the legacy
    /// `AgentRunner::emit_destructive_cap_hit`.
    async fn emit_destructive_cap_hit(&mut self) {
        let recent = std::mem::take(&mut self.state.recent_destructive_tools);
        let cap = self.config.consecutive_destructive_cap;
        warn!(
            cap,
            tools = ?recent,
            "state-spine: consecutive destructive cap reached — halting run"
        );
        self.emit_event(AgentEvent::ConsecutiveDestructiveCapHit {
            recent_tool_names: recent.clone(),
            cap,
        })
        .await;
        self.state.terminal_reason = Some(TerminalReason::ConsecutiveDestructiveCap {
            recent_tool_names: recent,
            cap,
        });
    }

    /// Evaluate the permission policy for a tool call.
    fn policy_for(
        &self,
        tool_name: &str,
        arguments: &Value,
        annotations_by_tool: &HashMap<String, ToolAnnotations>,
    ) -> PermissionAction {
        let ann = annotations_by_tool
            .get(tool_name)
            .copied()
            .unwrap_or_default();
        evaluate_permission(&self.permissions, tool_name, arguments, &ann)
    }

    /// Prompt the operator for approval of a tool action. Port of the
    /// legacy `AgentRunner::request_approval`. Returns `None` when no
    /// approval gate is configured (auto-approve).
    ///
    /// `description_suffix` is appended to the human-facing description for
    /// callers that need extra context.
    async fn request_approval(
        &self,
        tool_name: &str,
        arguments: &Value,
        step_index: usize,
        description_suffix: &str,
    ) -> Option<ApprovalResult> {
        let gate = self.approval_gate.as_ref()?;
        let description = format!(
            "{} with {}{}",
            tool_name,
            serde_json::to_string(arguments).unwrap_or_default(),
            description_suffix,
        );
        let request = ApprovalRequest {
            step_index,
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
            description,
        };
        let (resp_tx, resp_rx) = oneshot::channel();
        if gate.request_tx.send((request, resp_tx)).await.is_ok() {
            match resp_rx.await {
                Ok(true) => {
                    debug!(tool = %tool_name, "state-spine: user approved action");
                    Some(ApprovalResult::Approved)
                }
                Ok(false) => {
                    tracing::info!(tool = %tool_name, "state-spine: user rejected action");
                    Some(ApprovalResult::Rejected)
                }
                Err(_) => {
                    warn!(tool = %tool_name, "state-spine: approval channel closed");
                    Some(ApprovalResult::Unavailable)
                }
            }
        } else {
            warn!(tool = %tool_name, "state-spine: approval channel send failed");
            Some(ApprovalResult::Unavailable)
        }
    }

    /// Verify an agent-reported completion against a fresh screenshot via
    /// the VLM. Port of the legacy `AgentRunner::verify_completion`.
    ///
    /// Returns the prepared base64 screenshot + VLM reply **only when the
    /// VLM disagreed** (verdict = NO). The caller uses that payload to
    /// synthesise a `CompletionDisagreement` event and terminal reason.
    /// When the VLM agrees, or any step of the verification path fails (no
    /// vision backend, screenshot failure, VLM call failure, empty reply),
    /// returns `None` and the caller falls through to the normal
    /// `Completed` path — verification errors must not tank the run.
    ///
    /// On both YES and NO verdicts, a PNG screenshot + JSON metadata are
    /// written to `self.verification_artifacts_dir` when set. Persistence
    /// failures are logged at `warn` and do not affect the return value.
    async fn verify_completion<M: Mcp + ?Sized>(
        &mut self,
        goal: &str,
        summary: &str,
        mcp: &M,
    ) -> Option<(String, String)> {
        use crate::agent::completion_check::{
            VlmVerdict, build_completion_prompt, parse_yes_no, persist_verification_artifacts,
            pick_completion_screenshot_scope,
        };
        use crate::executor::screenshot::capture_screenshot_for_vlm;

        let vision = self.vision.as_ref()?.clone();

        // Target the screenshot scope at the connected CDP app when we
        // have one — Task 3a.6 wires `cdp_state` up via
        // `maybe_cdp_connect`, so `connected_app` now flows through to
        // the scope picker (matching legacy behaviour).
        let scope = pick_completion_screenshot_scope(self.cdp_state.connected_app.as_ref());
        let Some((prepared_b64, mime)) = capture_screenshot_for_vlm(mcp, scope.clone()).await
        else {
            warn!(
                scope = ?scope,
                "state-spine: completion verification screenshot capture failed — skipping VLM check",
            );
            return None;
        };

        let messages = vec![Message::user_with_images(
            build_completion_prompt(goal, summary),
            vec![(prepared_b64.clone(), mime)],
        )];
        let raw_reply = match vision.chat_boxed(&messages, None).await {
            Ok(resp) => resp
                .choices
                .first()
                .and_then(|c| c.message.content_text())
                .map(str::to_owned),
            Err(e) => {
                warn!(error = %e, "state-spine: VLM call failed — skipping completion check");
                return None;
            }
        };
        let reply = match raw_reply {
            Some(r) if !r.trim().is_empty() => r,
            _ => {
                warn!("state-spine: VLM returned empty reply — skipping completion check");
                return None;
            }
        };

        let verdict = parse_yes_no(&reply);

        // Persist artifacts for both verdicts so every verification call
        // leaves forensic evidence. Failures are non-fatal.
        if let Some(dir) = &self.verification_artifacts_dir {
            let ordinal = self.verification_count;
            if let Err(e) = persist_verification_artifacts(
                dir,
                ordinal,
                verdict,
                &reply,
                goal,
                summary,
                &prepared_b64,
            ) {
                warn!(
                    ordinal,
                    error = %e,
                    "state-spine: failed to persist completion-verification artifacts (non-fatal)",
                );
            }
        }
        self.verification_count += 1;

        match verdict {
            VlmVerdict::Yes => {
                tracing::info!(reply = %reply, "state-spine: VLM confirmed completion");
                None
            }
            VlmVerdict::No => {
                tracing::info!(reply = %reply, "state-spine: VLM rejected completion");
                Some((prepared_b64, reply))
            }
        }
    }
}

/// Parse a raw LLM response `Message` into an `AgentTurn` carrying
/// `0..N` task-state mutations followed by exactly one action.
///
/// We accept the turn via OpenAI-style `tool_calls`, which the LLM
/// emits as an ordered array. Each call is classified by name:
///
/// - **Mutation pseudo-tools** (`push_subgoal`, `complete_subgoal`,
///   `set_watch_slot`, `clear_watch_slot`, `record_hypothesis`,
///   `refute_hypothesis`) parse into `TaskStateMutation` values
///   regardless of position. Malformed args produce a per-call warning
///   but never abort the turn — a single bad mutation cannot poison
///   the action.
/// - **Action pseudo-tools** (`agent_done`, `agent_replan`) and any
///   other tool name become an `AgentAction`. The first action-shaped
///   call wins; subsequent action calls are dropped, since exactly one
///   action runs per turn. Mutations after the action are still
///   preserved — apply order is enforced by `apply_mutations`, not by
///   tool-call order.
///
/// If only mutations are present (the LLM forgot to choose an action),
/// the result is an `AgentReplan` with a self-describing reason so the
/// next turn re-observes instead of aborting.
///
/// Text-only replies (no `tool_calls`) also map to
/// `AgentAction::AgentReplan` with the assistant's raw text as the
/// reason — matches the legacy "no tool call" recovery hook.
pub fn parse_agent_turn(message: &Message) -> anyhow::Result<AgentTurn> {
    use crate::agent::prompt::is_mutation_tool_name;

    if let Some(tool_calls) = message.tool_calls.as_ref()
        && !tool_calls.is_empty()
    {
        let mut mutations: Vec<TaskStateMutation> = Vec::new();
        let mut action: Option<AgentAction> = None;

        for tc in tool_calls {
            let name = tc.function.name.as_str();
            let args = &tc.function.arguments;

            if is_mutation_tool_name(name) {
                match parse_mutation_call(name, args) {
                    Ok(m) => mutations.push(m),
                    Err(reason) => tracing::warn!(
                        tool = name,
                        error = %reason,
                        "state-spine: dropping malformed mutation pseudo-tool call"
                    ),
                }
                continue;
            }

            // Action — keep only the first one; exactly one action runs per turn.
            if action.is_some() {
                tracing::warn!(
                    tool = name,
                    "state-spine: ignoring extra action call after first action was claimed"
                );
                continue;
            }

            action = Some(match name {
                "agent_done" => {
                    let summary = args
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("Goal completed")
                        .to_string();
                    AgentAction::AgentDone { summary }
                }
                "agent_replan" => {
                    let reason = args
                        .get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("Unknown reason")
                        .to_string();
                    AgentAction::AgentReplan { reason }
                }
                "invoke_skill" => {
                    let skill_id = args
                        .get("skill_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let version = args.get("version").and_then(Value::as_u64);
                    match (skill_id, version) {
                        (Some(skill_id), Some(version)) => match u32::try_from(version) {
                            Ok(version) => {
                                let parameters =
                                    args.get("parameters").cloned().unwrap_or(Value::Null);
                                AgentAction::InvokeSkill {
                                    skill_id,
                                    version,
                                    parameters,
                                }
                            }
                            Err(_) => {
                                tracing::warn!("state-spine: invoke_skill version out of range");
                                AgentAction::AgentReplan {
                                    reason: "invoke_skill version out of range".to_string(),
                                }
                            }
                        },
                        _ => {
                            tracing::warn!(
                                "state-spine: invoke_skill missing required fields — replanning"
                            );
                            AgentAction::AgentReplan {
                                reason: "invoke_skill missing required fields".to_string(),
                            }
                        }
                    }
                }
                _ => AgentAction::ToolCall {
                    tool_name: name.to_string(),
                    arguments: args.clone(),
                    tool_call_id: tc.id.clone(),
                },
            });
        }

        let action = action.unwrap_or_else(|| AgentAction::AgentReplan {
            reason: NO_ACTION_MUTATION_ONLY_REASON.to_string(),
        });

        return Ok(AgentTurn { mutations, action });
    }

    // Text-only response: treat as a replan request so the run re-observes
    // next turn instead of aborting. Mirrors the legacy "no tool call"
    // recovery hook.
    let reason = message
        .content_text()
        .map(str::to_owned)
        .unwrap_or_else(|| "LLM returned no tool call and no text".to_string());
    Ok(AgentTurn {
        mutations: Vec::new(),
        action: AgentAction::AgentReplan { reason },
    })
}

/// Parse a single mutation-shaped tool call (`push_subgoal`,
/// `complete_subgoal`, `set_watch_slot`, `clear_watch_slot`,
/// `record_hypothesis`, `refute_hypothesis`) into a `TaskStateMutation`.
///
/// Returns a human-readable reason on malformed arguments so the caller
/// can log per-call instead of aborting the whole turn. The strict
/// enforcement (e.g. "watch slot not set") happens later in
/// `TaskState::apply` and surfaces via `apply_mutations`'s warnings vec.
fn parse_mutation_call(name: &str, args: &Value) -> Result<TaskStateMutation, String> {
    use crate::agent::task_state::WatchSlotName;

    fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
        args.get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing required string field `{}`", key))
    }

    // Defer enum-tag validation to serde — `WatchSlotName` already
    // declares `#[serde(rename_all = "snake_case")]`, so the same
    // strings the pseudo-tool schema lists are accepted here without
    // a hand-maintained match arm.
    fn watch_slot_name(args: &Value) -> Result<WatchSlotName, String> {
        let raw = args
            .get("name")
            .ok_or_else(|| "missing required string field `name`".to_string())?;
        serde_json::from_value::<WatchSlotName>(raw.clone())
            .map_err(|e| format!("invalid watch slot name: {}", e))
    }

    match name {
        "push_subgoal" => Ok(TaskStateMutation::PushSubgoal {
            text: required_str(args, "text")?.to_string(),
        }),
        "complete_subgoal" => Ok(TaskStateMutation::CompleteSubgoal {
            summary: required_str(args, "summary")?.to_string(),
        }),
        "set_watch_slot" => Ok(TaskStateMutation::SetWatchSlot {
            name: watch_slot_name(args)?,
            note: required_str(args, "note")?.to_string(),
        }),
        "clear_watch_slot" => Ok(TaskStateMutation::ClearWatchSlot {
            name: watch_slot_name(args)?,
        }),
        "record_hypothesis" => Ok(TaskStateMutation::RecordHypothesis {
            text: required_str(args, "text")?.to_string(),
        }),
        "refute_hypothesis" => {
            let idx = args
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| "missing required non-negative integer field `index`".to_string())?;
            Ok(TaskStateMutation::RefuteHypothesis {
                index: idx as usize,
            })
        }
        _ => Err(format!("not a mutation pseudo-tool: `{}`", name)),
    }
}

/// Adapter that turns any `&dyn Mcp` into the `ToolExecutor` trait expected
/// by `run_turn`. Kept private to `runner.rs` — the plan names this
/// `McpToolExecutor` so later tasks can grep for the anchor.
struct McpToolExecutor<'a, M: Mcp + ?Sized> {
    mcp: &'a M,
}

#[async_trait::async_trait]
impl<M: Mcp + ?Sized> ToolExecutor for McpToolExecutor<'_, M> {
    async fn call_tool(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, String> {
        if tool_name == crate::agent::time_oracle::TOOL_NAME {
            return Ok(crate::agent::time_oracle::current_datetime_json());
        }

        let result = self
            .mcp
            .call_tool(tool_name, Some(arguments.clone()))
            .await
            .map_err(|e| e.to_string())?;
        let text = result
            .content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        if result.is_error == Some(true) {
            Err(text)
        } else {
            Ok(text)
        }
    }
}

impl StateRunner {
    fn start_skill_watcher_if_enabled(&mut self) {
        if !self.skill_ctx.enabled
            || !self.config.skills_enabled
            || self.skill_watcher_handle.is_some()
        {
            return;
        }

        let mut dirs = Vec::new();
        let mut stores = Vec::new();

        let project_dir = self.skill_ctx.project_skills_dir.clone();
        if let Err(err) = std::fs::create_dir_all(&project_dir) {
            warn!(
                ?project_dir,
                ?err,
                "skills: failed to create project skills dir for watcher"
            );
            return;
        }
        dirs.push(project_dir);
        stores.push(self.skill_store.clone());

        if let Some(global_dir) = self.skill_ctx.global_skills_dir.clone() {
            if let Err(err) = std::fs::create_dir_all(&global_dir) {
                warn!(
                    ?global_dir,
                    ?err,
                    "skills: failed to create global skills dir for watcher"
                );
            } else {
                dirs.push(global_dir.clone());
                stores.push(Arc::new(SkillStore::new(global_dir)));
            }
        }

        match crate::agent::skills::watcher::SkillWatcher::spawn(dirs) {
            Ok(watcher) => {
                self.skill_watcher_handle = Some(
                    crate::agent::skills::watcher_consumer::WatcherConsumer::spawn_watcher(
                        self.skill_index.clone(),
                        stores,
                        watcher,
                    ),
                );
            }
            Err(err) => {
                warn!(
                    ?err,
                    "skills: watcher failed to start; external edits will be picked up on next run"
                );
            }
        }
    }

    fn initialize_run_loop(
        &mut self,
        goal: &str,
        workflow: clickweave_core::Workflow,
        mcp_tools: &[Value],
        anchor_node_id: Option<uuid::Uuid>,
    ) -> RunLoopContext {
        // Reset the visible state tuple to match the freshly-provided
        // workflow. `AgentState::new(workflow)` wipes steps/terminal_reason
        // so the same `StateRunner` could in theory be reused across runs,
        // though `self` is consumed by the public run wrapper.
        self.state = AgentState::new(workflow);
        self.state.last_node_id = anchor_node_id;

        // Build the system prompt from the raw openai-shaped tool list.
        // `build_system_prompt` expects `clickweave_mcp::Tool`; the raw
        // `Vec<Value>` is already openai-shape, so extract the minimum
        // fields each tool entry carries.
        //
        // D18: the system prompt is stable across runs. Variant context +
        // prior-turn log are pre-composed into `goal` at the caller seam, so
        // they land in `messages[1]`, preserving the `messages[0]` cache prefix.
        let tool_list_for_prompt = openai_tools_to_mcp_tool_list(mcp_tools);
        let system_text = if let Some(prompt) = self.agent_system_prompt_override.as_deref() {
            build_system_prompt_with_header(prompt, &tool_list_for_prompt)
        } else {
            build_system_prompt(&tool_list_for_prompt)
        };

        let advertised_tool_names: Vec<String> = tool_list_for_prompt
            .iter()
            .map(|t| t.name.clone())
            .collect();

        let initial_scope = self.compute_tools_in_scope(&advertised_tool_names);
        let initial_user = build_user_turn_message_from_input(UserTurnMessageInput {
            wm: &self.world_model,
            ts: &self.task_state,
            current_step: 0,
            observation_text: goal,
            retrieved: &[],
            applicable_skills: &[],
            tools_in_scope_names: &initial_scope,
            max_elements: self.config.state_block_max_elements,
        });

        RunLoopContext {
            messages: vec![Message::system(system_text), Message::user(initial_user)],
            tools: mcp_tools
                .iter()
                .cloned()
                .chain(crate::agent::prompt::pseudo_tools())
                .collect(),
            advertised_tool_names,
            annotations_by_tool: build_annotations_index(mcp_tools),
            budget: CompactBudget {
                recent_n: self.config.recent_n,
                ..CompactBudget::default()
            },
        }
    }

    async fn observe_for_next_turn<M>(
        &mut self,
        mcp: &M,
    ) -> (
        Vec<clickweave_core::cdp::CdpFindElementMatch>,
        Vec<crate::agent::episodic::RetrievedEpisode>,
    )
    where
        M: Mcp + ?Sized,
    {
        // Capture the pre-mirror world-model signatures so the
        // `WorldModelChanged` diff emitted by `run_turn` sees the
        // direct-observation writes below. Only seed the baseline when it is
        // empty: early-exit branches skip `run_turn`, so the baseline must
        // persist across iterations until `run_turn.take()` consumes it.
        if self.turn_pre_signatures.is_none() {
            self.turn_pre_signatures = Some(self.world_model.field_signatures());
        }
        // Spec 3: snapshot the world model before this iteration's dispatch
        // so successful tool calls record the state the LLM actually saw.
        self.pre_dispatch_snapshot = Some(
            crate::agent::step_record::WorldModelSnapshot::from_world_model(&self.world_model),
        );

        let CdpPageObservation {
            page_url,
            page_fingerprint,
            inventory,
        } = self.fetch_cdp_page_summary(mcp).await;
        self.mirror_cdp_page_summary(page_url, page_fingerprint, inventory);

        let elements = self.current_cdp_elements();
        let prev_phase_at_top = self.task_state.phase;
        self.queue_snapshot_stale_if_aged();
        self.observe();
        if prev_phase_at_top != self.task_state.phase {
            self.emit_event(AgentEvent::TaskStateChanged {
                run_id: self.run_id,
                task_state: self.task_state.clone(),
            })
            .await;
        }

        let retrieved = self.try_retrieve_episodic(prev_phase_at_top).await;
        (elements, retrieved)
    }

    fn mirror_cdp_page_summary(
        &mut self,
        page_url: String,
        page_fingerprint: String,
        inventory: Vec<CdpElementInventorySummary>,
    ) {
        use crate::agent::world_model::{CdpPageState, Fresh, FreshnessSource, ObservedElement};

        if matches!(
            self.world_model
                .elements
                .as_ref()
                .and_then(|f| f.value.first()),
            Some(ObservedElement::Cdp(_))
        ) {
            self.world_model.elements = None;
        }

        let url = if page_url.is_empty() {
            self.state.current_url.clone()
        } else {
            page_url
        };
        if !url.is_empty() {
            self.world_model.cdp_page = Some(Fresh {
                value: CdpPageState {
                    url,
                    page_fingerprint,
                    element_inventory: inventory,
                },
                written_at: self.step_index,
                source: FreshnessSource::DirectObservation,
                ttl_steps: Some(2),
            });
        } else {
            self.world_model.cdp_page = None;
        }
    }

    fn current_cdp_elements(&self) -> Vec<clickweave_core::cdp::CdpFindElementMatch> {
        self.world_model
            .elements
            .as_ref()
            .map(|fresh| {
                fresh
                    .value
                    .iter()
                    .filter_map(|element| match element {
                        crate::agent::world_model::ObservedElement::Cdp(match_) => {
                            Some(match_.clone())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn prepare_turn_for_dispatch(
        &mut self,
        turn: &mut AgentTurn,
        last_action: &mut Option<LastActionProgress>,
        recent_actions: &mut VecDeque<ActionProgressSignature>,
    ) {
        let outer_milestones_before = self.task_state.milestones.len();
        if !turn.mutations.is_empty() {
            let warnings = self.apply_mutations(&turn.mutations);
            for w in warnings {
                tracing::warn!(warning = %w, "state-spine: mutation warning");
            }
            self.emit_event(AgentEvent::TaskStateChanged {
                run_id: self.run_id,
                task_state: self.task_state.clone(),
            })
            .await;
        }
        let outer_milestones_appended = self
            .task_state
            .milestones
            .len()
            .saturating_sub(outer_milestones_before);

        if outer_milestones_appended > 0 {
            self.write_subgoal_completed_records(outer_milestones_appended, turn)
                .await;
            reset_no_progress_tracking(last_action, recent_actions);
        }

        self.retrieve_skills_for_pushed_subgoals();

        if let AgentAction::InvokeSkill {
            skill_id,
            version,
            parameters,
        } = turn.action.clone()
        {
            turn.action = match self.dispatch_skill(&skill_id, version, parameters).await {
                Ok(frame) => Self::skill_frame_to_single_step_action(&frame),
                Err(reason) => AgentAction::AgentReplan { reason },
            };
        }
    }

    fn retrieve_skills_for_pushed_subgoals(&mut self) {
        if !self.skill_ctx.enabled
            || !self.config.skills_enabled
            || self.last_pushed_subgoal_ids.is_empty()
        {
            return;
        }

        let pushed = std::mem::take(&mut self.last_pushed_subgoal_ids);
        let k = self.config.applicable_skills_k;
        for id in &pushed {
            let Some(subgoal) = self
                .task_state
                .subgoal_stack
                .iter()
                .find(|s| s.id == *id)
                .cloned()
            else {
                continue;
            };
            let subgoal_sig = crate::agent::skills::signature::compute_subgoal_signature(
                &subgoal.text,
                &self.world_model,
            );
            let app_sig =
                crate::agent::skills::signature::compute_applicability_signature(&self.world_model);
            let candidates = self.skill_index.read().lookup_at(
                &subgoal_sig,
                &app_sig,
                &subgoal.text,
                k,
                chrono::Utc::now(),
            );
            self.pending_applicable_skills.extend(candidates);
        }
    }

    fn push_tool_step(
        &mut self,
        elements: &[CdpFindElementMatch],
        tool_name: &str,
        arguments: &Value,
        tool_call_id: &str,
        outcome: StepOutcome,
    ) -> usize {
        let step_idx = self.state.steps.len();
        self.state.steps.push(AgentStep {
            index: step_idx,
            elements: elements.to_vec(),
            command: AgentCommand::ToolCall {
                tool_name: tool_name.to_string(),
                arguments: arguments.clone(),
                tool_call_id: tool_call_id.to_string(),
            },
            outcome,
            page_url: self.state.current_url.clone(),
        });
        self.advance_recorded_step_index();
        step_idx
    }

    fn clear_success_dispatch_state(&mut self, trackers: &mut RunLoopTrackers) {
        self.state.consecutive_errors = 0;
        self.consecutive_errors = 0;
        trackers.last_failure = None;
        self.clear_last_failure_tracking();
    }

    async fn finish_synthetic_success(
        &mut self,
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        step_idx: usize,
        tool_name: &str,
        arguments: &Value,
        tool_call_id: &str,
        body: &str,
    ) {
        trackers.previous_result = Some(body.to_string());
        if let Some(nudge) = self
            .track_repeat_action(
                tool_name,
                arguments,
                body,
                &loop_ctx.annotations_by_tool,
                &mut trackers.last_action,
                &mut trackers.recent_actions,
            )
            .await
        {
            trackers.previous_result = Some(nudge);
        }
        self.emit_event(AgentEvent::StepCompleted {
            step_index: step_idx,
            tool_name: tool_name.to_string(),
            summary: crate::agent::prompt::truncate_summary(body, 120),
        })
        .await;
        append_assistant_and_tool_result(
            &mut loop_ctx.messages,
            tool_name,
            arguments,
            tool_call_id,
            trackers.previous_result.as_deref(),
        );
    }

    async fn handle_no_focus_launch_skip<M>(
        &mut self,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        mcp: &M,
    ) -> bool
    where
        M: Mcp + ?Sized,
    {
        let AgentAction::ToolCall {
            tool_name,
            arguments,
            tool_call_id,
        } = &turn.action
        else {
            return false;
        };
        if tool_name != "launch_app" {
            return false;
        }
        let Some(running) = self.running_app_for_no_focus_launch(arguments, mcp).await else {
            return false;
        };

        self.emit_event(AgentEvent::SubAction {
            tool_name: "launch_app".to_string(),
            summary: "skipped: app already running; focus changes disabled".to_string(),
        })
        .await;
        let skip_body = Self::skipped_launch_result_text(&running);
        debug!(
            tool = "launch_app",
            app = running.name,
            "state-spine: suppressing launch_app for already-running app",
        );
        let step_idx = self.push_tool_step(
            elements,
            tool_name,
            arguments,
            tool_call_id,
            StepOutcome::Success(skip_body.clone()),
        );
        self.emit_world_model_changed_for_recorded_step().await;
        self.clear_success_dispatch_state(trackers);
        self.maybe_cdp_connect(tool_name, arguments, &skip_body, mcp)
            .await;
        self.finish_synthetic_success(
            loop_ctx,
            trackers,
            step_idx,
            tool_name,
            arguments,
            tool_call_id,
            &skip_body,
        )
        .await;
        true
    }

    async fn handle_synthetic_focus_skip<M>(
        &mut self,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        mcp: &M,
    ) -> bool
    where
        M: Mcp + ?Sized,
    {
        let AgentAction::ToolCall {
            tool_name,
            arguments,
            tool_call_id,
        } = &turn.action
        else {
            return false;
        };
        if tool_name != "focus_window" {
            return false;
        }
        let Some(reason) = self.should_skip_focus_window(arguments, mcp) else {
            return false;
        };

        self.emit_event(AgentEvent::SubAction {
            tool_name: "focus_window".to_string(),
            summary: reason.sub_action_summary().to_string(),
        })
        .await;
        let skip_body = reason.llm_message().to_string();
        debug!(
            tool = "focus_window",
            app = arguments
                .get("app_name")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            reason = skip_body,
            "state-spine: suppressing focus_window",
        );
        let step_idx = self.push_tool_step(
            elements,
            tool_name,
            arguments,
            tool_call_id,
            StepOutcome::Success(skip_body.clone()),
        );
        self.emit_world_model_changed_for_recorded_step().await;
        self.clear_success_dispatch_state(trackers);

        if let Some((app_name, kind_hint)) =
            self.cdp_target_for_skipped_focus_window(reason, arguments, mcp)
            && let Some(cdp_port) = self
                .auto_connect_cdp(&app_name, kind_hint.as_deref(), mcp)
                .await
        {
            self.finalize_cdp_connected(&app_name, cdp_port, mcp).await;
        }

        self.finish_synthetic_success(
            loop_ctx,
            trackers,
            step_idx,
            tool_name,
            arguments,
            tool_call_id,
            &skip_body,
        )
        .await;
        true
    }

    async fn record_blocked_tool_error(
        &mut self,
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        elements: &[CdpFindElementMatch],
        tool_name: &str,
        arguments: &Value,
        tool_call_id: &str,
        err_msg: String,
        sub_action_summary: &str,
        record_policy_deny: bool,
    ) -> LoopStepFlow {
        self.emit_event(AgentEvent::SubAction {
            tool_name: tool_name.to_string(),
            summary: sub_action_summary.to_string(),
        })
        .await;
        let step_idx = self.push_tool_step(
            elements,
            tool_name,
            arguments,
            tool_call_id,
            StepOutcome::Error(err_msg.clone()),
        );
        self.emit_world_model_changed_for_recorded_step().await;
        if record_policy_deny {
            self.record_policy_deny_failure(tool_name);
        }
        self.state.consecutive_errors += 1;
        self.consecutive_errors = self.state.consecutive_errors;
        trackers.previous_result = Some(err_msg.clone());
        append_assistant_and_tool_result(
            &mut loop_ctx.messages,
            tool_name,
            arguments,
            tool_call_id,
            trackers.previous_result.as_deref(),
        );
        self.emit_event(AgentEvent::StepFailed {
            step_index: step_idx,
            tool_name: tool_name.to_string(),
            error: err_msg.clone(),
        })
        .await;

        let looped = matches!(
            trackers.last_failure.as_ref(),
            Some((prev_tool, prev_args, prev_err))
                if prev_tool == tool_name && prev_args == arguments && prev_err == &err_msg
        );
        if looped {
            warn!(
                tool = %tool_name,
                "state-spine: identical blocked tool call repeated — aborting"
            );
            self.state.terminal_reason = Some(TerminalReason::LoopDetected {
                tool_name: tool_name.to_string(),
                error: err_msg,
            });
            return LoopStepFlow::Break;
        }
        trackers.last_failure = Some((tool_name.to_string(), arguments.clone(), err_msg));

        let action = recovery_strategy(
            self.state.consecutive_errors,
            self.config.max_consecutive_errors,
        );
        if matches!(action, RecoveryAction::Abort) {
            warn!(
                errors = self.state.consecutive_errors,
                "state-spine: too many consecutive blocked tool calls — aborting"
            );
            self.state.terminal_reason = Some(TerminalReason::MaxErrorsReached {
                consecutive_errors: self.state.consecutive_errors,
            });
            return LoopStepFlow::Break;
        }
        reset_no_progress_tracking(&mut trackers.last_action, &mut trackers.recent_actions);
        LoopStepFlow::Continue
    }

    async fn guard_runtime_managed_cdp(
        &mut self,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
    ) -> LoopStepFlow {
        let AgentAction::ToolCall {
            tool_name,
            arguments,
            tool_call_id,
        } = &turn.action
        else {
            return LoopStepFlow::Dispatch;
        };
        let Some(err_msg) = Self::raw_cdp_lifecycle_blocked(tool_name, arguments) else {
            return LoopStepFlow::Dispatch;
        };
        warn!(
            tool = %tool_name,
            "state-spine: raw CDP lifecycle tool blocked"
        );
        self.record_blocked_tool_error(
            loop_ctx,
            trackers,
            elements,
            tool_name,
            arguments,
            tool_call_id,
            err_msg,
            "blocked: CDP lifecycle is runtime-managed",
            false,
        )
        .await
    }

    async fn guard_coordinate_primitive<M>(
        &mut self,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        mcp: &M,
    ) -> LoopStepFlow
    where
        M: Mcp + ?Sized,
    {
        let AgentAction::ToolCall {
            tool_name,
            arguments,
            tool_call_id,
        } = &turn.action
        else {
            return LoopStepFlow::Dispatch;
        };
        let Some(err_msg) = self.coordinate_primitive_blocked(tool_name, mcp) else {
            return LoopStepFlow::Dispatch;
        };
        warn!(
            tool = %tool_name,
            "state-spine: coordinate primitive blocked by structured-surface guard"
        );
        self.record_blocked_tool_error(
            loop_ctx,
            trackers,
            elements,
            tool_name,
            arguments,
            tool_call_id,
            err_msg,
            "blocked: structured surface wired (CDP/AX)",
            false,
        )
        .await
    }

    async fn handle_permission_gate(
        &mut self,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
    ) -> LoopStepFlow {
        let AgentAction::ToolCall {
            tool_name,
            arguments,
            tool_call_id,
        } = &turn.action
        else {
            return LoopStepFlow::Dispatch;
        };
        if is_observation_tool(tool_name, &loop_ctx.annotations_by_tool) {
            return LoopStepFlow::Dispatch;
        }

        match self.policy_for(tool_name, arguments, &loop_ctx.annotations_by_tool) {
            PermissionAction::Deny => {
                warn!(tool = %tool_name, "state-spine: tool denied by permission policy");
                self.record_blocked_tool_error(
                    loop_ctx,
                    trackers,
                    elements,
                    tool_name,
                    arguments,
                    tool_call_id,
                    format!("Tool `{}` denied by permission policy", tool_name),
                    "blocked: permission policy denied tool",
                    true,
                )
                .await
            }
            PermissionAction::Allow => {
                debug!(
                    tool = %tool_name,
                    "state-spine: permission policy allowed tool — skipping approval"
                );
                LoopStepFlow::Dispatch
            }
            PermissionAction::Ask => {
                match self
                    .request_approval(tool_name, arguments, self.state.steps.len(), "")
                    .await
                {
                    Some(ApprovalResult::Rejected) => {
                        self.record_approval_rejection(
                            loop_ctx,
                            trackers,
                            elements,
                            tool_name,
                            arguments,
                            tool_call_id,
                        )
                        .await;
                        LoopStepFlow::Continue
                    }
                    Some(ApprovalResult::Unavailable) => {
                        warn!("state-spine: approval system unavailable — terminating");
                        self.state.terminal_reason = Some(TerminalReason::ApprovalUnavailable);
                        LoopStepFlow::Break
                    }
                    Some(ApprovalResult::Approved) | None => LoopStepFlow::Dispatch,
                }
            }
        }
    }

    async fn record_approval_rejection(
        &mut self,
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        elements: &[CdpFindElementMatch],
        tool_name: &str,
        arguments: &Value,
        tool_call_id: &str,
    ) {
        let step_idx = self.push_tool_step(
            elements,
            tool_name,
            arguments,
            tool_call_id,
            StepOutcome::Replan("User rejected action".to_string()),
        );
        self.emit_world_model_changed_for_recorded_step().await;
        trackers.previous_result = Some("Replan: user rejected action".to_string());
        append_assistant_and_tool_result(
            &mut loop_ctx.messages,
            tool_name,
            arguments,
            tool_call_id,
            trackers.previous_result.as_deref(),
        );
        let _ = step_idx;
        reset_no_progress_tracking(&mut trackers.last_action, &mut trackers.recent_actions);
    }

    async fn handle_run_turn_result<M>(
        &mut self,
        goal: &str,
        mcp: &M,
        mcp_tools: &[Value],
        loop_ctx: &mut RunLoopContext,
        trackers: &mut RunLoopTrackers,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        previous_errors: usize,
        outcome: TurnOutcome,
    ) -> LoopStepFlow
    where
        M: Mcp + ?Sized,
    {
        self.record_episodic_progress(turn, &outcome);
        self.queue_recovery_success_write(turn, &outcome, previous_errors)
            .await;

        let flow = match outcome {
            TurnOutcome::ToolSuccess {
                tool_name,
                tool_body,
            } => {
                self.handle_tool_success_outcome(
                    mcp, mcp_tools, loop_ctx, trackers, turn, elements, tool_name, tool_body,
                )
                .await
            }
            TurnOutcome::ToolError { tool_name, error } => {
                self.handle_tool_error_outcome(trackers, turn, elements, tool_name, error)
                    .await
            }
            TurnOutcome::Done { summary } => self.handle_done_outcome(goal, mcp, summary).await,
            TurnOutcome::Replan { reason } => {
                trackers.previous_result = Some(format!("replan: {}", reason));
                reset_no_progress_tracking(&mut trackers.last_action, &mut trackers.recent_actions);
                LoopStepFlow::Continue
            }
        };

        if matches!(flow, LoopStepFlow::Continue) {
            self.append_action_result_to_history(loop_ctx, trackers, &turn.action);
        }
        flow
    }

    fn record_episodic_progress(&mut self, turn: &AgentTurn, outcome: &TurnOutcome) {
        if !self.episodic_active() {
            return;
        }
        match outcome {
            TurnOutcome::ToolError { tool_name, error } => {
                self.last_failed_tool_name = Some(tool_name.clone());
                self.last_failed_error_kind = Some(error.clone());
            }
            TurnOutcome::ToolSuccess { .. } => {
                self.clear_last_failure_tracking();
            }
            _ => {}
        }
        if self.task_state.phase == crate::agent::phase::Phase::Recovering
            && let AgentAction::ToolCall {
                tool_name,
                arguments,
                ..
            } = &turn.action
        {
            let outcome_kind = match outcome {
                TurnOutcome::ToolSuccess { .. } => "ok",
                TurnOutcome::ToolError { .. } => "error",
                TurnOutcome::Done { .. } => "done",
                TurnOutcome::Replan { .. } => "replan",
            };
            self.recovery_actions_accumulator
                .push(crate::agent::episodic::types::CompactAction {
                    tool_name: tool_name.clone(),
                    brief_args: brief_summarize_args(arguments),
                    outcome_kind: outcome_kind.to_string(),
                });
        }
    }

    async fn queue_recovery_success_write(
        &mut self,
        turn: &AgentTurn,
        outcome: &TurnOutcome,
        previous_errors: usize,
    ) {
        if previous_errors == 0
            || self.consecutive_errors != 0
            || !matches!(outcome, TurnOutcome::ToolSuccess { .. })
        {
            return;
        }

        self.write_recovery_succeeded_record(turn, outcome).await;
        if !self.episodic_active() {
            return;
        }
        let Some(entry) = self.recovering_snapshot.take() else {
            return;
        };
        let Some(writer) = &self.episodic_writer else {
            return;
        };
        let actions = std::mem::take(&mut self.recovery_actions_accumulator);
        let record = self.build_step_record(
            crate::agent::step_record::BoundaryKind::RecoverySucceeded,
            serde_json::to_value(&turn.action).unwrap_or_else(|_| serde_json::json!({})),
            serde_json::json!({"kind": "tool_success"}),
        );
        let queue_result = writer
            .queue(
                crate::agent::episodic::types::WriteRequest::DeriveAndInsert {
                    entry: Box::new(entry),
                    recovery_success: Box::new(record),
                    recovery_actions: actions,
                },
            )
            .await;
        if let Err(e) = queue_result {
            self.emit_event(AgentEvent::Warning {
                message: format!("episodic: write dropped: backpressure ({e})"),
            })
            .await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_tool_success_outcome<M>(
        &mut self,
        mcp: &M,
        mcp_tools: &[Value],
        loop_ctx: &RunLoopContext,
        trackers: &mut RunLoopTrackers,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        tool_name: String,
        tool_body: String,
    ) -> LoopStepFlow
    where
        M: Mcp + ?Sized,
    {
        let AgentAction::ToolCall {
            arguments,
            tool_call_id,
            ..
        } = &turn.action
        else {
            unreachable!("ToolSuccess outcome implies ToolCall action");
        };

        let step_idx = self.push_tool_step(
            elements,
            &tool_name,
            arguments,
            tool_call_id,
            StepOutcome::Success(tool_body.clone()),
        );
        let unverified_side_effect =
            is_unverified_side_effect_action(&tool_name, arguments, &loop_ctx.annotations_by_tool);
        self.recorded_steps.push(RecordedStep {
            tool_name: tool_name.clone(),
            arguments: arguments.clone(),
            result_text: tool_body.clone(),
            world_model_pre: self.pre_dispatch_snapshot.take().unwrap_or_else(|| {
                crate::agent::step_record::WorldModelSnapshot::from_world_model(&self.world_model)
            }),
            world_model_post: crate::agent::step_record::WorldModelSnapshot::from_world_model(
                &self.world_model,
            ),
        });
        let unverified_side_effect_nudge = if unverified_side_effect {
            Some(build_unverified_side_effect_nudge(&tool_body))
        } else {
            None
        };
        trackers.previous_result = Some(
            unverified_side_effect_nudge
                .clone()
                .unwrap_or(tool_body.clone()),
        );
        trackers.last_failure = None;

        self.emit_event(AgentEvent::StepCompleted {
            step_index: step_idx,
            tool_name: tool_name.clone(),
            summary: crate::agent::prompt::truncate_summary(&tool_body, 120),
        })
        .await;
        if unverified_side_effect {
            self.emit_event(AgentEvent::Warning {
                message: format!(
                    "{}: `{}` result requires verification before completion",
                    UNVERIFIED_SIDE_EFFECT_PREFIX, tool_name
                ),
            })
            .await;
        }
        if matches!(
            self.maybe_halt_on_destructive_cap(&tool_name, &loop_ctx.annotations_by_tool),
            CapStatus::CapReached
        ) {
            self.emit_destructive_cap_hit().await;
            return LoopStepFlow::Break;
        }

        if let Some(node_id) = self
            .add_workflow_node(
                &tool_name,
                arguments,
                mcp_tools,
                &loop_ctx.annotations_by_tool,
            )
            .await
        {
            self.record_produced_node_id(node_id);
        }
        self.maybe_cdp_connect(&tool_name, arguments, &tool_body, mcp)
            .await;

        if let Some(nudge) = self
            .track_post_text_submit_search(
                &tool_name,
                arguments,
                &tool_body,
                &mut trackers.pending_text_submit_search,
            )
            .await
        {
            trackers.previous_result = Some(combine_with_side_effect_nudge(
                unverified_side_effect_nudge.as_deref(),
                nudge,
            ));
        }
        if let Some(nudge) = self
            .track_repeat_action(
                &tool_name,
                arguments,
                &tool_body,
                &loop_ctx.annotations_by_tool,
                &mut trackers.last_action,
                &mut trackers.recent_actions,
            )
            .await
        {
            trackers.previous_result = Some(combine_with_side_effect_nudge(
                unverified_side_effect_nudge.as_deref(),
                nudge,
            ));
        }
        LoopStepFlow::Continue
    }

    async fn handle_tool_error_outcome(
        &mut self,
        trackers: &mut RunLoopTrackers,
        turn: &AgentTurn,
        elements: &[CdpFindElementMatch],
        tool_name: String,
        error: String,
    ) -> LoopStepFlow {
        let AgentAction::ToolCall {
            arguments,
            tool_call_id,
            ..
        } = &turn.action
        else {
            unreachable!("ToolError outcome implies ToolCall action");
        };
        let step_idx = self.push_tool_step(
            elements,
            &tool_name,
            arguments,
            tool_call_id,
            StepOutcome::Error(error.clone()),
        );
        self.state.consecutive_errors = self.consecutive_errors;
        trackers.previous_result = Some(error.clone());
        self.emit_event(AgentEvent::StepFailed {
            step_index: step_idx,
            tool_name: tool_name.clone(),
            error: error.clone(),
        })
        .await;

        let looped = matches!(
            trackers.last_failure.as_ref(),
            Some((prev_tool, prev_args, prev_err))
                if prev_tool == &tool_name && prev_args == arguments && prev_err == &error
        );
        if looped {
            warn!(
                tool = %tool_name,
                error = %error,
                "state-spine: identical failing tool call repeated — aborting"
            );
            self.state.terminal_reason = Some(TerminalReason::LoopDetected { tool_name, error });
            return LoopStepFlow::Break;
        }
        trackers.last_failure = Some((tool_name, arguments.clone(), error));
        reset_no_progress_tracking(&mut trackers.last_action, &mut trackers.recent_actions);

        let action = recovery_strategy(
            self.state.consecutive_errors,
            self.config.max_consecutive_errors,
        );
        if matches!(action, RecoveryAction::Abort) {
            warn!(
                errors = self.state.consecutive_errors,
                "state-spine: too many consecutive errors — aborting"
            );
            self.state.terminal_reason = Some(TerminalReason::MaxErrorsReached {
                consecutive_errors: self.state.consecutive_errors,
            });
            return LoopStepFlow::Break;
        }
        LoopStepFlow::Continue
    }

    async fn handle_done_outcome<M>(&mut self, goal: &str, mcp: &M, summary: String) -> LoopStepFlow
    where
        M: Mcp + ?Sized,
    {
        let disagreement = self.verify_completion(goal, &summary, mcp).await;
        if let Some((screenshot_b64, vlm_reasoning)) = disagreement {
            warn!("state-spine: VLM disagreed with agent_done — halting for user review");
            self.emit_event(AgentEvent::CompletionDisagreement {
                screenshot_b64,
                vlm_reasoning: vlm_reasoning.clone(),
                agent_summary: summary.clone(),
            })
            .await;
            self.state.terminal_reason = Some(TerminalReason::CompletionDisagreement {
                agent_summary: summary,
                vlm_reasoning,
            });
            return LoopStepFlow::Break;
        }

        self.state.completed = true;
        self.state.summary = Some(summary.clone());
        self.state.terminal_reason = Some(TerminalReason::Completed {
            summary: summary.clone(),
        });
        self.emit_event(AgentEvent::GoalComplete { summary }).await;
        LoopStepFlow::Break
    }

    fn append_action_result_to_history(
        &mut self,
        loop_ctx: &mut RunLoopContext,
        trackers: &RunLoopTrackers,
        action: &AgentAction,
    ) {
        match action {
            AgentAction::ToolCall {
                tool_name,
                arguments,
                tool_call_id,
            } => {
                append_assistant_and_tool_result(
                    &mut loop_ctx.messages,
                    tool_name,
                    arguments,
                    tool_call_id,
                    trackers.previous_result.as_deref(),
                );
            }
            AgentAction::AgentReplan { reason } => {
                loop_ctx
                    .messages
                    .push(Message::assistant(format!("replan: {}", reason)));
            }
            AgentAction::AgentDone { .. } | AgentAction::InvokeSkill { .. } => {}
        }
    }

    /// Top-level observe → compose → LLM → parse → apply → dispatch →
    /// compact control loop. Task 3a.1 ships the minimum skeleton; later
    /// tasks (flagged by `TODO(task-3a.N)` markers inline) wire VLM
    /// verification, approval, loop detection,
    /// consecutive-destructive cap, workflow-graph emission, CDP
    /// auto-connect, synthetic `focus_window` skip, recovery strategy,
    /// and boundary `StepRecord` writes.
    ///
    /// Crate-private because the `Mcp` trait is `pub(crate)`; the public
    /// entry point stays [`crate::agent::run_agent_workflow`].
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run<B, M>(
        mut self,
        llm: &B,
        mcp: &M,
        goal: String,
        workflow: clickweave_core::Workflow,
        mcp_tools: Vec<Value>,
        anchor_node_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<AgentState>
    where
        B: ChatBackend + ?Sized,
        M: Mcp + ?Sized,
    {
        self.start_skill_watcher_if_enabled();
        // Drain queued episodic writes on *every* exit path,
        // including the early `?` returns from chat/parse failures.
        // Without this, a recovery write queued moments before an LLM
        // failure would race the Tauri-side cleanup and never commit
        // before the writer is dropped, defeating the run-terminal
        // promotion barrier the post-loop flush already installs.
        let inner = Self::run_inner(
            &mut self,
            llm,
            mcp,
            goal,
            workflow,
            mcp_tools,
            anchor_node_id,
        );
        let result = inner.await;
        if let Some(writer) = &self.episodic_writer {
            writer.flush().await;
        }
        // Spec 3: clear the per-run scratch state so the runner could
        // in theory be reused. Files (the on-disk skill store) outlive
        // the runner — only the in-memory accumulators are dropped here.
        self.recorded_steps.clear();
        self.push_idx_stack.clear();
        self.push_signature_stack.clear();
        self.last_pushed_subgoal_ids.clear();
        self.completed_subgoal_extraction_queue.clear();
        self.produced_node_ids_stack.clear();
        self.pending_applicable_skills.clear();
        self.pre_dispatch_snapshot = None;
        if let Some(handle) = self.skill_watcher_handle.take() {
            handle.abort();
        }
        match result {
            Ok(()) => Ok(self.state),
            Err(e) => Err(e),
        }
    }

    async fn run_inner<B, M>(
        &mut self,
        llm: &B,
        mcp: &M,
        goal: String,
        workflow: clickweave_core::Workflow,
        mcp_tools: Vec<Value>,
        anchor_node_id: Option<uuid::Uuid>,
    ) -> anyhow::Result<()>
    where
        B: ChatBackend + ?Sized,
        M: Mcp + ?Sized,
    {
        let mut loop_ctx = self.initialize_run_loop(&goal, workflow, &mcp_tools, anchor_node_id);
        let mut trackers = RunLoopTrackers::default();

        for _step_index in 0..self.config.max_steps {
            if self.state.completed {
                break;
            }

            // 1. Observe — refresh the compact CDP page summary, drain
            // invalidations, re-infer phase, and run episodic retrieval if
            // this iteration hits a retrieval trigger.
            let (elements, retrieved) = self.observe_for_next_turn(mcp).await;

            // 2. Compose the per-turn user message with the state block +
            // the previous tool body as the observation, then compact the
            // history before the LLM call.
            let step_obs = trackers.previous_result.clone().unwrap_or_default();
            let step_scope = self.compute_tools_in_scope(&loop_ctx.advertised_tool_names);
            // Spec 3: drain `pending_applicable_skills` once per turn —
            // the block surfaces in the next user turn after the
            // `push_subgoal` that produced it, then disappears.
            let applicable = std::mem::take(&mut self.pending_applicable_skills);
            let step_msg = build_user_turn_message_from_input(UserTurnMessageInput {
                wm: &self.world_model,
                ts: &self.task_state,
                current_step: self.step_index,
                observation_text: &step_obs,
                retrieved: &retrieved,
                applicable_skills: &applicable,
                tools_in_scope_names: &step_scope,
                max_elements: self.config.state_block_max_elements,
            });
            loop_ctx.messages.push(Message::user(step_msg));
            loop_ctx.messages = compact(loop_ctx.messages, &loop_ctx.budget);

            // 3. LLM call.
            let response = llm
                .chat(&loop_ctx.messages, Some(&loop_ctx.tools))
                .await
                .context("Agent LLM call failed")?;
            let choice = response
                .choices
                .into_iter()
                .next()
                .context("No choices in LLM response")?;

            // 4. Parse the LLM response into an AgentTurn carrying any
            //    `0..N` task-state mutations followed by exactly one
            //    action.
            let mut turn = parse_agent_turn(&choice.message)?;
            if guard_completion_after_unverified_side_effect(
                trackers.previous_result.as_deref(),
                &mut turn,
            ) {
                warn!("state-spine: blocked completion after unverified side-effectful action");
                self.emit_event(AgentEvent::Warning {
                    message: UNVERIFIED_SIDE_EFFECT_COMPLETION_BLOCKED_REASON.to_string(),
                })
                .await;
            }

            self.prepare_turn_for_dispatch(
                &mut turn,
                &mut trackers.last_action,
                &mut trackers.recent_actions,
            )
            .await;

            if self
                .handle_no_focus_launch_skip(&turn, &elements, &mut loop_ctx, &mut trackers, mcp)
                .await
            {
                continue;
            }

            force_background_launch_app(&mut turn.action, self.config.allow_focus_window);

            if self
                .handle_synthetic_focus_skip(&turn, &elements, &mut loop_ctx, &mut trackers, mcp)
                .await
            {
                continue;
            }

            match self
                .guard_runtime_managed_cdp(&turn, &elements, &mut loop_ctx, &mut trackers)
                .await
            {
                LoopStepFlow::Continue => continue,
                LoopStepFlow::Break => break,
                LoopStepFlow::Dispatch => {}
            }

            match self
                .guard_coordinate_primitive(&turn, &elements, &mut loop_ctx, &mut trackers, mcp)
                .await
            {
                LoopStepFlow::Continue => continue,
                LoopStepFlow::Break => break,
                LoopStepFlow::Dispatch => {}
            }

            match self
                .handle_permission_gate(&turn, &elements, &mut loop_ctx, &mut trackers)
                .await
            {
                LoopStepFlow::Continue => continue,
                LoopStepFlow::Break => break,
                LoopStepFlow::Dispatch => {}
            }

            // 5. Dispatch the action via run_turn. Mutations were
            //    already applied at step 4' above, so we forward an
            //    action-only turn — `run_turn`'s internal
            //    `apply_mutations` call becomes a no-op on the empty
            //    vec and `TaskStateChanged` is not emitted twice.
            //
            //    `previous_errors` captures the error counter from the
            //    iteration just before the new turn; a drop from >0 to
            //    0 after `run_turn` signals the
            //    `Recovering -> Executing` transition persisted as a
            //    `BoundaryKind::RecoverySucceeded` record.
            let previous_errors = self.consecutive_errors;
            let executor = McpToolExecutor { mcp };
            let action_only_turn = AgentTurn {
                mutations: Vec::new(),
                action: turn.action.clone(),
            };
            let (outcome, warnings, _run_turn_milestones) =
                self.run_turn(&action_only_turn, &executor).await;
            for w in warnings {
                tracing::warn!(warning = %w, "state-spine: mutation warning");
            }

            match self
                .handle_run_turn_result(
                    &goal,
                    mcp,
                    &mcp_tools,
                    &mut loop_ctx,
                    &mut trackers,
                    &turn,
                    &elements,
                    previous_errors,
                    outcome,
                )
                .await
            {
                LoopStepFlow::Break => break,
                LoopStepFlow::Continue | LoopStepFlow::Dispatch => {}
            }
        }

        // Post-loop: populate the terminal reason if the loop fell out of
        // max_steps without completing.
        if !self.state.completed && self.state.terminal_reason.is_none() {
            self.state.terminal_reason = Some(TerminalReason::MaxStepsReached {
                steps_executed: self.state.steps.len(),
            });
        }

        // Terminal boundary write (D8 / Task 3a.6.5). Every exit path from
        // the loop above sets `state.terminal_reason` before breaking —
        // plus the post-loop MaxStepsReached fallback right above — so a
        // single write here covers `Completed`, `MaxStepsReached`,
        // `MaxErrorsReached`, `ApprovalUnavailable`, `CompletionDisagreement`,
        // `ConsecutiveDestructiveCap`, and `LoopDetected` uniformly. A
        // run without any terminal_reason is a bug (no known code path
        // produces it), so the match_ is exhaustive on `Some`.
        if self.state.terminal_reason.is_some() {
            self.write_terminal_record().await;
        }

        // Drain happens in the outer `run` wrapper so it covers both
        // `Ok` and early-`?` `Err` exits from this function. See the
        // post-result `writer.flush().await` in `Self::run`.
        Ok(())
    }
}

/// Translate the openai-shaped `Vec<Value>` tool list (produced by
/// `Mcp::tools_as_openai`) into the `clickweave_mcp::Tool` shape the
/// prompt-spine builder needs. Keeps the openai format as the source of
/// truth for dispatch while letting the prompt builder operate on a typed
/// view.
fn openai_tools_to_mcp_tool_list(tools: &[Value]) -> Vec<clickweave_mcp::Tool> {
    tools
        .iter()
        .filter_map(|t| {
            let fun = t.get("function")?;
            let name = fun.get("name").and_then(Value::as_str)?.to_string();
            let description = fun
                .get("description")
                .and_then(Value::as_str)
                .map(String::from);
            let input_schema = fun
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let annotations = fun.get("annotations").cloned();
            Some(clickweave_mcp::Tool {
                name,
                description,
                input_schema,
                annotations,
            })
        })
        .collect()
}

/// Append the assistant's response and its tool result onto the transcript,
/// mirroring the legacy `AgentRunner::append_assistant_message`.
///
/// When the assistant returned `tool_calls`, the transcript gets the
/// assistant message (tool_calls only) plus a matching `tool_result`. When
/// the assistant returned plain text, only the assistant message is
/// appended.
/// Append an assistant tool-call + matching tool-result onto the
/// transcript so the next iteration's LLM call sees what was
/// dispatched. Synthesises the assistant message from the action's own
/// `(tool_call_id, tool_name, arguments)` rather than picking
/// `tool_calls.first()`: when a turn's `tool_calls` array starts with
/// mutation pseudo-tools (e.g. `push_subgoal` then `cdp_click`), the
/// "first call" is a mutation, not the action that actually ran, and
/// attaching the dispatched result to that id breaks action / result
/// causality from the LLM's point of view. Mutations are already
/// reflected in `<task_state>` at the next turn; they do not appear in
/// the transcript here.
///
/// The tool-result's `name` is stamped so `context::compact` can
/// identify stale snapshot-family bodies by the `SNAPSHOT_TOOL_NAMES`
/// set. Without this stamp, production tool-result messages leave
/// `name` unset and the snapshot-drop branch never fires for live
/// runs.
fn append_assistant_and_tool_result(
    messages: &mut Vec<Message>,
    tool_name: &str,
    arguments: &Value,
    tool_call_id: &str,
    previous_result: Option<&str>,
) {
    let tc = clickweave_llm::ToolCall {
        id: tool_call_id.to_string(),
        call_type: clickweave_llm::CallType::Function,
        function: clickweave_llm::FunctionCall {
            name: tool_name.to_string(),
            arguments: arguments.clone(),
        },
    };
    messages.push(Message::assistant_tool_calls(vec![tc]));
    let mut tool_msg = Message::tool_result(tool_call_id, previous_result.unwrap_or("ok"));
    tool_msg.name = Some(tool_name.to_string());
    messages.push(tool_msg);
}

/// Test-only re-exports for Task 3a.6 unit tests that need access to the
/// otherwise-private CDP classifier helpers. Keeps the helpers private on
/// the production surface while letting the integration tests exercise
/// them directly.
#[cfg(test)]
pub(crate) mod test_support {
    use serde_json::Value;

    use super::{FocusSkipReason, StateRunner};
    use crate::executor::Mcp;

    pub(crate) fn call_should_skip_focus_window<M: Mcp + ?Sized>(
        runner: &StateRunner,
        arguments: &Value,
        mcp: &M,
    ) -> Option<FocusSkipReason> {
        runner.should_skip_focus_window(arguments, mcp)
    }

    pub(crate) async fn call_maybe_cdp_connect<M: Mcp + ?Sized>(
        runner: &mut StateRunner,
        tool_name: &str,
        arguments: &Value,
        result_text: &str,
        mcp: &M,
    ) {
        runner
            .maybe_cdp_connect(tool_name, arguments, result_text, mcp)
            .await;
    }

    pub(crate) async fn call_finalize_cdp_connected<M: Mcp + ?Sized>(
        runner: &StateRunner,
        app_name: &str,
        cdp_port: u16,
        mcp: &M,
    ) {
        runner.finalize_cdp_connected(app_name, cdp_port, mcp).await;
    }
}

#[cfg(test)]
mod tests;
