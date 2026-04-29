import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  loadSkillsForPanel: vi.fn(),
  walkthrough: {
    status: "Review",
    panelOpen: true,
    error: null,
    sessionId: "session-1",
    actions: [
      {
        id: "action-1",
        candidate: false,
        kind: { type: "Click" },
        app_name: null,
        artifact_paths: [],
        target_candidates: [],
        warnings: [],
        confidence: "high",
      },
    ],
    draft: {
      id: "draft-1",
      name: "Draft",
      nodes: [],
      edges: [],
      groups: [],
      intent: null,
    },
    warnings: [],
    annotations: {
      deleted_node_ids: [],
      renamed_nodes: [],
      target_overrides: [],
      variable_promotions: [],
    },
    expandedAction: null,
    actionNodeMap: [],
    nodeOrder: [],
    setPanelOpen: vi.fn(),
    setExpandedAction: vi.fn(),
    keepCandidate: vi.fn(),
    dismissCandidate: vi.fn(),
    deleteNode: vi.fn(),
    restoreNode: vi.fn(),
    renameNode: vi.fn(),
    overrideTarget: vi.fn(),
    promoteToVariable: vi.fn(),
    removeVariablePromotion: vi.fn(),
    reorderNode: vi.fn(),
    reorderGroup: vi.fn(),
    applyDraftToCanvas: vi.fn(),
    discardDraft: vi.fn(),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => path,
  invoke: (...args: unknown[]) => mocks.invoke(...args),
}));

vi.mock("../hooks/useHorizontalResize", () => ({
  useHorizontalResize: () => ({
    width: 360,
    handleResizeStart: vi.fn(),
  }),
}));

vi.mock("../hooks/useWalkthrough", () => ({
  useWalkthrough: () => mocks.walkthrough,
}));

vi.mock("../store/useAppStore", () => ({
  useStore: (selector: (state: unknown) => unknown) =>
    selector({
      assistantOpen: false,
      projectPath: null,
      workflow: {
        id: "workflow-1",
        name: "Workflow",
        nodes: [],
        edges: [],
        groups: [],
      },
      skillsGlobalParticipation: false,
      loadSkillsForPanel: mocks.loadSkillsForPanel,
    }),
}));

vi.mock("../utils/walkthroughGrouping", () => ({
  computeAppGroups: () => [],
}));

import { WalkthroughPanel } from "./WalkthroughPanel";

describe("WalkthroughPanel", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.invoke.mockResolvedValue({
      id: "skill-1",
      version: 1,
      name: "Saved Skill",
    });
    mocks.loadSkillsForPanel.mockReset();
    mocks.loadSkillsForPanel.mockResolvedValue(undefined);
  });

  it("saves the reviewed walkthrough as a skill", async () => {
    render(<WalkthroughPanel />);

    fireEvent.click(screen.getByRole("button", { name: /save as skill/i }));

    await waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith("save_walkthrough_as_skill", {
        request: {
          session_id: "session-1",
          project_path: null,
          workflow_name: "Workflow",
          workflow_id: "workflow-1",
        },
      });
    });
    await waitFor(() => {
      expect(mocks.loadSkillsForPanel).toHaveBeenCalledWith({
        projectPath: null,
        workflowName: "Workflow",
        workflowId: "workflow-1",
        includeGlobal: false,
      });
    });
  });
});
