# Planning & LLM Retry Logic (Reference)

Verified at commit: `0e907fc`

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
| `crates/clickweave-llm/src/planner/repair.rs` | one-shot repair retry wrapper |
| `crates/clickweave-llm/src/planner/assistant.rs` | assistant retry loop + patch merge validation |
| `crates/clickweave-llm/src/planner/plan.rs` | planner entrypoint and workflow build |
| `crates/clickweave-llm/src/planner/patch.rs` | patcher entrypoint |
| `crates/clickweave-llm/src/planner/mod.rs` | lenient parsing, patch build, control-flow inference |
| `crates/clickweave-llm/src/planner/parse.rs` | JSON extraction and layout helpers |
| `crates/clickweave-core/src/validation.rs` | workflow structural validation |
| `ui/src/store/settings.ts` | settings persistence (`maxRepairAttempts`) |
