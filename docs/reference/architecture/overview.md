# Architecture Overview (Reference)

Clickweave is a Tauri v2 desktop app with a Rust backend and a React frontend, plus a headless CLI.

## Workspace Crates

```
crates/
├── clickweave-core/     # Project manifest, runtime state, storage, safety types
├── clickweave-engine/   # Skill runner, agent loop, trace graph, skill store
├── clickweave-llm/      # LLM client, image prep, chat types
├── clickweave-mcp/      # MCP JSON-RPC client
├── clickweave-host/     # UI-agnostic wiring library (above engine/core/llm/mcp)
└── clickweave-cli/      # Headless CLI binary (`clickweave`) built on clickweave-host
src-tauri/               # Tauri app shell + IPC commands
ui/                      # React frontend
```

### Dependency Graph

```
clickweave-engine
├── clickweave-core
├── clickweave-llm
│   └── clickweave-core
└── clickweave-mcp

clickweave-host
├── clickweave-core
├── clickweave-engine
├── clickweave-llm
└── clickweave-mcp

src-tauri
└── clickweave-host  (+ clickweave-core, clickweave-engine, clickweave-llm, clickweave-mcp)

clickweave-cli
└── clickweave-host

clickweave-evals
└── clickweave-host  (+ clickweave-engine, clickweave-llm, clickweave-mcp)
```

## Crate Responsibilities

### `clickweave-core`

| Module | Purpose |
|--------|---------|
| `project.rs` | `ProjectManifest` — on-disk project envelope `{ id, name, intent, schema_version }` |
| `skill_run.rs` | `SkillRun`, `SectionOutcome` — skill execution record; run storage is skill-keyed |
| `runtime.rs` | `RuntimeContext` variable store |
| `storage/` | `RunStorage` — skill-keyed execution and event persistence, `cache_path()` for decision cache |
| `decision_cache.rs` | `DecisionCache` — persists LLM decisions for replay |
| `safety.rs` | `SafetyScope` discriminant for supervision and approval events |
| `cdp.rs` | CDP types: `CdpFindElementsResponse`, `CdpFindElementMatch`, `rand_ephemeral_port()` |
| `app_detection.rs` | App classification (Electron, Chrome, native) from bundle ID / path / PID |
| `walkthrough/` | Walkthrough recording types, event normalization, draft synthesis, session storage |
| `variant_index.rs` | Agent variant index for caching action outcomes |

`clickweave-core` does **not** export `Workflow`, `Node`, `Edge`, or `NodeType`. Those canvas-graph types were removed. The agent runner's accumulating trace graph lives in `clickweave-engine` and is engine-private.

### `clickweave-engine`

| Module | Purpose |
|--------|---------|
| `executor/skill_runner.rs` | Native skill runner — index-walk over `&[ActionSketchStep]` with first-class `Loop` |
| `executor/mod.rs` | Executor shared state, MCP lifecycle |
| `agent/trace_graph.rs` | `AgentTraceGraph`, `TraceNode`, `TraceEdge`, `TraceNodeKind` — engine-private accumulating trace (no specta derives, not exposed across IPC) |
| `agent/tool_mapping/` | `TraceNodeKind` ↔ MCP tool invocation mapping (engine-private) |
| `agent/runner/` | `StateRunner` — state-centric ReAct loop (observe / phase-infer / render / decide / dispatch / compact) |
| `agent/skills/` | `SkillStore`, `SkillIndex`, `SkillPatch`, patch application, journal protocol |
| `agent/world_model.rs` | `WorldModel` — harness-owned environment facts with per-field freshness |
| `agent/task_state.rs` | `TaskState` — subgoal stack, watch slots, harness-inferred phase |
| `agent/phase.rs` | `Phase` — `{ Exploring, Executing, Recovering }`, pure `phase::infer` |
| `agent/step_record.rs` | `StepRecord` / `BoundaryKind` — boundary snapshots written to `events.jsonl` |
| `agent/episodic/` | Two-tier episodic memory (workflow-local + global SQLite) |

See [Skill Execution](../engine/execution.md).

### `clickweave-llm`

| Module | Purpose |
|--------|---------|
| `client.rs` | OpenAI-compatible chat client, health check, AI-step prompts |
| `types.rs` | `ChatBackend`, message/response/tool-call types |
| `image_prep.rs` | Image resizing for VLM input |

### `clickweave-mcp`

| Module | Purpose |
|--------|---------|
| `client.rs` | `McpClient` subprocess lifecycle + tool calls |
| `protocol.rs` | JSON-RPC and MCP payload types |

See [MCP Integration](../mcp/integration.md).

### `clickweave-host`

UI-agnostic wiring library. Sits above `{clickweave-engine, clickweave-core, clickweave-llm, clickweave-mcp}` and below `{src-tauri, clickweave-cli, clickweave-evals}`. No `tauri` dependency; performs no event emission or terminal I/O.

| Module | Purpose |
|--------|---------|
| `config.rs` | `llm_config(...)` — builds `LlmConfig`, normalises `Some("") → None` for the API key |
| `mcp.rs` | `EnvOverride { Always, DebugOnly }`, `resolve_mcp_binary(EnvOverride)`, `spawn_mcp(...)` — MCP binary resolution and subprocess spawn |
| `storage.rs` | `app_data_dir()`, `project_dir(path)`, `ProjectLocation`, `resolve_storage(...)`, `load_project(path)` — path normalisation and storage construction |
| `context.rs` | `build_episodic_context(...)`, `build_skill_context(...)` — agent-context construction |
| `run.rs` | `AgentRunParams`, `run_agent(params)` — engine-call seam, dispatches to the appropriate `run_agent_workflow*` variant |
| `approval.rs` | `ApprovalDecision`, `ApprovalResponder` trait, `AutoApprove` |
| `lifecycle.rs` | `spawn_agent_run(...)`, `AgentRunHandle` — live-run lifecycle wrapper for CLI and Tauri |
| `skills.rs` | `run_skill(...)`, `list_skills(dir)`, `load_skill(dir, id)` |
| `runs.rs` | `list_runs(storage, skill_id)`, `load_run_events(...)` |

### `clickweave-cli`

Headless CLI (`clickweave` binary) built on `clickweave-host`. Provides `run`, `run-skill`, `skills`, and `runs` subcommands. Uses `clap` derive for argument parsing. Handles rendering (`RunnerOutput` → stderr; final summary → stdout; `--json` NDJSON mode), exit codes, and the `StdinResponder` / `AutoApprove` approval strategies. `run-skill` has no approval flags — the deterministic runner does not gate per step.

## Data Flow

### Agent Execution

```
UI
  -> Tauri command: run_agent (goal, endpoint config)
  -> run_agent_workflow builds a StateRunner + AgentTraceGraph
     - observe: drain pending InvalidationEvents into WorldModel, refresh stale fields
     - phase-infer: derive Phase { Exploring | Executing | Recovering } from signals
     - skill retrieval: refresh applicable skills after push_subgoal mutations
     - render: state block (<world_model> + <task_state> + optional <applicable_skills>) at top of user turn
     - decide: one LLM call -> AgentTurn { mutations, action }
     - apply mutations: TaskStateMutation batch (push/complete subgoal, watch slots, hypotheses)
     - dispatch: AgentAction::ToolCall via MCP, InvokeSkill expansion, or AgentDone / AgentReplan
     - continuity hooks: update WorldModel.last_screenshot / last_native_ax_snapshot
     - invalidation: queue InvalidationEvents for the next observe
     - boundary record: write StepRecord at Terminal / SubgoalCompleted / RecoverySucceeded
     - compact: drop snapshot tool-result messages older than current step
  -> emit agent://* events (including task_state_changed, world_model_changed,
     boundary_record_written) to UI
```

`AgentTraceGraph` (`clickweave-engine::agent::trace_graph`) accumulates `TraceNode` and `TraceEdge` entries as the agent loop runs. It is engine-private: no specta derives, never serialized across IPC. The UI receives structured events over the `agent://*` channel instead.

### Skill Execution

```
UI / CLI
  -> run_skill (skill_id, variables)  [Tauri command or host::run_skill]
  -> skill_runner::run_skill_steps walks &[ActionSketchStep]
     - ToolCall steps: resolve target, call MCP tool, record trace events
       (requires_approval field is read by the Tauri executor's safety gate
        but is ignored by the deterministic host::run_skill runner — no
        per-step approval gating in the CLI or headless path)
     - Loop steps: evaluate LoopPredicate, iterate body steps
  -> persist SkillRun per section outcome
  -> emit executor://* events to UI  [Tauri path only]
```

The frontend approval overlay (`SkillSectionApprovalOverlay` / `useSafetyEventRouter`) is retained for **agent-loop** approvals routed via `SafetyScope::AdHoc`; skill-step `SafetyScope::Skill` approval enforcement is future work.

## IPC Commands

### Agent Commands
- `run_agent` — start an agent session with a goal
- `stop_agent` — cancel a running agent
- `approve_agent_action` — approve or reject a pending agent action
- `add_run_to_skill` — promote a completed agent run into a skill
- `save_run_as_skill` — save a run as a new skill draft
- `resolve_completion_disagreement` — resolve a pending VLM completion disagreement

### Executor Commands
- `run_skill` — execute a skill by `skill_id` with optional variable bindings
- `resume_skill_from_failure` — resume a failed skill run from a given section
- `stop_workflow` — cancel execution
- `supervision_respond` — respond to supervision pause (retry/skip/abort)

### Project Commands
- `ping`, `get_mcp_status` — health checks
- `open_project`, `save_project` — file I/O (`ProjectManifest` on disk)
- `pick_workflow_file`, `pick_save_file` — native open/save dialogs
- `import_asset` — pick an image and copy it into the project's `assets/` dir
- `confirmable_tools`, `check_endpoint`, `list_models` — settings helpers

### Skill Commands
- `list_skills_for_panel` — list skills by state bucket (draft/confirmed/promoted)
- `load_skill_full` — load full skill (sections, action_sketch, variables, replay)
- `confirm_skill_proposal` — confirm a draft skill proposal with edits
- `reject_skill_proposal` — reject a draft skill proposal
- `promote_skill_to_global` — move a skill to the global tier
- `fork_skill` — fork a skill into a new editable copy
- `delete_skill` — delete a skill and its associated files
- `apply_skill_patch` — apply a `SkillPatch` (four-layer atomic write via journal protocol)

### Walkthrough Commands
- `start_walkthrough`, `stop_walkthrough`, `pause_walkthrough`, `resume_walkthrough`, `cancel_walkthrough`
- `get_walkthrough_draft`, `apply_walkthrough_annotations`, `seed_walkthrough_cache`
- `save_walkthrough_as_skill` — convert a walkthrough session into a skill draft
- `detect_cdp_apps`, `validate_app_path`

### Chrome Profile Commands
- `list_chrome_profiles`, `create_chrome_profile`, `is_chrome_profile_configured`
- `get_chrome_profile_path`, `launch_chrome_for_setup`

### Run History
- `list_runs` — list runs for a skill; query keyed by `{ skill_id, run_id? }`
- `load_run_events` — load trace events for a skill run; keyed by `{ skill_id, run_id, section_id? }`
- `read_artifact_base64` — read an artifact from a skill run directory

## Safety Events

`SafetyScope` (`clickweave-core::safety`) is the discriminant carried in all supervision and approval events:

```rust
pub enum SafetyScope {
    Skill { skill_id: String, section_id: String, step_id: String },
    AdHoc { run_id: Uuid },
}
```

- `SafetyScope::Skill` — defined for skill-step approval events. Carries the exact skill, section, and step position. The Tauri executor's `should_gate_step` logic uses this scope, but the deterministic skill runner invoked by `host::run_skill` and the CLI does not gate per step — `requires_approval` is passed through as an ignored parameter (`_requires_approval`). Skill-step approval enforcement is future work.
- `SafetyScope::AdHoc` — emitted by agent-loop runs (no active skill). The frontend routes these to an `AssistantThread`-anchored approval card. The `SkillSectionApprovalOverlay` and `useSafetyEventRouter` surface remain active for this path.

Both `SupervisionPaused` / `SupervisionPassed` and `ApprovalRequired` carry the `SafetyScope` field.
