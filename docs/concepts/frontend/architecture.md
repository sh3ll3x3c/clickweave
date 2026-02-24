# Frontend Architecture (Conceptual)

The frontend is a workflow editor plus execution cockpit.

## Primary UX Surfaces

- Graph canvas for building and wiring workflow nodes.
- Node detail panel for setup, checks, and trace inspection.
- Assistant panel for conversational edits and patch proposals.
- Run/log/verdict surfaces for execution feedback.
- Supervision modal for human-in-the-loop review during Test runs. When a step fails verification the engine pauses and the modal shows the node name, a finding description, and an optional screenshot. The user can retry the step, skip past it, or abort the entire run.

## Execution Modes

Workflows can be launched in two modes, selectable from the toolbar:

- **Test** -- the engine verifies each step after it executes by taking a screenshot and evaluating the result. If verification fails the supervision modal pauses the run for human review. On completion the engine saves a decision cache so subsequent runs can replay known-good choices faster.
- **Run** -- the engine executes steps without per-step supervision, running straight through to completion. This is the production-like mode used once a workflow has been verified in Test mode.

The current mode is stored in execution state and sent to the backend as part of the run request, so the frontend never needs to know the details of what the engine skips -- it simply reacts to whichever events the backend emits.

## State Philosophy

A single store with slices keeps cross-feature coordination simple:

- project/workflow editing,
- execution state (run status, current mode, supervision pause),
- undo/redo history (snapshotted workflow states for reversible edits),
- assistant conversation and pending patches,
- settings,
- logs/verdicts,
- UI chrome/selection state.

## Event-Driven Runtime UX

Backend events stream into the store, and UI updates are derived from state rather than direct imperative DOM updates.

## Why This Matters

- Graph editing stays responsive while execution runs.
- Assistant changes are staged as patches before apply.
- Trace/artifact views make failures debuggable without leaving the app.

For exact file/component/state contracts, see `docs/reference/frontend/architecture.md`.
