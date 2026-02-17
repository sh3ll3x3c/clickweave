# Workflow Execution (Conceptual)

Execution is a graph walk with guardrails.

## How to Think About It

- The executor advances node by node.
- Control-flow nodes choose the next path.
- Action nodes call tools or run an agentic loop.
- Every meaningful step emits traceable evidence.

## Three Execution Modes

1. Deterministic action: one node maps to one concrete tool operation.
2. Agentic step: one node can run a short LLM + tools conversation.
3. Control flow: nodes like If/Switch/Loop route the graph.

## Why Loops Are Do-While

Desktop automation often needs "try once, inspect, retry." Do-while semantics guarantee the body runs once before checking exit criteria.

## Reliability Principles

- Retry failed nodes a bounded number of times.
- Clear resolution caches between retries when environment may have changed.
- Capture traces/artifacts per run so failures are diagnosable.
- Evaluate checks after execution to convert raw outputs into verdicts.

## Runtime Context

Node outputs become variables that later branches can read. This turns workflow graphs into stateful execution plans without introducing a scripting language.

For exact runtime behavior and file-level references, see `docs/reference/engine/execution.md`.
