import { describe, it, expect, vi, beforeEach } from "vitest";

// Tauri's `invoke` must be mocked before agentSlice is imported — the
// slice captures the imported binding at module init time.
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { useStore } from "../useAppStore";

describe("agentSlice.startAgent — already_running surfaces as error state", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useStore.getState().resetAgent();
  });

  // Regression: during the MCP spawn window a second `run_agent` call
  // must be rejected at the Tauri layer with `AlreadyRunning`. The
  // frontend must surface that rejection as an `error` status with
  // a non-null error message — not silently set status back to `running`
  // or swallow the rejection.
  it("surfaces AlreadyRunning rejections into agentStatus=error", async () => {
    invokeMock.mockRejectedValueOnce({
      kind: "AlreadyRunning",
      message: "Already running",
    });

    await useStore.getState().startAgent("do something");

    const state = useStore.getState();
    expect(state.agentStatus).toBe("error");
    expect(state.agentError).not.toBeNull();
    expect(state.agentError).not.toBe("");
  });

  // Same guarantee when the error arrives as a plain string — the
  // Tauri-specta layer serializes `CommandError` through Display.
  it("surfaces AlreadyRunning string-serialized rejection into agentStatus=error", async () => {
    invokeMock.mockRejectedValueOnce("AlreadyRunning: Already running");

    await useStore.getState().startAgent("do something");

    const state = useStore.getState();
    expect(state.agentStatus).toBe("error");
    expect(state.agentError).toMatch(/already running/i);
  });

  it("stays in running state when invoke succeeds (no error path)", async () => {
    invokeMock.mockResolvedValueOnce(undefined);

    await useStore.getState().startAgent("do something else");

    const state = useStore.getState();
    expect(state.agentStatus).toBe("running");
    expect(state.agentError).toBeNull();
  });

  it("clears previous agentRunId when starting a new run (null-gap arms)", async () => {
    // Simulate a prior run: install a run_id.
    useStore.getState().setAgentRunId("run-prior");
    expect(useStore.getState().agentRunId).toBe("run-prior");

    invokeMock.mockImplementationOnce(async () => {
      // At the moment invoke is in flight, the store must have already
      // dropped the previous run_id so any in-flight events from
      // "run-prior" are treated as stale by the null-gap guard.
      expect(useStore.getState().agentRunId).toBeNull();
      return undefined;
    });

    await useStore.getState().startAgent("fresh goal");
  });
});
