# Node Checks

Per-node validation that runs after a workflow completes. A VLM evaluates whether each checked node produced the expected result by examining trace events and a post-execution screenshot.

## How It Works

### 1. Defining Checks

Each workflow node has two optional verification mechanisms on its **Setup** and **Checks** tabs:

**Typed checks** (Checks tab) — structured assertions:
- `TextPresent` — verify specific text is visible on screen
- `TextAbsent` — verify specific text is NOT visible
- `TemplateFound` — verify a visual template/image is found
- `WindowTitleMatches` — verify the window title matches expected value

Each check has:
- `name` — descriptive label (e.g. "Calculator shows result")
- `check_type` — one of the four types above
- `params` — type-specific parameters (JSON)
- `on_fail` — `FailNode` (hard failure, stops evaluation) or `WarnOnly` (soft, continues)

**Expected outcome** (Setup tab) — free-text description of what should happen after the node runs (e.g. "The calculator display should show 128"). Evaluated as an additional check alongside typed checks.

### 2. During Execution

After each node completes successfully, if it has checks or an expected outcome:
1. A **screenshot** is captured via the MCP `take_screenshot` tool
2. The screenshot is saved as an artifact in the node's run directory
3. The node is added to the `completed_checks` queue for post-run evaluation

### 3. Post-Workflow Evaluation Pass

After **all nodes finish**, a single evaluation pass runs:

1. **Deduplication** — for nodes that ran multiple times (e.g. inside loops), only the last execution is evaluated
2. **Evidence gathering** — for each checked node:
   - Trace summary: last 20 `tool_call`/`tool_result` events from `events.jsonl`
   - Screenshot: base64 PNG captured after the node ran
3. **VLM evaluation** — each node's checks are sent to the VLM with:
   - System prompt instructing it to evaluate UI automation results
   - Node name, check descriptions, trace events, and screenshot
   - VLM responds with JSON: `[{"check_name": "...", "verdict": "pass"|"fail", "reasoning": "..."}]`
4. **Verdict resolution** — VLM responses are matched against original checks, `on_fail` policy applied (fail demoted to warn for `WarnOnly`)
5. **Short-circuit** — if any node has a hard failure (`Fail`), evaluation stops immediately

### 4. Results

- Saved to `verdict.json` in each node's run directory
- Emitted as `executor://checks_completed` event to the frontend
- Displayed in the **VerdictBar** at the top of the app:
  - Green: PASSED (all checks pass)
  - Yellow: PASSED with warnings (some `WarnOnly` checks failed)
  - Red: FAILED (at least one `FailNode` check failed)
  - Expandable to show per-node breakdowns with individual check verdicts and VLM reasoning

## Key Files

| File | Role |
|------|------|
| `crates/clickweave-engine/src/executor/check_eval.rs` | VLM prompt construction, response parsing, verdict resolution |
| `crates/clickweave-engine/src/executor/run_loop.rs` | Screenshot capture, check collection, evaluation pass orchestration |
| `crates/clickweave-core/src/workflow.rs` | Core types: `Check`, `CheckType`, `CheckResult`, `CheckVerdict`, `NodeVerdict`, `OnCheckFail` |
| `crates/clickweave-core/src/storage.rs` | `save_node_verdict()` — persists `verdict.json` per node |
| `ui/src/components/VerdictBar.tsx` | Verdict display with expandable per-node details |
| `ui/src/components/node-detail/tabs/ChecksTab.tsx` | UI for adding/removing checks on a node |
| `ui/src/store/slices/verdictSlice.ts` | Zustand state for verdicts |

## Limitations

- Checks are **manual** — the user must add them per node. Nothing is auto-generated from the original prompt.
- The VLM backend defaults to the agent's LLM but can be overridden with a dedicated VLM (`self.vlm`).
- Screenshot capture uses the currently focused app; if focus shifted unexpectedly, the screenshot may not show the right window.
- Loop nodes are deduplicated to the last iteration only — earlier iterations are not evaluated.
