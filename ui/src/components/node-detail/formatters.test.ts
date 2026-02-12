import { describe, it, expect } from "vitest";
import { runDuration, eventTypeColor, formatEventPayload } from "./formatters";
import type { NodeRun } from "../../bindings";

function makeRun(overrides: Partial<NodeRun> = {}): NodeRun {
  return {
    node_id: "n1",
    run_id: "r1",
    started_at: 1000,
    ended_at: null,
    status: "Ok",
    trace_level: "Minimal",
    events: [],
    artifacts: [],
    observed_summary: null,
    ...overrides,
  };
}

describe("runDuration", () => {
  it("returns null when run has no ended_at", () => {
    expect(runDuration(makeRun())).toBeNull();
  });

  it("computes duration in seconds", () => {
    expect(runDuration(makeRun({ started_at: 1000, ended_at: 3500 }))).toBe(
      "2.5",
    );
  });

  it("returns 0.0 for instant runs", () => {
    expect(runDuration(makeRun({ started_at: 1000, ended_at: 1000 }))).toBe(
      "0.0",
    );
  });
});

describe("eventTypeColor", () => {
  it("returns known color for tool_call", () => {
    expect(eventTypeColor("tool_call")).toContain("purple");
  });

  it("returns fallback for unknown type", () => {
    expect(eventTypeColor("unknown_event")).toContain("bg-[var(--bg-hover)]");
  });
});

describe("formatEventPayload", () => {
  it("returns empty string for null", () => {
    expect(formatEventPayload(null)).toBe("");
  });

  it("formats string payload directly", () => {
    expect(formatEventPayload("hello")).toBe("hello");
  });

  it("formats object with name and text_len", () => {
    expect(formatEventPayload({ name: "click", text_len: 42 })).toBe(
      "click | 42 chars",
    );
  });

  it("falls back to JSON for unknown keys", () => {
    expect(formatEventPayload({ custom: true })).toBe('{"custom":true}');
  });

  it("formats error and attempt fields", () => {
    expect(
      formatEventPayload({ error: "timeout", attempt: 2 }),
    ).toBe("error: timeout | attempt 2");
  });
});
