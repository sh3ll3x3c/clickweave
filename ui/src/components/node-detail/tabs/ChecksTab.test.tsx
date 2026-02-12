import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ChecksTab } from "./ChecksTab";
import type { Node } from "../../../bindings";

function makeNode(overrides: Partial<Node> = {}): Node {
  return {
    id: "node-1",
    name: "Test Node",
    node_type: { type: "Click", x: null, y: null, button: "Left", click_count: 1 },
    position: { x: 0, y: 0 },
    enabled: true,
    timeout_ms: null,
    retries: 0,
    trace_level: "Minimal",
    expected_outcome: null,
    checks: [],
    ...overrides,
  };
}

describe("ChecksTab", () => {
  it("renders add-check buttons", () => {
    render(<ChecksTab node={makeNode()} onUpdate={vi.fn()} />);
    expect(screen.getByText("+ TextPresent")).toBeInTheDocument();
    expect(screen.getByText("+ TextAbsent")).toBeInTheDocument();
    expect(screen.getByText("+ TemplateFound")).toBeInTheDocument();
    expect(screen.getByText("+ WindowTitleMatches")).toBeInTheDocument();
  });

  it("calls onUpdate with new check when add button is clicked", () => {
    const onUpdate = vi.fn();
    render(<ChecksTab node={makeNode()} onUpdate={onUpdate} />);

    fireEvent.click(screen.getByText("+ TextPresent"));

    expect(onUpdate).toHaveBeenCalledWith({
      checks: [
        expect.objectContaining({
          name: "Check 1",
          check_type: "TextPresent",
          on_fail: "FailNode",
        }),
      ],
    });
  });

  it("renders existing checks", () => {
    const node = makeNode({
      checks: [
        { name: "My Check", check_type: "TextAbsent", params: {}, on_fail: "FailNode" },
      ],
    });
    render(<ChecksTab node={node} onUpdate={vi.fn()} />);
    expect(screen.getByText("My Check (TextAbsent)")).toBeInTheDocument();
  });

  it("calls onUpdate to remove a check when delete is clicked", () => {
    const onUpdate = vi.fn();
    const node = makeNode({
      checks: [
        { name: "Check A", check_type: "TextPresent", params: {}, on_fail: "FailNode" },
        { name: "Check B", check_type: "TextAbsent", params: {}, on_fail: "FailNode" },
      ],
    });
    render(<ChecksTab node={node} onUpdate={onUpdate} />);

    const deleteButtons = screen.getAllByText("Delete");
    fireEvent.click(deleteButtons[0]);

    expect(onUpdate).toHaveBeenCalledWith({
      checks: [
        expect.objectContaining({ name: "Check B" }),
      ],
    });
  });
});
