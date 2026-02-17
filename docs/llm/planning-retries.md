# Planning & LLM Retry Logic

Clickweave uses LLMs to generate and modify workflows. Because LLM output is inherently unpredictable, multiple layers of retry and repair logic ensure robust handling of malformed, invalid, or structurally broken output.

## Overview

There are three distinct retry mechanisms, each operating at a different level:

| Mechanism | Scope | Max Retries | Trigger |
|-----------|-------|-------------|---------|
| **JSON repair** (`chat_with_repair`) | Planner, Patcher | 1 | JSON parse failure or structural error |
| **Validation retry** (assistant loop) | Assistant chat | Configurable (default: 3) | Patch fails `validate_workflow()` |
| **Lenient parsing** (skip malformed) | All plan/patch paths | N/A | Individual nodes/edges/steps malformed |

## 1. JSON Repair (One-Shot Retry)

**File:** `crates/clickweave-llm/src/planner/repair.rs`

The `chat_with_repair()` function wraps any LLM call with a single retry attempt when the output can't be processed.

### How It Works

```
1. Send messages to LLM
2. Process the response with a caller-provided function
3. If processing fails:
   a. Append the assistant's response to messages
   b. Append error feedback as a user message:
      "Your previous output had an error: <error>
       Please fix the JSON and try again. Output ONLY the corrected JSON object."
   c. Call LLM again with the extended conversation
   d. Process the new response
4. If the retry also fails → return the error
```

### Used By

- **`plan_workflow_with_backend()`** — wraps the entire plan parsing pipeline (JSON extraction, step mapping, graph building, validation)
- **`patch_workflow_with_backend()`** — wraps the entire patch parsing pipeline

### Design Rationale

One retry is sufficient because the error feedback gives the LLM concrete information about what went wrong. Common self-correctable errors:
- Trailing commas in JSON
- Missing required fields
- Malformed nested objects
- Wrong key names

The repair prompt asks for "ONLY the corrected JSON object" to discourage the LLM from adding conversational text that would break parsing.

## 2. Validation Retry (Assistant Chat)

**File:** `crates/clickweave-llm/src/planner/assistant.rs`

The assistant chat has its own retry loop that catches patches which parse correctly but produce structurally invalid workflows.

### How It Works

```
attempt = 0
loop:
  1. Call LLM with conversation messages
  2. Parse response (patch/plan/conversation)
  3. If a patch was produced AND max_repair_attempts > 0:
     a. Merge patch into existing workflow (simulating frontend apply)
     b. Run validate_workflow() on the merged result
     c. If validation fails AND attempts remain:
        - Append assistant response + error message to conversation
        - Increment attempt counter
        - Continue loop (retry)
     d. If validation fails AND attempts exhausted:
        - Log warning, return patch as-is (let frontend handle)
  4. Return result
```

### Configuration

The `max_repair_attempts` parameter controls behavior:

| Value | Behavior |
|-------|----------|
| `0` | Skip validation entirely — return whatever the LLM produces |
| `1` | Validate but don't retry — return even if invalid |
| `2` | Validate + 1 retry attempt |
| `3` (default) | Validate + up to 2 retry attempts |
| `N` | Validate + up to N-1 retry attempts |

The setting is configurable in the UI (Settings panel) and persisted in `settings.json`.

### Error Feedback Format

When validation fails, the retry message includes the specific validation error:

```
Your previous output produced a patch that fails validation: <validation_error>

Please fix the JSON output so the resulting workflow is valid.
```

This gives the LLM context about structural issues like:
- Missing IfFalse edge on an If node
- Cycle detected outside a loop
- Unknown switch case name
- Orphan nodes with no edges

### Merge Simulation

Before validating, the assistant simulates applying the patch to the existing workflow using `merge_patch_into_workflow()`. This mirrors the frontend's `applyPendingPatch` logic:

1. Remove nodes listed in `removed_node_ids`
2. Apply updates from `updated_nodes`
3. Add new nodes from `added_nodes`
4. Remove edges from `removed_edges`
5. Add new edges from `added_edges`

This ensures validation runs against the complete post-patch workflow, not just the patch in isolation.

## 3. Lenient Parsing (Skip Malformed)

**File:** `crates/clickweave-llm/src/planner/parse.rs`

Rather than failing on the first malformed element, the parser skips bad items and collects warnings.

### Mechanisms

**`parse_lenient<T>(items: &[Value]) -> (Vec<T>, Vec<String>)`**

Attempts to deserialize each item individually. Items that fail deserialization are skipped with a warning like:

```
"Step 3 malformed: missing field `tool_name`"
```

**Step rejection by feature flags:**

Steps that don't match enabled feature flags are removed with warnings:

```
"Planner step removed: AiTransform steps disabled"
"Planner step removed: Agent steps disabled"
```

**Unknown step types:**

The `PlanStep` enum uses `#[serde(other)]` to deserialize unknown step types as `PlanStep::Unknown`, which are then silently skipped during node construction.

**Malformed edges:**

Edges with unknown `output` types or referencing non-existent node IDs are skipped with warnings.

### Result

The workflow is built from whatever valid nodes and edges survived parsing. Warnings are propagated to the frontend and displayed to the user. If zero valid nodes survive, the entire operation fails.

## Interaction Between Layers

The three mechanisms work together in a pipeline:

```
LLM Response
    │
    ├── Layer 3: Lenient parsing
    │   Skip malformed nodes/edges, collect warnings
    │
    ├── Layer 2: Structural validation
    │   validate_workflow() on complete graph
    │   → retry if invalid (assistant) or fail (planner)
    │
    └── Layer 1: JSON repair
        One-shot retry on total parse failure (planner/patcher)
```

For the **planner** (`plan_workflow`):
```
chat_with_repair(backend, messages, |content| {
    extract_json(content)           // find JSON in response
    parse graph or flat format      // lenient parsing
    map steps to NodeType           // skip unknown tools
    build edges                     // infer control flow
    validate_workflow()             // structural check
})
// If any step fails → repair retry (1 attempt)
```

For the **assistant** (`assistant_chat`):
```
loop {
    call LLM
    parse_assistant_response()      // lenient parsing
    if patch produced:
        merge_patch_into_workflow() // simulate apply
        validate_workflow()         // structural check
        if invalid && attempts left → retry with error
    return result
}
```

## Control Flow Edge Inference

A notable post-processing step occurs between parsing and validation. The `infer_control_flow_edges()` function fixes common LLM mistakes with control flow:

1. **Unlabeled If edges:** If an If node has two unlabeled outgoing edges, the first is labeled `IfTrue` and the second `IfFalse`
2. **Unlabeled Loop edges:** First outgoing edge from Loop becomes `LoopBody`, second becomes `LoopDone`
3. **Back-edge rerouting:** If a body node points directly back to a Loop node, the edge is rerouted through the EndLoop node instead (prevents cycle validation failures)
4. **EndLoop pairing:** In flat plans, EndLoop nodes are paired with Loop nodes by nesting order

These heuristics handle the most common LLM output patterns and make the plan robust even when the LLM doesn't perfectly follow the schema.

## Key Files

| File | Role |
|------|------|
| `crates/clickweave-llm/src/planner/repair.rs` | `chat_with_repair()` — one-shot JSON repair |
| `crates/clickweave-llm/src/planner/assistant.rs` | Assistant validation retry loop, `merge_patch_into_workflow()` |
| `crates/clickweave-llm/src/planner/plan.rs` | `plan_workflow_with_backend()` — planner entry point |
| `crates/clickweave-llm/src/planner/patch.rs` | `patch_workflow_with_backend()` — patcher entry point |
| `crates/clickweave-llm/src/planner/parse.rs` | `extract_json()`, `parse_lenient()`, layout, step filtering |
| `crates/clickweave-llm/src/planner/mapping.rs` | `step_to_node_type()` — PlanStep to NodeType conversion |
| `crates/clickweave-core/src/validation.rs` | `validate_workflow()` — structural validation rules |
| `ui/src/store/settings.ts` | `maxRepairAttempts` persistence |
