import { describe, expect, it, beforeEach, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("./CanvasPreviewCard", () => ({
  CanvasPreviewCard: () => <div data-testid="canvas-preview-card" />,
}));
vi.mock("./LiveRuntimeCard", () => ({
  LiveRuntimeCard: () => <div data-testid="live-runtime-card" />,
}));
vi.mock("./OverviewAssistantCard", () => ({
  OverviewAssistantCard: () => <div data-testid="overview-assistant-card" />,
}));
vi.mock("./StatsStrip", () => ({
  StatsStrip: () => <div data-testid="stats-strip" />,
}));
vi.mock("./WorkflowRow", () => ({
  WorkflowRow: () => <div data-testid="workflow-row" />,
}));
vi.mock("../skills/SkillsPanel", () => ({
  SkillsPanel: () => <div data-testid="skills-panel" />,
}));

import { OverviewView } from "./OverviewView";
import { useStore } from "../../store/useAppStore";

describe("OverviewView walkthrough entry", () => {
  beforeEach(() => {
    useStore.setState({
      currentView: "overview",
      isNewWorkflow: true,
      agentStatus: "idle",
      walkthroughCdpModalOpen: false,
      walkthroughCdpProgress: [],
      workflow: {
        ...useStore.getState().workflow,
        nodes: [],
        edges: [],
        groups: [],
      },
    });
  });

  it("switches to Canvas before opening the CDP walkthrough modal", () => {
    render(<OverviewView />);

    fireEvent.click(screen.getByRole("button", { name: /record walkthrough/i }));

    expect(useStore.getState().currentView).toBe("canvas");
    expect(useStore.getState().walkthroughCdpModalOpen).toBe(true);
  });
});
