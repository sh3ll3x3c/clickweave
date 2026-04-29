import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@xyflow/react", () => ({
  Handle: ({ type }: { type: string }) => <div data-testid={`handle-${type}`} />,
  Position: { Left: "left", Right: "right" },
}));

import { SkillLoopNode } from "./SkillLoopNode";
import { SkillSubSkillNode } from "./SkillSubSkillNode";
import { SkillToolCallNode } from "./SkillToolCallNode";

describe("skill React Flow nodes", () => {
  it("renders a tool-call node with binding chips", () => {
    render(
      <SkillToolCallNode
        {...({
          id: "tool",
        } as any)}
        data={{
          tool: "click",
          args: { target: "{{params.button_label}}" },
        }}
        selected={false}
      />,
    );

    expect(screen.getByText("click")).toBeInTheDocument();
    expect(screen.getByText("params.button_label")).toBeInTheDocument();
  });

  it("renders a sub-skill node with pinned version and bindings", () => {
    render(
      <SkillSubSkillNode
        {...({
          id: "sub",
        } as any)}
        data={{
          skillId: "open-chat",
          version: 2,
          name: "Open chat",
          parameters: { chat: "{{params.name}}" },
          bindOutputsAs: { selected_chat: "chat_id" },
        }}
        selected={false}
      />,
    );

    expect(screen.getByText("Open chat")).toBeInTheDocument();
    expect(screen.getByText("v2")).toBeInTheDocument();
    expect(screen.getByText("selected_chat -> chat_id")).toBeInTheDocument();
  });

  it("renders a loop node with predicate and iteration cap", () => {
    render(
      <SkillLoopNode
        {...({
          id: "loop",
        } as any)}
        data={{
          label: "Loop",
          until: "modal_present == false",
          maxIterations: 5,
          childCount: 2,
        }}
        selected={false}
      />,
    );

    expect(screen.getByText("Loop")).toBeInTheDocument();
    expect(screen.getByText(/modal_present == false/)).toBeInTheDocument();
    expect(screen.getByText("5")).toBeInTheDocument();
  });
});
