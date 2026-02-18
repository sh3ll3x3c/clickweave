# Planning & LLM Retry Logic (Reference)

Verified at commit: `1d53429`

Planner/assistant flows layer retries and parsing tolerance to handle malformed LLM output.

## Retry Layers

| Layer | Scope | Trigger | Limit |
|------|-------|---------|-------|
| JSON repair (`chat_with_repair`) | planner + patcher | parse/build/validation failure in processing closure | 1 retry |
| Assistant validation retry | assistant chat | patch merges to invalid workflow | configurable (`0..10`, default 3) |
| Lenient parsing (`parse_lenient`) | planner + patcher + assistant parsing paths | malformed individual items | skip bad items, keep processing |

## 1. JSON Repair (`planner/repair.rs`)

`chat_with_repair()` wraps an LLM call and retries once with error feedback.

Flow:

1. Call LLM
2. Run caller `process(content)`
3. On failure, append assistant output + corrective user message
4. Call LLM again
5. Re-run processing and return success/failure

Used by:

- `plan_workflow_with_backend()`
- `patch_workflow_with_backend()`

## 2. Assistant Validation Retry (`planner/assistant.rs`)

Assistant path retries only when a patch is produced and merged workflow fails `validate_workflow()`.

Flow:

1. Build messages and call LLM
2. Parse assistant response (conversation/patch/plan)
3. If patch exists and `max_repair_attempts > 0`:
   - build candidate via `merge_patch_into_workflow()`
   - validate candidate
   - if invalid and attempts remain, append validation error feedback and retry
   - if invalid and exhausted, return patch as-is
4. Return assistant result

`max_repair_attempts` semantics:

- `0`: skip validation
- `1`: validate, no retry
- `N >= 2`: validate + up to `N-1` retries

UI setting is persisted as `maxRepairAttempts` in `settings.json`.

## 3. Lenient Parsing (`planner/mod.rs`)

### `parse_lenient<T>(raw: &[Value])`

- deserializes each item independently
- malformed items are skipped with warnings
- prevents whole response from failing because of one bad entry

### Unknown step handling

`PlanStep` includes `#[serde(other)] Unknown`, allowing unknown `step_type` values to deserialize and later be filtered.

### Feature-flag filtering

`step_rejected_reason()` drops `AiStep`/`AiTransform` based on enabled flags, with warnings.

## Control-Flow Edge Inference

`infer_control_flow_edges()` (in `planner/mod.rs`) repairs common LLM graph issues:

1. Label unlabeled `If` edges as `IfTrue`/`IfFalse`
2. Label unlabeled `Loop` edges as `LoopBody`/`LoopDone`
3. Reroute body-to-loop back edges through `EndLoop`
4. Add missing `EndLoop -> Loop` back-edge
5. Convert `EndLoop -> Next` forward edge into `LoopDone` when needed
6. Remove `LoopDone -> EndLoop` edges that would create infinite loops

For flat plans, `pair_endloop_with_loop()` pairs EndLoop/Loop by nesting order before inference.

## Prompt Structure

All prompt builders live in `crates/clickweave-llm/src/planner/prompt.rs`. The AI-step runtime prompt lives in `crates/clickweave-llm/src/client.rs`.

### Planner Prompt (`planner_system_prompt`)

Composed for `plan_workflow`. Structure:

```
Role: "You are a workflow planner for UI automation."
  ↓
MCP tool schemas (pretty-printed JSON array from tools/list)
  ↓
Step type catalog (conditionally includes AiTransform / AiStep based on feature flags):
  1. Tool         — single MCP tool call
  2. AiTransform  — bounded AI op, no tool access (if allow_ai_transforms)
  3. AiStep       — agentic LLM+tool loop (if allow_agent_steps)
  4. Loop         — do-while with exit condition
  5. EndLoop      — marks loop body end
  6. If           — 2-branch conditional
  ↓
Condition / Variable / Operator reference
  ↓
Output format rules:
  - Simple workflows: {"steps": [...]}
  - Control-flow workflows: {"nodes": [...], "edges": [...]}
  ↓
Behavioral rules (find_text before click, focus window first, launch_app if needed, etc.)
```

User message: `"Plan a workflow for: <intent>"`

### Patcher Prompt (`patcher_system_prompt`)

Composed for `patch_workflow`. Structure:

```
Role: "You are a workflow editor for UI automation."
  ↓
Current workflow snapshot:
  - Nodes: [{id, name, tool_name, arguments}] (Click nodes include target field)
  - Edges: [{from, to}]
  ↓
MCP tool schemas
  ↓
Step types summary (references planning format)
  ↓
Output format: JSON patch object with optional fields:
  - add: [<steps>]
  - add_nodes: [<nodes with id>] (for control flow)
  - add_edges: [{from, to, output}]
  - remove_node_ids: [<ids>]
  - update: [{node_id, name, node_type}]
  ↓
Patch rules (only changed fields, valid IDs, keep flow functional)
```

User message: `"Modify the workflow: <user_prompt>"`

### Assistant Prompt (`assistant_system_prompt`)

Delegates to planner or patcher prompt based on workflow state:

- **Empty workflow** → wraps `planner_system_prompt` with conversational preamble
- **Non-empty workflow** → wraps `patcher_system_prompt` with conversational preamble + instruction to respond conversationally when no changes are needed

Both variants append `run_context` (execution results summary) when available.

Message assembly in `assistant_chat_with_backend`:

```
1. System prompt (planner or patcher variant)
2. Summary context (if available): injected as user + assistant exchange
3. Recent conversation window (last 5 exchanges = 10 messages)
4. New user message
```

### AI-Step Runtime Prompt (`workflow_system_prompt` + `build_step_prompt`)

Used at execution time for `AiStep` nodes. Lives in `client.rs`.

System prompt:
```
Role: "You are a UI automation assistant executing an AI Step node."
  ↓
Available MCP tool descriptions (abbreviated)
  ↓
VLM_IMAGE_SUMMARY format documentation
  ↓
Strategy guidance (screenshot → find → act → verify)
  ↓
Completion signal: "STEP_COMPLETE" when done
```

User message built by `build_step_prompt`:
```
<prompt text>
[Button to find: "<button_text>"]     (optional)
[Image to find: <template_path>]      (optional)
```

## Planner Pipeline (`plan_workflow`)

1. Build planner prompt
2. LLM call via `chat_with_repair`
3. `extract_json()` (`planner/parse.rs`)
4. Parse graph or flat output
5. `parse_lenient` + feature filtering
6. `PlanStep -> NodeType` mapping
7. Edge build + control-flow inference
8. `validate_workflow()`
9. Return workflow + warnings

## Patcher Pipeline (`patch_workflow`)

1. Build patcher prompt
2. LLM call via `chat_with_repair`
3. Parse `PatcherOutput`
4. Build patch via lenient add/update/remove parsing
5. Return patch + warnings

## Assistant Pipeline (`assistant_chat`)

1. Build conversation messages (summary + recent window + new user message)
2. Call LLM
3. Parse response to patch/plan/conversation
4. If patch and validation enabled: merge + validate + retry loop
5. Return assistant text, optional patch, warnings, optional summary update

## Key Files

| File | Role |
|------|------|
| `crates/clickweave-llm/src/planner/prompt.rs` | planner, patcher, and assistant system prompts |
| `crates/clickweave-llm/src/client.rs` | AI-step runtime prompt (`workflow_system_prompt`, `build_step_prompt`) |
| `crates/clickweave-llm/src/planner/repair.rs` | one-shot repair retry wrapper |
| `crates/clickweave-llm/src/planner/assistant.rs` | assistant retry loop + patch merge validation |
| `crates/clickweave-llm/src/planner/plan.rs` | planner entrypoint and workflow build |
| `crates/clickweave-llm/src/planner/patch.rs` | patcher entrypoint |
| `crates/clickweave-llm/src/planner/mod.rs` | lenient parsing, patch build, control-flow inference |
| `crates/clickweave-llm/src/planner/parse.rs` | JSON extraction and layout helpers |
| `crates/clickweave-core/src/validation.rs` | workflow structural validation |
| `ui/src/store/settings.ts` | settings persistence (`maxRepairAttempts`) |
