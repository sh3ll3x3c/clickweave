# Workflow Execution (Conceptual)

Execution is a graph walk with guardrails.

## How to Think About It

- The executor advances node by node.
- Control-flow nodes choose the next path.
- Action nodes call tools or run an agentic loop.
- Every meaningful step emits traceable evidence.

## Node Execution Strategies

Each action node uses one of two strategies:

1. **Deterministic action:** one node maps to one concrete tool operation (click, type, launch app, take screenshot, etc.).
2. **Agentic step:** one node runs a short LLM-plus-tools conversation that can span multiple tool calls before producing a result.

Control-flow nodes (If, Switch, Loop, EndLoop) do not execute actions themselves -- they evaluate conditions and route the graph.

## Runtime Modes: Test and Run

The executor has two runtime modes, chosen before execution begins:

- **Test** -- the interactive authoring mode. The executor runs each node, then verifies its effect through per-step supervision (see below). LLM decisions (element disambiguation, app resolution) are recorded into a decision cache so they can be replayed later. The decision cache is saved to disk when the workflow completes.
- **Run** -- the headless replay mode. Supervision is skipped. Previously cached LLM decisions are replayed deterministically, so elements and apps resolve the same way they did during the Test run without repeating LLM calls. If a cached decision no longer matches the live UI (e.g., the resolved element name is missing from the accessibility tree), the executor falls through to the LLM for a fresh resolution.

This Test-then-Run workflow means a workflow is authored once with human oversight, then executed repeatedly without it.

## Per-Step Supervision (Test Mode)

In Test mode, every action node is verified immediately after execution:

1. **Screenshot** -- the executor captures a window screenshot of the focused app.
2. **VLM description** -- a vision-language model describes what the screen shows relative to the action that just ran.
3. **LLM judge** -- a planner-class LLM receives the VLM description along with the full conversation history of prior steps and returns a pass/fail verdict with reasoning.

If the step passes, execution continues. If it fails, the executor pauses and presents the finding (plus screenshot) to the user, who can choose:

- **Retry** -- re-execute the node from scratch.
- **Skip** -- accept the current state and move on.
- **Abort** -- stop the workflow.

The supervision conversation history is persistent across the entire run, so the judge accumulates context about what the workflow has done so far.

Nodes inside a loop skip per-step supervision during iterations. Instead, the loop's outcome is verified once after the loop exits, since individual steps (clicks, keypresses) are only meaningful in aggregate.

## Focused App Tracking

The executor tracks which application is currently in focus. When a `launch_app` or `focus_window` (by app name) action runs, the resolved app name is stored as the focused app. This scoping is used throughout execution:

- **Screenshots** are captured as window-scoped screenshots of the focused app rather than full-screen captures.
- **find_text** and **click** operations use the focused app to scope accessibility queries, avoiding false matches from other windows.
- **Supervision** screenshots target the focused app window for accurate verification.

## Why Loops Are Do-While

Desktop automation often needs "try once, inspect, retry." Do-while semantics guarantee the body runs once before checking exit criteria.

## Control Flow

- **If** -- evaluates a condition and follows either the true or false branch.
- **Switch** -- evaluates multiple case conditions in order and follows the first match. If no case matches, follows the default edge. If no default edge exists, the workflow path ends at that node.
- **Loop** -- do-while semantics. The body always executes at least once. After each iteration, the exit condition is checked; if met (or max iterations reached), execution follows the "done" edge. A pending loop exit triggers deferred supervision verification in Test mode.
- **EndLoop** -- marks the end of a loop body. Jumps back to the corresponding Loop node to re-evaluate the exit condition.

## Reliability Principles

- Retry failed nodes a bounded number of times.
- Evict resolution caches for the specific node being retried (app name, element name), not the entire cache -- other nodes' cached resolutions remain valid.
- Capture traces and artifacts per run so failures are diagnosable.

## Post-Workflow Check Evaluation

Nodes can carry checks (assertions about expected outcomes). These are not evaluated inline during execution. Instead, after the graph walk completes, the executor runs a separate check-evaluation pass: it gathers the trace summary and post-node screenshot for each checked node, sends them to the VLM, and produces pass/fail verdicts. This is distinct from per-step supervision -- supervision verifies that each step took effect; check evaluation verifies that the workflow produced the right business-level outcomes.

## Runtime Context

Node outputs become variables that later branches can read. This turns workflow graphs into stateful execution plans without introducing a scripting language.

For exact runtime behavior and file-level references, see `docs/reference/engine/execution.md`.
