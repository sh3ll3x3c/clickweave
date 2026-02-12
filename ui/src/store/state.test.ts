import { describe, it, expect } from "vitest";
import { makeDefaultWorkflow, DEFAULT_ENDPOINT } from "./state";

describe("makeDefaultWorkflow", () => {
  it("creates a workflow with UUID id", () => {
    const wf = makeDefaultWorkflow();
    expect(wf.id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/,
    );
  });

  it("creates unique IDs on each call", () => {
    const a = makeDefaultWorkflow();
    const b = makeDefaultWorkflow();
    expect(a.id).not.toBe(b.id);
  });

  it("starts with empty nodes and edges", () => {
    const wf = makeDefaultWorkflow();
    expect(wf.nodes).toEqual([]);
    expect(wf.edges).toEqual([]);
  });
});

describe("DEFAULT_ENDPOINT", () => {
  it("has localhost base URL", () => {
    expect(DEFAULT_ENDPOINT.baseUrl).toContain("localhost");
  });

  it("has empty apiKey", () => {
    expect(DEFAULT_ENDPOINT.apiKey).toBe("");
  });
});
