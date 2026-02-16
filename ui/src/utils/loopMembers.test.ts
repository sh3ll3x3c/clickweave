import { describe, it, expect } from "vitest";
import { computeLoopMembers } from "./loopMembers";
import type { Node, Edge } from "../bindings";

// Minimal node factory -- only fields the function uses
function node(
  id: string,
  type: string,
  params?: Record<string, unknown>,
): Node {
  return {
    id,
    node_type: { type, ...params } as Node["node_type"],
    position: { x: 0, y: 0 },
    name: id,
    enabled: true,
    timeout_ms: null,
    settle_ms: null,
    retries: 0,
    trace_level: "Full",
    expected_outcome: null,
    checks: [],
  };
}

function edge(from: string, to: string, output?: Edge["output"]): Edge {
  return { from, to, output: output ?? null };
}

describe("computeLoopMembers", () => {
  it("returns empty map when there are no loops", () => {
    const nodes = [node("a", "AiStep"), node("b", "Click")];
    const edges = [edge("a", "b")];
    expect(computeLoopMembers(nodes, edges)).toEqual(new Map());
  });

  it("finds body nodes between Loop and EndLoop", () => {
    // Loop -> A -> B -> EndLoop
    const nodes = [
      node("loop1", "Loop", {
        exit_condition: { type: "Always" },
        max_iterations: 3,
      }),
      node("a", "AiStep"),
      node("b", "Click"),
      node("end1", "EndLoop", { loop_id: "loop1" }),
    ];
    const edges = [
      edge("loop1", "a", { type: "LoopBody" }),
      edge("a", "b"),
      edge("b", "end1"),
      edge("end1", "loop1"),
    ];
    const result = computeLoopMembers(nodes, edges);
    expect(result.get("loop1")).toEqual(expect.arrayContaining(["a", "b"]));
    expect(result.get("loop1")).toHaveLength(2);
  });

  it("handles empty loop (Loop -> EndLoop directly)", () => {
    const nodes = [
      node("loop1", "Loop", {
        exit_condition: { type: "Always" },
        max_iterations: 3,
      }),
      node("end1", "EndLoop", { loop_id: "loop1" }),
    ];
    const edges = [
      edge("loop1", "end1", { type: "LoopBody" }),
      edge("end1", "loop1"),
    ];
    const result = computeLoopMembers(nodes, edges);
    expect(result.get("loop1")).toEqual([]);
  });

  it("handles branching inside a loop (If node)", () => {
    // Loop -> If -> A (true) / B (false) -> EndLoop
    const nodes = [
      node("loop1", "Loop", {
        exit_condition: { type: "Always" },
        max_iterations: 3,
      }),
      node("if1", "If", { condition: { type: "Always" } }),
      node("a", "AiStep"),
      node("b", "Click"),
      node("end1", "EndLoop", { loop_id: "loop1" }),
    ];
    const edges = [
      edge("loop1", "if1", { type: "LoopBody" }),
      edge("if1", "a", { type: "IfTrue" }),
      edge("if1", "b", { type: "IfFalse" }),
      edge("a", "end1"),
      edge("b", "end1"),
      edge("end1", "loop1"),
    ];
    const result = computeLoopMembers(nodes, edges);
    expect(result.get("loop1")).toEqual(
      expect.arrayContaining(["if1", "a", "b"]),
    );
    expect(result.get("loop1")).toHaveLength(3);
  });

  it("handles nested loops", () => {
    // OuterLoop -> InnerLoop -> A -> InnerEndLoop -> OuterEndLoop
    const nodes = [
      node("outer", "Loop", {
        exit_condition: { type: "Always" },
        max_iterations: 3,
      }),
      node("inner", "Loop", {
        exit_condition: { type: "Always" },
        max_iterations: 3,
      }),
      node("a", "AiStep"),
      node("innerEnd", "EndLoop", { loop_id: "inner" }),
      node("outerEnd", "EndLoop", { loop_id: "outer" }),
    ];
    const edges = [
      edge("outer", "inner", { type: "LoopBody" }),
      edge("inner", "a", { type: "LoopBody" }),
      edge("a", "innerEnd"),
      edge("innerEnd", "inner"),
      edge("inner", "outerEnd", { type: "LoopDone" }),
      edge("outerEnd", "outer"),
    ];
    const result = computeLoopMembers(nodes, edges);
    // Outer loop body contains inner loop, its body, and its EndLoop
    expect(result.get("outer")).toEqual(
      expect.arrayContaining(["inner", "a", "innerEnd"]),
    );
    // Inner loop body contains just A
    expect(result.get("inner")).toEqual(["a"]);
  });
});
