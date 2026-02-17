# Workflow Execution

The execution engine walks a directed graph of nodes, dispatching each to either a deterministic MCP tool call or an AI agentic loop. This document covers the run loop, control flow evaluation, retry logic, and variable extraction.

## Entry Point

Execution starts when the frontend calls the `run_workflow` Tauri command, which creates a `WorkflowExecutor<C>` and calls its `run()` method.

```
WorkflowExecutor::run()
  1. Log model info for agent (and VLM if configured)
  2. Spawn MCP server (npx or binary path)
  3. Create execution directory via RunStorage
  4. Find entry points (nodes with no incoming edges)
  5. Walk graph sequentially
  6. Run post-execution check evaluation
  7. Emit WorkflowCompleted
```

## Graph Walk

The executor follows a simple state machine:

```rust
let mut current: Option<Uuid> = Some(entry_point);

while let Some(node_id) = current {
    // 1. Check for user stop
    // 2. Skip disabled nodes
    // 3. Control flow nodes → evaluate, follow edge
    // 4. Execution nodes → execute with retries
    // 5. Follow single outgoing edge → next
}
```

### Entry Points

A node is an entry point if no edges point to it, **excluding** EndLoop back-edges (which form cycles by design). The executor starts from the first entry point found.

### Edge Following

| Method | Used For |
|--------|----------|
| `follow_single_edge(node_id)` | Regular sequential nodes — follows the one unlabeled outgoing edge |
| `follow_edge(node_id, output)` | Control flow nodes — follows a labeled edge (`IfTrue`, `LoopBody`, etc.) |
| `follow_disabled_edge(node_id, node_type)` | Disabled nodes — follows fallthrough edges (e.g., `LoopDone` for disabled Loop) |

## Control Flow

Control flow nodes (`If`, `Switch`, `Loop`, `EndLoop`) are evaluated in-place without executing any MCP tools.

### If Nodes

```
If(condition)
  ├── IfTrue edge  → condition is true
  └── IfFalse edge → condition is false
```

Evaluates the condition against the `RuntimeContext` (which stores variables from previous node results), then follows the appropriate edge.

### Switch Nodes

```
Switch(cases)
  ├── SwitchCase("case_a") → first matching case
  ├── SwitchCase("case_b") → ...
  └── SwitchDefault         → no case matched
```

Evaluates each case's condition in order; follows the first match. Falls back to `SwitchDefault` if no case matches.

### Loop Nodes

Loops use **do-while semantics**: the body always executes at least once. The exit condition is NOT checked on the first iteration (iteration 0).

```
Loop(exit_condition, max_iterations)
  ├── LoopBody edge → enters loop body
  └── LoopDone edge → exit condition met or max iterations hit
```

A `loop_counters` map in `RuntimeContext` tracks the current iteration per loop node. When the executor encounters a Loop node:

1. **First visit (counter = 0):** Always follow LoopBody (skip condition check)
2. **Subsequent visits:** Evaluate exit condition
   - True → follow LoopDone, reset counter
   - False + under max → increment counter, follow LoopBody
   - False + at max → follow LoopDone, emit warning, reset counter

### EndLoop Nodes

EndLoop nodes jump back to their paired Loop node via the `loop_id` field. This creates the back-edge that allows the loop to re-evaluate its exit condition.

## Node Execution

Non-control-flow nodes are dispatched through `execute_node_with_retries()`, which wraps the actual execution with retry logic.

### Deterministic Nodes

Most node types map directly to a single MCP tool call:

```
NodeType → node_type_to_tool_invocation() → ToolInvocation { name, arguments }
         → mcp.call_tool(name, arguments)
         → parse result
```

Special handling exists for:

- **Click with target:** If a Click node has a text `target` (e.g., "Submit"), the executor calls `find_text` first to resolve coordinates. If no match is found but `available_elements` are returned, it uses the LLM to resolve the element name and retries.
- **FocusWindow with AppName:** Calls `list_apps` and uses the LLM to match the user's app name to a running process.
- **TakeScreenshot with Window mode:** Resolves the app name the same way as FocusWindow.

### AI Step Nodes

`AiStep` nodes run an agentic loop:

```
1. Build system prompt + user prompt
2. Filter tools to allowed_tools list (if specified)
3. Loop:
   a. Send messages to agent LLM (with tools)
   b. If no tool calls → done
   c. For each tool call:
      - Execute via MCP
      - Save result images as artifacts
      - Append tool_result to messages
   d. If images returned:
      - VLM configured → analyze_images() → append summary
      - No VLM → append raw images to messages
   e. Check max_tool_calls / timeout limits
4. Return final assistant text
```

## Retry Logic

### Node-Level Retries

Each node has a configurable `retries` count (0-10). When a node fails:

1. Caches are evicted for the node type (app name cache, element cache, focused app)
2. The node is re-executed
3. If all retries are exhausted, the workflow stops with a failure

Cache eviction on retry forces re-resolution of app names and UI element names, which is critical when the desktop state has changed between attempts.

### Planning Phase Retries

See [Planning & Retries](../llm/planning-retries.md) for the LLM output repair system.

## Variable Extraction

After each node completes successfully, the executor extracts variables from the result and stores them in `RuntimeContext` for use by subsequent control flow conditions.

### Extraction Rules

For every completed node:
- `<node_name>.success = true`

For object results (e.g., from `find_text`):
- Each top-level key becomes `<node_name>.<key>`

For array results (e.g., from `list_windows`):
- `<node_name>.found = true/false` (based on non-empty)
- `<node_name>.count = N`
- First element's fields become `<node_name>.<key>`
- Type-specific alias: `<node_name>.windows`, `<node_name>.matches`, etc.

### Typed Aliases

| Node Type | Alias |
|-----------|-------|
| ListWindows | `.windows` |
| FindImage | `.matches` |
| FindText | (uses object extraction directly) |

## Caching

Three runtime caches prevent redundant LLM calls during execution:

| Cache | Key | Value | Used By |
|-------|-----|-------|---------|
| `app_cache` | User input string | `{ name, pid }` | FocusWindow, TakeScreenshot |
| `element_cache` | `(target, app_name)` | Resolved element name | Click, FindText |
| `focused_app` | — | Current app name | Scoped element resolution |

All caches are evicted per-node-type on retry to force fresh resolution.

## Run Storage

Every execution creates a directory structure:

```
.clickweave/runs/<workflow_name>/<YYYY-MM-DD_HH-MM-SS_uuid>/
├── events.jsonl          # Execution-level events
└── <node_name>/
    ├── run.json          # Run metadata (status, timestamps, trace_level)
    ├── events.jsonl      # Node-level trace events
    └── artifacts/        # Screenshots, OCR results, template matches
```

### Trace Events

Events are newline-delimited JSON objects appended to `events.jsonl`:

| Event Type | When |
|------------|------|
| `node_started` | Node execution begins |
| `tool_call` | MCP tool invoked (name + args) |
| `tool_result` | MCP tool returned (text + image count) |
| `vision_summary` | VLM analyzed images |
| `branch_evaluated` | Control flow condition evaluated |
| `variable_set` | Variable stored in RuntimeContext |
| `retry` | Node retry triggered |
| `loop_exited` | Loop exited (condition met or max iterations) |

## Post-Execution Checks

After all nodes complete, if any had checks or expected outcomes, a check evaluation pass runs. See [Node Checks](../verification/node-checks.md) for details.

## Key Files

| File | Role |
|------|------|
| `crates/clickweave-engine/src/executor/mod.rs` | `WorkflowExecutor`, graph walk, control flow |
| `crates/clickweave-engine/src/executor/run_loop.rs` | Main run loop, retries, variable extraction |
| `crates/clickweave-engine/src/executor/ai_step.rs` | AI step agentic loop |
| `crates/clickweave-engine/src/executor/deterministic.rs` | Deterministic node dispatch |
| `crates/clickweave-engine/src/executor/app_resolve.rs` | App name resolution |
| `crates/clickweave-engine/src/executor/element_resolve.rs` | Element name resolution |
| `crates/clickweave-engine/src/executor/check_eval.rs` | Check evaluation pass |
| `crates/clickweave-engine/src/executor/trace.rs` | Event recording, artifact saving |
| `crates/clickweave-core/src/context.rs` | RuntimeContext (variables, conditions) |
| `crates/clickweave-core/src/storage.rs` | RunStorage (file I/O) |
