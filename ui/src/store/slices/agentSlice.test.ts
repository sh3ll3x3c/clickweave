import { describe, it, expect, vi, beforeEach } from "vitest";

// Tauri's `invoke` must be mocked before agentSlice is imported — the
// slice captures the imported binding at module init time.
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useStore } from "../useAppStore";

describe("agentSlice.startAgent", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useStore.getState().resetAgent();
  });

  it("surfaces AlreadyRunning rejections into agentStatus=error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "AlreadyRunning",
      message: "Already running",
    });

    await useStore.getState().startAgent("do something");

    const state = useStore.getState();
    expect(state.agentStatus).toBe("error");
    expect(state.agentError).toBe("Already running");
  });

  it("surfaces string-serialized AlreadyRunning rejections into agentStatus=error", async () => {
    invokeMock.mockRejectedValueOnce("AlreadyRunning: Already running");

    await useStore.getState().startAgent("do something");

    const state = useStore.getState();
    expect(state.agentStatus).toBe("error");
    expect(state.agentError).toMatch(/already running/i);
  });

  it("stays in running state when invoke succeeds", async () => {
    invokeMock.mockResolvedValueOnce(undefined);

    await useStore.getState().startAgent("do something else");

    const state = useStore.getState();
    expect(state.agentStatus).toBe("running");
    expect(state.agentError).toBeNull();
  });

  it("clears the previous agentRunId before invoke so stale events are treated as stale", async () => {
    useStore.getState().setAgentRunId("run-prior");
    invokeMock.mockResolvedValueOnce(undefined);

    await useStore.getState().startAgent("fresh goal");

    expect(useStore.getState().agentRunId).toBeNull();
  });
});

describe("agentSlice approval actions", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useStore.getState().resetAgent();
  });

  it("formats structured Tauri errors from approveAction into the activity log", async () => {
    useStore.getState().setPendingApproval({
      stepIndex: 0,
      toolName: "click",
      arguments: {},
      description: "Click the button",
    });
    invokeMock.mockRejectedValueOnce({
      kind: "Validation",
      message: "No pending approval request",
    });

    await useStore.getState().approveAction();

    const lastLog = useStore.getState().logs.at(-1);
    expect(lastLog).toContain("No pending approval request");
    expect(lastLog).not.toContain("[object Object]");
  });

  it("formats structured Tauri errors from rejectAction into the activity log", async () => {
    useStore.getState().setPendingApproval({
      stepIndex: 0,
      toolName: "click",
      arguments: {},
      description: "Click the button",
    });
    invokeMock.mockRejectedValueOnce({
      kind: "Validation",
      message: "Approval channel closed — agent task may have ended",
    });

    await useStore.getState().rejectAction();

    const lastLog = useStore.getState().logs.at(-1);
    expect(lastLog).toContain("Approval channel closed");
    expect(lastLog).not.toContain("[object Object]");
  });
});
