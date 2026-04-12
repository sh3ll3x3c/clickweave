import { describe, it, expect } from "vitest";
import { isStaleRunId } from "./useAgentEvents";

describe("isStaleRunId", () => {
  it("treats a null active run as stale so events during stop/restart are dropped", () => {
    expect(isStaleRunId(null, "run-a")).toBe(true);
  });

  it("rejects events whose run_id does not match the active run", () => {
    expect(isStaleRunId("run-b", "run-a")).toBe(true);
  });

  it("accepts events whose run_id matches the active run", () => {
    expect(isStaleRunId("run-b", "run-b")).toBe(false);
  });
});
