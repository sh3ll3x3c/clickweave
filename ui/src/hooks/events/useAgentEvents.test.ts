import { describe, it, expect } from "vitest";
import { isStaleRunId } from "./useAgentEvents";

// Regression tests for the null-gap that used to allow events from a
// previous agent run to leak into a subsequent run. The fix:
// `agentRunId === null` is treated as stale, even before the new
// `agent://started` event installs the next run_id.

describe("isStaleRunId", () => {
  it("drops events when no run is active (null-gap guard)", () => {
    // Stop/restart leaves a window where agentRunId is null but events
    // from the previous run can still arrive. All such events are stale.
    expect(isStaleRunId(null, "run-a")).toBe(true);
    expect(isStaleRunId(null, "")).toBe(true);
  });

  it("drops events from a previous run_id", () => {
    // Run A was active; run B just started. An in-flight event from A
    // must not update run B's state.
    expect(isStaleRunId("run-b", "run-a")).toBe(true);
  });

  it("passes events matching the active run_id", () => {
    expect(isStaleRunId("run-b", "run-b")).toBe(false);
  });

  it("rejects empty incoming run_id when active run is set", () => {
    expect(isStaleRunId("run-a", "")).toBe(true);
  });
});
