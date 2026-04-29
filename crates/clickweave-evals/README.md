# Clickweave Evals

Prompt-eval harness for the state-spine agent.

The harness is intentionally separate from unit tests. It runs synthetic
scenario fixtures through the real `StateRunner`, accepts an optional agent
system-prompt candidate, scores the tool trajectory deterministically, and can
ask a fixed semantic judge prompt for trace-level feedback.

## Privacy Contract

- Scenario fixtures must be synthetic.
- Scenario IDs must use the `synthetic_` prefix; the loader rejects fixtures
  that are not explicitly marked synthetic.
- Real project paths, workflow names, screenshots, image bytes, secrets, and
  user content must not be committed as fixtures.
- The harness omits system-prompt bodies from trace logs and records only a
  prompt hash.
- Trace values are redacted before being written or sent to the judge.
- Judge input is sanitized JSON only; no raw screenshots or private files are
  passed through the eval path.

## Run

Run every synthetic scenario and produce an aggregate report:

```bash
cargo run -p clickweave-evals -- \
  --scenario-dir crates/clickweave-evals/scenarios \
  --agent-base-url http://localhost:1234/v1 \
  --agent-model local-model \
  --out target/clickweave-eval-suite.json
```

Run one scenario:

```bash
cargo run -p clickweave-evals -- \
  --scenario crates/clickweave-evals/scenarios/synthetic_electron_pre_cdp.json \
  --agent-prompt /path/to/candidate_agent_system.md \
  --agent-base-url http://localhost:1234/v1 \
  --agent-model local-model \
  --out /tmp/clickweave-eval.json
```

With semantic judging:

```bash
cargo run -p clickweave-evals -- \
  --scenario crates/clickweave-evals/scenarios/synthetic_electron_pre_cdp.json \
  --agent-prompt /path/to/candidate_agent_system.md \
  --agent-base-url http://localhost:1234/v1 \
  --agent-model local-agent-model \
  --judge-base-url http://localhost:1234/v1 \
  --judge-model codex-judge-model
```

The command also reads `CLICKWEAVE_EVAL_AGENT_BASE_URL`,
`CLICKWEAVE_EVAL_AGENT_MODEL`, `CLICKWEAVE_EVAL_AGENT_API_KEY`,
`CLICKWEAVE_EVAL_JUDGE_BASE_URL`, `CLICKWEAVE_EVAL_JUDGE_MODEL`, and
`CLICKWEAVE_EVAL_JUDGE_API_KEY`.

## Scenarios

- `synthetic_electron_pre_cdp` checks launch -> auto-CDP attach -> CDP click, and forbids manual reconnects after attach.
- `synthetic_wrong_cdp_port_runtime_owned` checks that the model does not manually connect to a guessed debug port when CDP acquisition should be app-scoped and runtime-owned.
- `synthetic_cdp_widen_surface` checks DOM widening via `cdp_take_dom_snapshot` or `cdp_evaluate_script` when visible CDP elements are insufficient.
- `synthetic_native_ax_required` checks native AX snapshot plus AX dispatch.
- `synthetic_ax_snapshot_expired` checks fresh AX snapshot recovery after `snapshot_expired`.
- `synthetic_no_progress_replan` checks structured probing followed by `agent_replan` when a target is absent.
- `synthetic_applicable_skill_invocation` checks `invoke_skill` selection when a synthetic `<applicable_skills>` block is surfaced.

Deterministic scoring distinguishes runner auto-actions from model-selected
tools. Use `required_tools` / `forbidden_tools` for the full MCP trace,
`allowed_error_tools` for expected recovery-trigger errors, and
`required_agent_tools`, `required_agent_tool_groups`,
`required_agent_tool_counts`, `forbidden_agent_tools`, and
`max_agent_tool_calls` for the assistant's tool calls.

## GEPA Loop

Treat the JSON report as the objective function output:

- `deterministic` is the hard execution score.
- `semantic_judge` explains failures and prompt risks when a judge model is
  configured.
- `final_score` is `deterministic` only without a judge, or an 80/20 blend of
  deterministic and semantic scores with a judge.

GEPA should mutate only the candidate prompt file, run this harness for every
synthetic scenario, and optimize against aggregate `final_score` plus the
judge's sanitized failure feedback.
