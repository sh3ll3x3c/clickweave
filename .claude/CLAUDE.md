## Run Trace Logs

Workflow dir roots (sanitized workflow name, lowercase with dashes):
- **Saved projects:** `<project>/.clickweave/runs/<workflow_name>/`
- **Unsaved projects (macOS):** `~/Library/Application Support/com.clickweave.app/runs/<workflow_name>_<short_uuid>/`
- **Unsaved projects (Windows):** `%APPDATA%\Clickweave\runs\<workflow_name>_<short_uuid>\`
- **Unsaved projects (Linux):** `$XDG_DATA_HOME/clickweave/runs/<workflow_name>_<short_uuid>/`

Layout under the workflow dir:
```
<workflow_dir>/
  decisions.json            ← workflow-level decision cache
  agent_cache.json          ← workflow-level agent decision cache (persists across runs)
  variant_index.jsonl       ← workflow-level variant index (one line per execution)
  <execution_dir>/          ← YYYY-MM-DD_HH-MM-SS_<short_uuid>, one per workflow execution
    events.jsonl            ← execution-level events: agent step events (step_completed, step_failed) + control-flow (branch_evaluated, loop_iteration)
    <node_name>/            ← sanitized node name (e.g. launch-calculator/)
      run.json              ← run metadata (status, timestamps, trace_level)
      events.jsonl          ← node-level trace events (node_started, tool_call, tool_result)
      verdict.json          ← optional, written by save_node_verdict (check outcome)
      artifacts/             ← output artifacts from this node run
```

- Authoritative source: `crates/clickweave-core/src/storage.rs` (`RunStorage`)
- **When debugging runtime issues**, always check the most recent run logs first. Start with the execution-level `events.jsonl` for the agent/step-level narrative, then drop into each `<node_name>/events.jsonl` for tool-level detail.
- If an execution dir has no `<node_name>/` subdir, the agent failed before any node run was created — the exec-level `events.jsonl` is the only trace.

## Walkthrough Session Logs
- **Saved projects:** `<project>/.clickweave/walkthroughs/<session_dir>/`
- **Unsaved projects (macOS):** `~/Library/Application Support/com.clickweave.app/walkthroughs/<session_dir>/`
- **Unsaved projects (Windows):** `%APPDATA%\Clickweave\walkthroughs\<session_dir>\`
- **Unsaved projects (Linux):** `$XDG_DATA_HOME/clickweave/walkthroughs/<session_dir>/`
- `session.json` — session metadata
- `events.jsonl` — raw walkthrough events
- `actions.json` — extracted actions
- `draft.json` — generated workflow draft
- `artifacts/` — screenshots and other captured artifacts

## Application Logs
- **macOS:** `~/Library/Logs/Clickweave/clickweave.YYYY-MM-DD.txt`
- **Windows:** `%LOCALAPPDATA%\Clickweave\logs\clickweave.YYYY-MM-DD.txt`
- **Linux:** `$XDG_DATA_HOME/clickweave/logs/clickweave.YYYY-MM-DD.txt` (fallback: `~/.local/share/clickweave/logs/`)
- JSON-formatted, daily rotation
- Configured in `src-tauri/src/main.rs` (`log_dir()` + tracing subscriber setup)

## Reference Docs
- `docs/reference/` — **read these first** when exploring a subsystem, before doing broad searches
  - `architecture/overview.md` — crate structure, dependency graph, module tables, IPC commands, event contract
  - `engine/execution.md` — agent loop, tool dispatch, context compaction, retries
  - `frontend/architecture.md` — React stack, directory layout, Zustand slices, graph editor behavior
  - `mcp/integration.md` — MCP client lifecycle, tool mapping, protocol types

## Design Docs & Implementation Plans
- **Design docs** (durable decision record) live in a separate private repo — see `.claude/issues.local.md` for the path convention. Do not commit design docs to this public repo.
- **Implementation plans** (ephemeral guidance for the coding agent): `internal_docs/plans/` (gitignored, local-only), named `YYYY-MM-DD_HH-MM-SS-<topic>.md`. Scoped to one execution, not a durable artifact.
- **Design reviews** (Codex review output on drafts): `internal_docs/design-reviews/` (gitignored).

## Issue Conventions
- **Issues repo is separate from this code repo.** Do not file issues against this repo.
- **Private specifics** (target issues repo, project board, canonical labels) live in `.claude/issues.local.md` (gitignored). Read that before creating any issue.
- **Sub-issue linkage:** add `Parent: #NN` in the child body.
- **Public-PR hazard:** never reference private issue numbers in PRs against this public repo. Link from the private issue to the PR instead.

## Rust Development

### Code Style
- Follow the Rust style guide as outlined in [rustfmt](https://github.com/rust-lang/rustfmt)
- Use 4 spaces for indentation
- Sort imports alphabetically

### Tooling
- Use `cargo clippy` for linting
- Use `cargo fmt` for formatting

### Error Handling
- Use Result types that are used in the file you're editing for functions that can fail
- Avoid using `unwrap()` or `expect()` in production code

### Build & Run
```bash
cargo build
cargo run
```

### Preferred Patterns
- Use traits for polymorphism
- Leverage Rust's ownership system for memory safety
- Use iterators and closures for data transformation
- Pin dependency versions for reproducible builds
