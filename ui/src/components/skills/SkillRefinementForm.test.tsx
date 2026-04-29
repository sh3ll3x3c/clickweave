import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import {
  SkillRefinementForm,
  type SkillRefinementProposal,
} from "./SkillRefinementForm";

const proposal: SkillRefinementProposal = {
  parameter_schema: [
    { name: "contact", type_tag: "string", default: null },
    { name: "message", type_tag: "string", default: null },
  ],
  binding_corrections: [
    {
      step_index: 1,
      capture_name: "chat_id",
      keep: true,
      correction: null,
    },
  ],
  description: "Send a saved message",
  name_suggestion: "Send message",
};

describe("SkillRefinementForm", () => {
  it("submits edited proposal values", () => {
    const onAccept = vi.fn();
    const onReject = vi.fn();

    render(
      <SkillRefinementForm
        initial={proposal}
        onAccept={onAccept}
        onReject={onReject}
      />,
    );

    fireEvent.change(screen.getByDisplayValue("contact"), {
      target: { value: "recipient" },
    });
    fireEvent.click(screen.getByRole("button", { name: /confirm/i }));

    expect(onAccept).toHaveBeenCalledTimes(1);
    expect(onAccept.mock.calls[0][0].parameter_schema[0].name).toBe(
      "recipient",
    );
  });
});
