# Architecture Overview (Conceptual)

Clickweave has one core idea: describe desktop automation as a graph, then execute that graph reliably with observable state.

## Mental Model

1. A user starts with intent.
2. The planner turns intent into a workflow graph.
3. The user reviews/edits the graph visually.
4. The executor walks the graph and drives tools.
5. Results, traces, and checks feed back into iteration.

## Layered System

- Core model layer: workflow graph types and validation rules.
- Planning layer: translates natural language into graph structures.
- Execution layer: runs deterministic tool steps and AI-agentic steps.
- Integration layer: MCP bridge to external automation tools.
- UI layer: canvas editor + run/trace/assistant UX.

## Why This Split Exists

- Deterministic nodes make runs inspectable and replayable.
- Agentic nodes handle ambiguity when strict tool calls are not enough.
- Control-flow nodes let workflows encode decision logic without scripting.
- Trace + checks create a feedback loop for reliability.

## Reliability Strategy

The system assumes LLM output and runtime environments are imperfect, so it uses:

- graph validation before execution,
- retry/repair loops for planning and patching,
- runtime retries with cache eviction,
- persisted traces and artifacts for diagnosis.

## What Humans Should Keep in Mind

- A workflow is not a script text file; it is a typed graph.
- Planning and execution are separate responsibilities.
- "Success" is not only node completion; checks and observed outcomes matter.
- The assistant is for acceleration, not bypassing validation.

For code-coupled details, see `docs/reference/architecture/overview.md`.
