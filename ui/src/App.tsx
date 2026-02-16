import { useAppStore } from "./store/useAppStore";
import { Sidebar } from "./components/Sidebar";
import { Header } from "./components/Header";
import { NodePalette } from "./components/NodePalette";
import { LogsDrawer } from "./components/LogsDrawer";
import { FloatingToolbar } from "./components/FloatingToolbar";
import { SettingsModal } from "./components/SettingsModal";
import { GraphCanvas } from "./components/GraphCanvas";
import { NodeDetailModal } from "./components/node-detail/NodeDetailModal";
import { AssistantPanel } from "./components/AssistantPanel";
import { IntentEmptyState } from "./components/IntentEmptyState";
import { VerdictBar } from "./components/VerdictBar";
import { useEffect, useMemo } from "react";
import { listen } from "@tauri-apps/api/event";

function App() {
  const [state, actions] = useAppStore();

  const selectedNodeData = useMemo(
    () =>
      state.selectedNode
        ? state.workflow.nodes.find((n) => n.id === state.selectedNode) ?? null
        : null,
    [state.selectedNode, state.workflow.nodes],
  );

  const hasAiNodes = useMemo(
    () => state.workflow.nodes.some((n) => n.node_type.type === "AiStep"),
    [state.workflow.nodes],
  );

  useEffect(() => {
    const subscriptions = Promise.all([
      listen<{ message: string }>("executor://log", (e) => {
        actions.pushLog(e.payload.message);
      }),
      listen<{ state: string }>("executor://state", (e) => {
        const s = e.payload.state as "idle" | "running";
        actions.setExecutorState(s);
        if (s === "idle") actions.setActiveNode(null);
        if (s === "running") actions.clearVerdicts();
      }),
      listen<{ node_id: string }>("executor://node_started", (e) => {
        actions.setActiveNode(e.payload.node_id);
        actions.pushLog(`Node started: ${e.payload.node_id}`);
      }),
      listen<{ node_id: string }>("executor://node_completed", (e) => {
        actions.setActiveNode(null);
        actions.pushLog(`Node completed: ${e.payload.node_id}`);
      }),
      listen<{ node_id: string; error: string }>("executor://node_failed", (e) => {
        actions.setActiveNode(null);
        actions.pushLog(`Node failed: ${e.payload.node_id} - ${e.payload.error}`);
      }),
      listen<import("./store/slices/verdictSlice").NodeVerdict[]>(
        "executor://checks_completed",
        (e) => {
          actions.setVerdicts(e.payload);
        },
      ),
      listen("executor://workflow_completed", () => {
        actions.pushLog("Workflow completed");
        actions.setExecutorState("idle");
        actions.setActiveNode(null);
      }),
      listen("menu://new", () => actions.newProject()),
      listen("menu://open", () => actions.openProject()),
      listen("menu://save", () => actions.saveProject()),
      listen("menu://toggle-sidebar", () => actions.toggleSidebar()),
      listen("menu://toggle-logs", () => actions.toggleLogsDrawer()),
      listen("menu://run-workflow", () => actions.runWorkflow()),
      listen("menu://stop-workflow", () => actions.stopWorkflow()),
      listen("menu://toggle-assistant", () => actions.toggleAssistant()),
    ]);

    return () => {
      subscriptions.then((unlisteners) => unlisteners.forEach((u) => u()));
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="flex h-screen overflow-hidden bg-[var(--bg-dark)]">
      <Sidebar
        collapsed={state.sidebarCollapsed}
        onToggle={actions.toggleSidebar}
      />

      <div className="flex flex-1 flex-col overflow-hidden">
        <Header
          workflowName={state.workflow.name}
          projectPath={state.projectPath}
          executorState={state.executorState}
          onSave={actions.saveProject}
          onOpen={actions.openProject}
          onNew={actions.newProject}
          onSettings={() => actions.setShowSettings(true)}
          onNameChange={(name) =>
            actions.setWorkflow({ ...state.workflow, name })
          }
        />
        <VerdictBar />

        <div className="flex flex-1 overflow-hidden">
          {state.isNewWorkflow && state.workflow.nodes.length === 0 ? (
            <IntentEmptyState
              onGenerate={(intent) => {
                actions.setAssistantOpen(true);
                actions.skipIntentEntry();
                actions.sendAssistantMessage(intent);
              }}
              onSkip={actions.skipIntentEntry}
              loading={state.assistantLoading}
            />
          ) : (
            <>
              <div className="relative flex-1 overflow-hidden bg-[var(--bg-dark)]">
                <GraphCanvas
                  workflow={state.workflow}
                  selectedNode={state.selectedNode}
                  activeNode={state.activeNode}
                  onSelectNode={actions.selectNode}
                  onNodePositionsChange={actions.updateNodePositions}
                  onEdgesChange={(edges) =>
                    actions.setWorkflow({ ...state.workflow, edges })
                  }
                  onConnect={actions.addEdge}
                  onDeleteNodes={actions.removeNodes}
                />

                <FloatingToolbar
                  executorState={state.executorState}
                  logsOpen={state.logsDrawerOpen}
                  hasAiNodes={hasAiNodes}
                  onToggleLogs={actions.toggleLogsDrawer}
                  onRunStop={
                    state.executorState === "running"
                      ? actions.stopWorkflow
                      : actions.runWorkflow
                  }
                  onAssistant={actions.toggleAssistant}
                />
              </div>

              <AssistantPanel
                open={state.assistantOpen}
                loading={state.assistantLoading}
                error={state.assistantError}
                conversation={state.conversation}
                pendingPatch={state.pendingPatch}
                pendingPatchWarnings={state.pendingPatchWarnings}
                onSendMessage={actions.sendAssistantMessage}
                onApplyPatch={actions.applyPendingPatch}
                onDiscardPatch={actions.discardPendingPatch}
                onClearConversation={actions.clearConversation}
                onClose={() => actions.setAssistantOpen(false)}
              />

              <NodePalette
                nodeTypes={state.nodeTypes}
                search={state.nodeSearch}
                onSearchChange={actions.setNodeSearch}
                onAdd={actions.addNode}
              />
            </>
          )}
        </div>

        <LogsDrawer
          open={state.logsDrawerOpen}
          logs={state.logs}
          onToggle={actions.toggleLogsDrawer}
          onClear={actions.clearLogs}
        />
      </div>

      <NodeDetailModal
        node={selectedNodeData}
        projectPath={state.projectPath}
        workflowId={state.workflow.id}
        workflowName={state.workflow.name}
        tab={state.detailTab}
        onTabChange={actions.setDetailTab}
        onUpdate={actions.updateNode}
        onClose={() => actions.selectNode(null)}
      />

      <SettingsModal
        open={state.showSettings}
        plannerConfig={state.plannerConfig}
        agentConfig={state.agentConfig}
        vlmConfig={state.vlmConfig}
        vlmEnabled={state.vlmEnabled}
        mcpCommand={state.mcpCommand}
        onClose={() => actions.setShowSettings(false)}
        onPlannerConfigChange={actions.setPlannerConfig}
        onAgentConfigChange={actions.setAgentConfig}
        onVlmConfigChange={actions.setVlmConfig}
        onVlmEnabledChange={actions.setVlmEnabled}
        onMcpCommandChange={actions.setMcpCommand}
      />
    </div>
  );
}

export default App;
