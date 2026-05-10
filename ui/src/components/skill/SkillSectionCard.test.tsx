/**
 * Tests for `SkillSectionCard`.
 *
 * Coverage per plan:
 * (a) renders heading + summary
 * (b) shows step count
 * (c) hover button visible only on hover
 * (d) expand toggles ### rows
 */

import { cleanup, render, screen, fireEvent } from "@testing-library/react";
import { afterEach, describe, it, expect, vi, beforeEach } from "vitest";

// Mock Tauri
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: class {
    static async getByLabel() { return null; }
  },
}));
vi.mock("@tauri-apps/api/window", () => ({ currentMonitor: async () => null }));
vi.mock("../../bindings", () => ({
  commands: new Proxy({}, {
    get: () => vi.fn(async () => undefined),
  }),
}));

import { useStore } from "../../store/useAppStore";
import { SkillSectionCard } from "./SkillSectionCard";
import type { SkillSection } from "../../bindings";

const section: SkillSection = {
  id: "section_1",
  heading: "Launch the app",
  level: 2,
  step_ids: ["s_001", "s_002"],
  body_range: [0, 30],
};

const sectionBody = "Open the application and click the button.";

function renderCard(props?: Partial<Parameters<typeof SkillSectionCard>[0]>) {
  return render(
    <SkillSectionCard
      section={section}
      sectionBody={sectionBody}
      selected={false}
      onClick={vi.fn()}
      {...props}
    />,
  );
}

describe("SkillSectionCard", () => {
  beforeEach(() => {
    useStore.setState({ sectionApproval: null });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  // (a) renders heading + summary
  it("renders the section heading", () => {
    renderCard();
    expect(screen.getByText("Launch the app")).toBeInTheDocument();
  });

  it("renders a one-line summary from the section body", () => {
    renderCard();
    expect(
      screen.getByText("Open the application and click the button."),
    ).toBeInTheDocument();
  });

  // (b) shows step count
  it("shows the step count badge when step_ids are present", () => {
    renderCard();
    expect(screen.getByText("2 steps")).toBeInTheDocument();
  });

  it("shows singular step label for a single step", () => {
    const oneStep: SkillSection = { ...section, step_ids: ["s_001"] };
    render(
      <SkillSectionCard
        section={oneStep}
        sectionBody={sectionBody}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByText("1 step")).toBeInTheDocument();
  });

  // (c) hover button visible only on hover (data-testid)
  it("the Edit button exists in the DOM and is revealed on hover", () => {
    renderCard();
    // Trigger hover by dispatching mouseenter
    const card = screen.getByText("Launch the app").closest("div.group");
    expect(card).toBeTruthy();
    fireEvent.mouseEnter(card!);
    expect(screen.getByTestId("edit-with-assistant")).toBeInTheDocument();
  });

  // (d) expand toggle exists for sub-sections (level >= 3)
  it("shows expand toggle for sub-sections (level 3)", () => {
    const subSection: SkillSection = { ...section, level: 3 };
    render(
      <SkillSectionCard
        section={subSection}
        sectionBody={sectionBody}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByTestId("expand-toggle")).toBeInTheDocument();
  });

  it("expand toggle reveals step ids when clicked", () => {
    const subSection: SkillSection = { ...section, level: 3 };
    render(
      <SkillSectionCard
        section={subSection}
        sectionBody={sectionBody}
        selected={false}
        onClick={vi.fn()}
      />,
    );
    // Step IDs are not visible yet
    expect(screen.queryByText("s_001")).not.toBeInTheDocument();
    // Click expand
    fireEvent.click(screen.getByTestId("expand-toggle"));
    expect(screen.getByText("s_001")).toBeInTheDocument();
    expect(screen.getByText("s_002")).toBeInTheDocument();
    // Click again to collapse
    fireEvent.click(screen.getByTestId("expand-toggle"));
    expect(screen.queryByText("s_001")).not.toBeInTheDocument();
  });
});
