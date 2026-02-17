# Workflow Execution (Reference)

Verified at commit: `0e907fc`

The engine executes a workflow graph sequentially, evaluating control-flow nodes in place and dispatching execution nodes to MCP tools or an AI-step tool loop.

## Entry Point

Execution starts at Tauri command `run_workflow` (`src-tauri/src/commands/executor.rs`), which creates `WorkflowExecutor` and calls `run()`.

High-level flow in `run()`:

1. Emit `StateChanged(Running)`
2. Log agent/VLM model info
3. Spawn MCP server (`npx` or custom command)
4. `RunStorage::begin_execution()`
5. Find entry points
6. Walk graph
7. Run post-execution check pass (if needed)
8. Emit `WorkflowCompleted` when completed normally
9. Emit `StateChanged(Idle)`

## Graph Walk

Main state machine (in `executor/run_loop.rs`):

1. Stop check (`ExecutorCommand::Stop`)
2. Skip disabled nodes (`follow_disabled_edge`)
3. For control-flow nodes, evaluate branch and jump
4. For execution nodes, run with retries
5. Follow next edge (`follow_single_edge`)

### Entry Points

Entry points are nodes with no incoming edges, excluding EndLoop back-edges (so loop cycles do not invalidate start-node detection).

### Edge Helpers

| Method | Purpose |
|--------|---------|
| `follow_single_edge(node_id)` | Regular unlabeled edge |
| `follow_edge(node_id, output)` | Labeled edge (`IfTrue`, `LoopBody`, etc.) |
| `follow_disabled_edge(node_id, node_type)` | Disabled control-flow fallback |

## Control Flow Semantics

### If

Evaluates condition in `RuntimeContext` and takes `IfTrue` or `IfFalse` edge.

### Switch

Evaluates cases in order, takes first matching `SwitchCase(name)`, else `SwitchDefault`.

### Loop

Uses do-while semantics: first visit always takes `LoopBody`.

Iteration logic:

1. If `iteration >= max_iterations`, exit via `LoopDone`
2. Else if `iteration > 0` and exit condition is true, exit via `LoopDone`
3. Else increment counter and continue via `LoopBody`

Loop counters are stored in `RuntimeContext.loop_counters` keyed by Loop node id.

### EndLoop

`EndLoop { loop_id }` jumps directly back to the paired Loop node.

## Node Execution

Non-control-flow nodes run through `execute_node_with_retries()`.

### Deterministic Path

For most nodes:

`NodeType -> node_type_to_tool_invocation() -> mcp.call_tool(name, args)`

Special handling:

- `Click` with `target` and no coordinates: resolve via `find_text` first
- `find_text` fallback: if no matches and `available_elements` exists, resolve element name with LLM and retry
- `FocusWindow` by app name: resolve app to pid via `list_apps` + LLM
- `TakeScreenshot(Window)` with target app name: same app-resolution path

### AI Step Path

`AiStep` runs an LLM/tool loop:

1. Build system + user prompts
2. Filter tools by `allowed_tools` if set
3. Repeatedly call LLM
4. Execute returned tool calls via MCP
5. Save tool result images as artifacts
6. If images exist:
   - with VLM: summarize via `analyze_images()`
   - without VLM: attach images directly to next LLM turn
7. Stop on no tool calls, timeout, max tool calls, or user stop

## Retry Behavior

### Node Retries

Each node has `retries` (0-10). On failure before final attempt:

1. Evict relevant caches (`app_cache`, `element_cache`, `focused_app` when applicable)
2. Record `retry` trace event
3. Re-run the node

If retries are exhausted, execution fails and graph walk stops.

### Planning/Assistant Retries

See [Planning & LLM Retry Logic](../llm/planning-retries.md).

## Variable Extraction

After each successful execution node, results are written into `RuntimeContext` using sanitized node name prefixes.

Always set:

- `<node>.success = true`
- `<node>.result` (raw parsed result, or empty string for null)

Object result:

- each top-level field -> `<node>.<field>`

Array result:

- `<node>.found` (bool)
- `<node>.count` (number)
- first element fields -> `<node>.<field>`
- typed aliases:
  - `ListWindows` -> `<node>.windows`
  - `FindText` -> `<node>.matches`
  - `FindImage` -> `<node>.matches`

String/number/bool result:

- `<node>.result`

## Runtime Caches

| Cache | Key | Value | Used By |
|-------|-----|-------|---------|
| `app_cache` | user app text | `{name, pid}` | FocusWindow, TakeScreenshot |
| `element_cache` | `(target, app_name?)` | resolved element name | Click, FindText |
| `focused_app` | none | app name | scoped find-text and resolution |

## Run Storage Layout

Saved project path:

```
<project>/.clickweave/runs/<workflow>/<execution_dir>/
```

Unsaved project fallback path:

```
<app_data>/runs/<workflow>_<short_workflow_id>/<execution_dir>/
```

Per-node run directory:

```
<execution_dir>/<sanitized_node_name>/
├── run.json
├── events.jsonl
└── artifacts/
```

Execution-level events are also stored in:

```
<execution_dir>/events.jsonl
```

## Trace Events

Common event types recorded in trace files:

- `node_started`
- `tool_call`
- `tool_result`
- `vision_summary`
- `branch_evaluated`
- `loop_iteration`
- `loop_exited`
- `variable_set`
- `retry`
- `target_resolved`
- `app_resolved`
- `element_resolved`

## Post-Execution Checks

If any completed node has checks or expected outcome text, a check pass runs after graph walk. Results are emitted as `ChecksCompleted` and persisted per node.

See [Node Checks](../../verification/node-checks.md).

## Key Files

| File | Role |
|------|------|
| `crates/clickweave-engine/src/executor/mod.rs` | Executor struct and events |
| `crates/clickweave-engine/src/executor/run_loop.rs` | Graph walk + retries + variable extraction |
| `crates/clickweave-engine/src/executor/deterministic.rs` | Deterministic node execution |
| `crates/clickweave-engine/src/executor/ai_step.rs` | AI-step tool loop |
| `crates/clickweave-engine/src/executor/app_resolve.rs` | App resolution + cache eviction |
| `crates/clickweave-engine/src/executor/element_resolve.rs` | Element resolution + cache eviction |
| `crates/clickweave-engine/src/executor/check_eval.rs` | Post-run check pass |
| `crates/clickweave-core/src/runtime.rs` | Runtime context and condition evaluation |
| `crates/clickweave-core/src/storage.rs` | Run/event/artifact persistence |
