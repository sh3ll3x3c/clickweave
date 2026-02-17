# Frontend Architecture (Conceptual)

The frontend is a workflow editor plus execution cockpit.

## Primary UX Surfaces

- Graph canvas for building and wiring workflow nodes.
- Node detail panel for setup, checks, and trace inspection.
- Assistant panel for conversational edits and patch proposals.
- Run/log/verdict surfaces for execution feedback.

## State Philosophy

A single store with slices keeps cross-feature coordination simple:

- project/workflow editing,
- execution state,
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
