import type { Node as RFNode } from "@xyflow/react";
import { describe, expect, it, vi } from "vitest";
import type { Workflow } from "../../bindings";
import {
  AGENT_RUN_GROUP_PREFIX,
  projectAgentRunGroups,
  type AgentRunProjectionContext,
} from "./useRfNodeBuilder";

function workflowNode(id: string, runId: string | null = null) {
  return {
    id,
    name: id,
    node_type: { type: "CdpWait", text: "Ready", timeout_ms: 1000 },
    position: { x: 0, y: 0 },
    enabled: true,
    timeout_ms: null,
    settle_ms: null,
    retries: 0,
    trace_level: "Minimal",
    role: "Default",
    expected_outcome: null,
    source_run_id: runId,
  };
}

function workflow(nodes = [workflowNode("n1", "run-1")]): Workflow {
  return {
    id: "00000000-0000-0000-0000-000000000001",
    name: "wf",
    nodes,
    edges: [],
    groups: [],
  } as Workflow;
}

function rfNode(
  id: string,
  type = "workflow",
  parentId?: string,
): RFNode {
  return {
    id,
    type,
    parentId,
    position: { x: 20, y: 40 },
    data: {},
  };
}

function context(
  wf: Workflow,
  overrides: Partial<AgentRunProjectionContext> = {},
): AgentRunProjectionContext {
  return {
    workflow: wf,
    collapsedApps: new Set(),
    appGroups: new Map(),
    appGroupMeta: new Map(),
    nodeToUserGroup: new Map(),
    agentRunCollapsed: {},
    runTraces: {},
    toggleAgentRunCollapsed: vi.fn(),
    ...overrides,
  };
}

describe("projectAgentRunGroups", () => {
  it("wraps direct agent nodes with no app or user group", () => {
    const out = projectAgentRunGroups(
      [rfNode("n1")],
      context(workflow(), {
        runTraces: {
          "run-1": {
            runId: "run-1",
            phase: "executing",
            activeSubgoal: "",
            steps: [],
            worldModelDeltas: [],
            milestones: [],
            terminalFrame: { kind: "complete", detail: "Finished run" },
          },
        },
      }),
    );

    const group = out.find((node) => node.id === "agent-run-run-1");
    const child = out.find((node) => node.id === "n1");

    expect(group?.type).toBe("agent_run_group");
    expect(group?.data.summary).toBe("Finished run");
    expect(child?.parentId).toBe(`${AGENT_RUN_GROUP_PREFIX}run-1`);
    expect(child?.extent).toBe("parent");
  });

  it("wraps a same-run app group while preserving its inner children", () => {
    const groupId = "appgroup-n1";
    const out = projectAgentRunGroups(
      [rfNode(groupId, "appGroup"), rfNode("n1", "workflow", groupId), rfNode("n2", "workflow", groupId)],
      context(workflow([workflowNode("n1", "run-1"), workflowNode("n2", "run-1")]), {
        appGroups: new Map([[groupId, ["n1", "n2"]]]),
        appGroupMeta: new Map([
          [groupId, { appName: "Chrome", color: "#22c55e", anchorId: "n1" }],
        ]),
      }),
    );

    expect(out.find((node) => node.id === groupId)?.parentId).toBe(
      "agent-run-run-1",
    );
    expect(out.find((node) => node.id === "n1")?.parentId).toBe(groupId);
    expect(out.find((node) => node.id === "n2")?.parentId).toBe(groupId);
  });

  it("leaves mixed-source app groups unwrapped", () => {
    const groupId = "appgroup-n1";
    const out = projectAgentRunGroups(
      [rfNode(groupId, "appGroup"), rfNode("n1", "workflow", groupId), rfNode("n2", "workflow", groupId)],
      context(workflow([workflowNode("n1", "run-1"), workflowNode("n2", "run-2")]), {
        appGroups: new Map([[groupId, ["n1", "n2"]]]),
        appGroupMeta: new Map([
          [groupId, { appName: "Chrome", color: "#22c55e", anchorId: "n1" }],
        ]),
      }),
    );

    expect(out.some((node) => node.type === "agent_run_group")).toBe(false);
    expect(out.find((node) => node.id === groupId)?.parentId).toBeUndefined();
  });

  it("lets user-defined groups take precedence over agent-run wrapping", () => {
    const out = projectAgentRunGroups(
      [rfNode("n1")],
      context(workflow(), {
        nodeToUserGroup: new Map([["n1", "user-group-1"]]),
      }),
    );

    expect(out.some((node) => node.type === "agent_run_group")).toBe(false);
    expect(out.find((node) => node.id === "n1")?.parentId).toBeUndefined();
  });
});
