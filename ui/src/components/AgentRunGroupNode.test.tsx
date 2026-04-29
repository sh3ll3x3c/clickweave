import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AgentRunGroupNode } from "./AgentRunGroupNode";

describe("AgentRunGroupNode", () => {
  it("renders the run summary and step count", () => {
    render(
      <AgentRunGroupNode
        {...({ id: "agent-run-run-1" } as any)}
        data={{
          runId: "run-1",
          summary: "Create invoice workflow",
          stepCount: 3,
          isCollapsed: false,
          onToggleCollapse: vi.fn(),
        }}
        selected={false}
      />,
    );

    expect(screen.getByText("Create invoice workflow")).toBeInTheDocument();
    expect(screen.getByText("3 steps")).toBeInTheDocument();
  });

  it("calls onToggleCollapse from the header button", () => {
    const onToggleCollapse = vi.fn();
    render(
      <AgentRunGroupNode
        {...({ id: "agent-run-run-1" } as any)}
        data={{
          runId: "run-1",
          summary: "Create invoice workflow",
          stepCount: 1,
          isCollapsed: true,
          onToggleCollapse,
        }}
        selected={false}
      />,
    );

    fireEvent.click(screen.getByTitle("Expand run"));

    expect(onToggleCollapse).toHaveBeenCalledTimes(1);
    expect(screen.getByText("1 step")).toBeInTheDocument();
  });
});
