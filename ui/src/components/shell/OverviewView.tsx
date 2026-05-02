import { useStore } from "../../store/useAppStore";
import { useShallow } from "zustand/react/shallow";
import { AssistantPanel } from "../AssistantPanel";
import { IntentEmptyState } from "../IntentEmptyState";
import { WorkflowRow } from "./WorkflowRow";

export function OverviewView() {
  const { workflow, isNewWorkflow } = useStore(
    useShallow((s) => ({
      workflow: s.workflow,
      isNewWorkflow: s.isNewWorkflow,
    })),
  );
  const { drawerOpen, assistantError, messages } = useStore(
    useShallow((s) => ({
      drawerOpen: s.assistantSurface === "drawer",
      assistantError: s.assistantError,
      messages: s.messages,
    })),
  );
  const setAssistantSurface = useStore((s) => s.setAssistantSurface);
  const skipIntentEntry = useStore((s) => s.skipIntentEntry);
  const startAgent = useStore((s) => s.startAgent);
  const agentStatus = useStore((s) => s.agentStatus);

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

  // Phase 1 placeholder: show the existing AssistantPanel centered.
  // Phase 3 replaces this with the OverviewAssistantCard + LiveRuntimeCard
  // + CanvasPreviewCard composition.
  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <WorkflowRow />
      <div className="flex flex-1 items-stretch justify-center overflow-hidden">
        <AssistantPanel
          open={drawerOpen}
          error={assistantError}
          messages={messages}
          onSendMessage={startAgent}
          onClose={() => setAssistantSurface(null)}
        />
      </div>
    </div>
  );
}
