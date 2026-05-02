import { useShallow } from "zustand/react/shallow";
import { useStore } from "../../store/useAppStore";
import { CanvasPreviewCard } from "./CanvasPreviewCard";
import { IntentEmptyState } from "../IntentEmptyState";
import { LiveRuntimeCard } from "./LiveRuntimeCard";
import { OverviewAssistantCard } from "./OverviewAssistantCard";
import { WorkflowRow } from "./WorkflowRow";

/**
 * Overview view body composition. The Overview is the new primary
 * cockpit: assistant thread on the left (7 columns), live runtime +
 * canvas preview on the right (5 columns).
 *
 * Branches on `IntentEmptyState` per D22 when the workflow is fresh
 * and empty. Phase 4 inserts `<StatsStrip />` between `WorkflowRow`
 * and the body grid.
 */
export function OverviewView() {
  const { workflow, isNewWorkflow, agentStatus } = useStore(
    useShallow((s) => ({
      workflow: s.workflow,
      isNewWorkflow: s.isNewWorkflow,
      agentStatus: s.agentStatus,
    })),
  );
  const setAssistantSurface = useStore((s) => s.setAssistantSurface);
  const skipIntentEntry = useStore((s) => s.skipIntentEntry);
  const startAgent = useStore((s) => s.startAgent);

  if (isNewWorkflow && workflow.nodes.length === 0) {
    return (
      <IntentEmptyState
        onGenerate={(intent) => {
          setAssistantSurface("overview-card");
          skipIntentEntry();
          startAgent(intent);
        }}
        onSkip={skipIntentEntry}
        onRecordWalkthrough={() => {
          skipIntentEntry();
          useStore.getState().openCdpModal();
        }}
        loading={agentStatus === "running"}
      />
    );
  }

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <WorkflowRow />
      {/* Phase 4 inserts <StatsStrip /> here. */}
      <div className="grid min-h-0 flex-1 grid-cols-12 gap-3 px-6 pb-3">
        <div className="col-span-7 min-h-0">
          <OverviewAssistantCard />
        </div>
        <div className="col-span-5 grid min-h-0 grid-rows-2 gap-3">
          <LiveRuntimeCard />
          <CanvasPreviewCard />
        </div>
      </div>
    </div>
  );
}
