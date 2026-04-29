import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const storeMock = vi.hoisted(() => ({
  state: {
    agentStatus: "running",
    agentRunId: "run-1",
    pendingApproval: null,
    completionDisagreement: null,
    consecutiveDestructiveCapHit: null,
    setConsecutiveDestructiveCapHit: vi.fn(),
    confirmDisagreementAsComplete: vi.fn(),
    cancelDisagreement: vi.fn(),
    stopAgent: vi.fn(),
    approveAction: vi.fn(),
    rejectAction: vi.fn(),
    ambiguityResolutions: [],
    activeAmbiguityId: null,
    openAmbiguityModal: vi.fn(),
    closeAmbiguityModal: vi.fn(),
    clearConversationFlow: vi.fn(),
    executorState: "idle",
    workflow: {
      intent: null,
      nodes: [],
    },
    setIntent: vi.fn(),
    runTraces: {
      "run-1": {
        runId: "run-1",
        phase: "executing",
        activeSubgoal: "Open account page",
        steps: [
          {
            stepIndex: 0,
            toolName: "cdp_click",
            phase: "executing",
            body: "Clicked account",
            failed: false,
          },
        ],
        worldModelDeltas: [],
        milestones: [],
        terminalFrame: null,
      },
    },
  },
}));

vi.mock("../hooks/useHorizontalResize", () => ({
  useHorizontalResize: () => ({
    width: 360,
    handleResizeStart: vi.fn(),
  }),
}));

vi.mock("../store/useAppStore", () => ({
  useStore: <T,>(selector: (state: typeof storeMock.state) => T) =>
    selector(storeMock.state),
}));

import { AssistantPanel } from "./AssistantPanel";

describe("AssistantPanel", () => {
  beforeEach(() => {
    storeMock.state.agentStatus = "running";
    storeMock.state.agentRunId = "run-1";
  });

  it("renders the active run trace instead of the old running text", () => {
    render(
      <AssistantPanel
        open
        error={null}
        messages={[]}
        onSendMessage={vi.fn()}
        onClose={vi.fn()}
      />,
    );

    expect(screen.getByText("Open account page")).toBeInTheDocument();
    expect(screen.getByText("cdp_click")).toBeInTheDocument();
    expect(screen.queryByText("Agent running...")).not.toBeInTheDocument();
  });
});
