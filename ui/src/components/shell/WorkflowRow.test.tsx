import { describe, expect, it, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { WorkflowRow } from "./WorkflowRow";
import { useStore } from "../../store/useAppStore";

describe("WorkflowRow", () => {
  beforeEach(() => {
    useStore.setState({
      workflow: {
        ...useStore.getState().workflow,
        name: "MyFlow",
      },
    });
  });

  it("wraps rename in pushHistory so undo restores the previous name", () => {
    render(<WorkflowRow />);
    fireEvent.click(screen.getByRole("button", { name: /rename/i }));
    const input = screen.getByDisplayValue("MyFlow");
    fireEvent.change(input, { target: { value: "Renamed" } });
    fireEvent.blur(input);

    expect(useStore.getState().workflow.name).toBe("Renamed");
    useStore.getState().undo();
    expect(useStore.getState().workflow.name).toBe("MyFlow");
  });
});
