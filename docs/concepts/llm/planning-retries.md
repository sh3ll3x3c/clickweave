# Planning & Retries (Conceptual)

LLM output is probabilistic; workflow execution must be dependable. The planner therefore treats LLM output as input to be repaired, filtered, and validated.

## Strategy in Plain Terms

1. Try to parse what the model produced.
2. If structurally wrong, ask once for corrected JSON.
3. Keep valid pieces, skip malformed pieces, and surface warnings.
4. Validate the final graph before accepting it.
5. In assistant patch mode, retry with validation feedback when possible.

## Why Multiple Layers

A single retry mechanism is not enough:

- parsing issues need formatting repair,
- structural issues need validation-aware feedback,
- partial corruption should not discard the entire response.

## Product Outcome

Users still get fast AI-assisted planning, but unsafe graph states are blocked before execution.

For implementation-level behavior, see `docs/reference/llm/planning-retries.md`.
