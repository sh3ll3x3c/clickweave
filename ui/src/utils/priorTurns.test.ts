import { describe, expect, it } from "vitest";
import type { Workflow } from "../bindings";
import type { AssistantMessage } from "../store/slices/assistantSlice";
import { buildPriorTurns, REDACTED_SUMMARY } from "./priorTurns";

const wf = (agentNodes: Array<{ id: string; runId: string | null }>): Workflow =>
  ({
    id: "00000000-0000-0000-0000-000000000001",
    name: "",
    nodes: agentNodes.map((n) => ({
      id: n.id,
      source_run_id: n.runId,
    })) as unknown,
    edges: [],
    groups: [],
    next_id_counters: {},
    intent: null,
  }) as unknown as Workflow;

const pair = (runId: string, goal: string, summary: string): AssistantMessage[] => [
  { role: "user", content: goal, runId, timestamp: "t1" },
  { role: "assistant", content: summary, runId, timestamp: "t2" },
];

describe("buildPriorTurns", () => {
  it("returns empty when there are no user/assistant pairs", () => {
    expect(buildPriorTurns([], wf([]))).toEqual([]);
  });

  it("pairs user and assistant messages that share a runId", () => {
    const msgs = pair("r1", "send test", "done");
    const workflow = wf([{ id: "n1", runId: "r1" }]);
    const turns = buildPriorTurns(msgs, workflow);
    expect(turns).toEqual([
      { goal: "send test", summary: "done", run_id: "r1" },
    ]);
  });

  it("drops turns whose runId has no surviving agent nodes", () => {
    const msgs = pair("r1", "send test", "done");
    const workflow = wf([]); // all r1 nodes deleted
    expect(buildPriorTurns(msgs, workflow)).toEqual([]);
  });

  it("forwards summary verbatim when at least one runId node survives", () => {
    // The handler that triggers partial-delete redaction writes the
    // redacted string into messages via `mapMessagesByRunIds` — this
    // helper is just a pure filter. Given a non-redacted summary plus
    // surviving nodes, forward it unchanged.
    const msgs = pair("r1", "goal", "summary-of-complete-work");
    const workflow = wf([{ id: "n1", runId: "r1" }]);
    const turns = buildPriorTurns(msgs, workflow);
    expect(turns[0].summary).toBe("summary-of-complete-work");
  });

  it("ignores system annotations (no role=user, no runId)", () => {
    const msgs: AssistantMessage[] = [
      ...pair("r1", "goal", "summary"),
      { role: "system", content: "Deleted 1 node", timestamp: "t3" },
    ];
    const workflow = wf([{ id: "n1", runId: "r1" }]);
    expect(buildPriorTurns(msgs, workflow)).toHaveLength(1);
  });

  it("preserves chronological order when multiple turns survive", () => {
    const msgs = [
      ...pair("r1", "first", "done first"),
      ...pair("r2", "second", "done second"),
    ];
    const workflow = wf([
      { id: "n1", runId: "r1" },
      { id: "n2", runId: "r2" },
    ]);
    const turns = buildPriorTurns(msgs, workflow);
    expect(turns.map((t) => t.run_id)).toEqual(["r1", "r2"]);
  });

  it("exports the canonical redacted string used by the delete handler", () => {
    expect(REDACTED_SUMMARY).toBe("(partially deleted by user)");
  });
});
