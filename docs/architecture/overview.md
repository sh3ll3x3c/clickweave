# Architecture Overview

Clickweave is a hybrid desktop application that combines a Rust backend with a React frontend via Tauri v2. The backend is organized as a Cargo workspace with four domain crates, and the frontend is a Vite-based React app with auto-generated TypeScript bindings.

## Workspace Crates

```
crates/
├── clickweave-core/     # Shared types, validation, storage
├── clickweave-engine/   # Workflow execution engine
├── clickweave-llm/      # LLM client & planner logic
└── clickweave-mcp/      # MCP client & protocol
src-tauri/               # Tauri application shell & IPC commands
ui/                      # React frontend (Vite + Tailwind)
```

### Dependency Graph

```
clickweave-engine
├── clickweave-core
├── clickweave-llm
│   └── clickweave-core
└── clickweave-mcp

src-tauri
├── clickweave-engine
├── clickweave-llm
├── clickweave-mcp
└── clickweave-core
```

## Crate Responsibilities

### clickweave-core

Shared types and pure logic used by all other crates.

| Module | Purpose |
|--------|---------|
| `workflow.rs` | `Workflow`, `Node`, `Edge`, `NodeType`, `EdgeOutput` — the graph model |
| `node_types.rs` | Per-node parameter structs (`ClickParams`, `FindTextParams`, `AiStepParams`, etc.) |
| `control_flow.rs` | `IfParams`, `SwitchParams`, `LoopParams`, `EndLoopParams`, `Condition`, `ValueRef`, `Operator` |
| `checks.rs` | `Check`, `CheckType`, `CheckResult`, `CheckVerdict`, `NodeVerdict`, `OnCheckFail` |
| `validation.rs` | `validate_workflow()` — structural validation (cycles, missing edges, orphan nodes) |
| `storage.rs` | `RunStorage` — file I/O for run directories, events.jsonl, artifacts |
| `tool_mapping.rs` | Bidirectional conversion between `NodeType` and MCP tool invocations |
| `context.rs` | `RuntimeContext` — variable storage and condition evaluation during execution |

All core types derive `Serialize`/`Deserialize` and optionally `specta::Type` behind a feature flag for auto-generated TypeScript bindings.

### clickweave-engine

The workflow executor. Walks the node graph, dispatches tool calls or AI agentic loops, manages retries, and streams trace events.

| Module | Purpose |
|--------|---------|
| `executor/mod.rs` | `WorkflowExecutor<C>` struct, main `run()` loop, graph navigation, control flow evaluation |
| `executor/run_loop.rs` | Entry point orchestration, node retry logic, variable extraction, check screenshot capture |
| `executor/ai_step.rs` | Agentic loop for `AiStep` nodes — multi-turn LLM+tool conversation |
| `executor/deterministic.rs` | Deterministic node execution — maps `NodeType` to MCP tool calls |
| `executor/app_resolve.rs` | LLM-assisted app name resolution with caching |
| `executor/element_resolve.rs` | LLM-assisted UI element name resolution with caching |
| `executor/check_eval.rs` | Post-execution check evaluation via VLM |
| `executor/trace.rs` | Event recording, image saving, run finalization |

See [Workflow Execution](../engine/execution.md) for details.

### clickweave-llm

LLM client and planner/patcher logic.

| Module | Purpose |
|--------|---------|
| `client.rs` | `LlmClient` — OpenAI-compatible HTTP client |
| `types.rs` | `ChatBackend` trait, `Message`, `ChatResponse`, `ToolCall` |
| `planner/plan.rs` | `plan_workflow()` — generates a workflow from natural language |
| `planner/patch.rs` | `patch_workflow()` — modifies an existing workflow from a patch prompt |
| `planner/assistant.rs` | `assistant_chat()` — conversational assistant with patch generation and validation retry |
| `planner/repair.rs` | `chat_with_repair()` — generic one-shot retry with error feedback |
| `planner/prompt.rs` | System prompt construction for planner, patcher, and assistant |
| `planner/parse.rs` | JSON extraction, node layout, step filtering |
| `planner/mapping.rs` | `PlanStep` to `NodeType` conversion |
| `planner/summarize.rs` | Conversation overflow summarization |
| `planner/conversation.rs` | `ConversationSession` — persistent chat history with windowing |
| `vision.rs` | `analyze_images()` — VLM image analysis |
| `step_prompt.rs` | Prompt construction for AI step execution |

See [Planning & Retries](../llm/planning-retries.md) for the retry/repair system.

### clickweave-mcp

MCP (Model Context Protocol) client for communicating with the native-devtools-mcp server.

| Module | Purpose |
|--------|---------|
| `client.rs` | `McpClient` — spawns subprocess, manages lifecycle, calls tools |
| `protocol.rs` | JSON-RPC message types and serialization |

See [MCP Integration](../mcp/integration.md) for details.

## Data Flow

### Planning Phase

```
User intent (text)
    │
    ▼
┌─────────────┐    system prompt + tools    ┌─────────┐
│ Tauri cmd   │ ──────────────────────────► │ LLM     │
│ plan_workflow│                             │ Provider│
└─────────────┘ ◄────────────────────────── └─────────┘
    │               JSON workflow plan
    ▼
┌─────────────┐
│ Parse &     │  extract JSON → PlannerOutput/PlannerGraphOutput
│ Validate    │  map steps → NodeType
│             │  infer control flow edges
│             │  validate_workflow()
└─────────────┘
    │
    ▼ (retry with error feedback if parse/validation fails)
Workflow { nodes, edges }
    │
    ▼
Frontend receives workflow → renders on canvas
```

### Execution Phase

```
Frontend: runWorkflow(request)
    │
    ▼
┌───────────────┐     spawn     ┌──────────┐
│ WorkflowExec  │ ────────────► │ MCP      │
│ utor.run()    │               │ Server   │
└───────────────┘               └──────────┘
    │                               ▲
    │  for each node:               │ tool calls
    │  ┌─────────────────┐          │
    │  │ Deterministic:   │─────────┘
    │  │ node→tool→MCP    │
    │  ├─────────────────┤
    │  │ AiStep:          │──► LLM ──► tool calls ──► MCP
    │  │ agentic loop     │
    │  ├─────────────────┤
    │  │ Control flow:    │
    │  │ If/Switch/Loop   │──► evaluate condition → follow edge
    │  └─────────────────┘
    │
    ▼ events streamed via executor:// prefix
Frontend: updates node status, logs, verdicts
```

### IPC (Tauri Commands)

The frontend communicates with the backend through Tauri commands, defined in `src-tauri/src/commands/`. Key commands:

| Command | Purpose |
|---------|---------|
| `plan_workflow` | Generate a workflow from intent text |
| `patch_workflow` | Modify a workflow from a patch prompt |
| `assistant_chat` | Conversational assistant with optional patching |
| `cancel_assistant_chat` | Cancel an in-progress assistant LLM request |
| `run_workflow` | Execute a workflow |
| `stop_workflow` | Stop a running workflow |
| `validate` | Validate a workflow's structure |
| `open_project` / `save_project` | Project file I/O |
| `list_runs` / `load_run_events` | Run history queries |
| `read_artifact_base64` | Read a saved artifact |
| `import_asset` | Import an image asset into a project |
| `node_type_defaults` | Get default configurations for all node types |

### Event System

Backend-to-frontend events use the `executor://` prefix and are emitted via Tauri's `app.emit()`:

| Event | Payload | Purpose |
|-------|---------|---------|
| `executor://log` | `String` | Execution log message |
| `executor://state` | `ExecutorState` | Running/Idle state change |
| `executor://node_started` | `Uuid` | Node began execution |
| `executor://node_completed` | `Uuid` | Node finished successfully |
| `executor://node_failed` | `(Uuid, String)` | Node failed with error |
| `executor://run_created` | `(Uuid, NodeRun)` | New run record created |
| `executor://workflow_completed` | — | All nodes finished |
| `executor://checks_completed` | `Vec<NodeVerdict>` | Post-execution check results |
| `executor://error` | `String` | Executor-level error |

## Type Safety Bridge

TypeScript types are auto-generated from Rust types using **specta 2.0.0-rc** and **tauri-specta 2.0.0-rc**:

1. Rust types annotated with `#[derive(specta::Type)]` (behind `specta` feature flag)
2. Tauri commands registered with `tauri_specta::Builder`
3. Bindings exported to `ui/src/bindings.ts` at debug startup
4. Frontend imports typed command wrappers and type definitions

This ensures the frontend and backend type systems stay in sync without manual maintenance.

## Key Files

| File | Role |
|------|------|
| `Cargo.toml` (root) | Workspace definition and shared dependency versions |
| `src-tauri/src/main.rs` | Tauri app setup, logging, specta binding export |
| `src-tauri/src/commands/*.rs` | IPC command handlers |
| `ui/src/bindings.ts` | Auto-generated TypeScript types and command wrappers |
| `ui/src/store/useAppStore.ts` | Main Zustand store |
| `ui/src/App.tsx` | Root React component |
