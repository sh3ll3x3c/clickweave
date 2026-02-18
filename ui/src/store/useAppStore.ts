import { useEffect } from "react";
import { create } from "zustand";
import type { Workflow } from "../bindings";
import type { AppState, AppActions } from "./state";
import { createAssistantSlice } from "./slices/assistantSlice";
import { createExecutionSlice } from "./slices/executionSlice";
import { createHistorySlice } from "./slices/historySlice";
import { createLogSlice } from "./slices/logSlice";
import { createProjectSlice } from "./slices/projectSlice";
import { createSettingsSlice } from "./slices/settingsSlice";
import { createUiSlice } from "./slices/uiSlice";
import { createVerdictSlice } from "./slices/verdictSlice";
import type { StoreState } from "./slices/types";
import { useWorkflowMutations } from "./useWorkflowMutations";

export type { DetailTab, EndpointConfig, AppState, AppActions } from "./state";

// ── Zustand store ────────────────────────────────────────────────

export const useStore = create<StoreState>()((...a) => ({
  ...createSettingsSlice(...a),
  ...createProjectSlice(...a),
  ...createAssistantSlice(...a),
  ...createExecutionSlice(...a),
  ...createHistorySlice(...a),
  ...createLogSlice(...a),
  ...createUiSlice(...a),
  ...createVerdictSlice(...a),
}));

// ── Adapter: React-style dispatchers for useWorkflowMutations ───
// TODO: Remove this adapter layer once consumers migrate to direct Zustand
// selectors (useStore(s => s.field)). The [AppState, AppActions] wrapper
// subscribes to the entire store, negating Zustand's selective re-render benefit.

const setWorkflowDispatch: React.Dispatch<React.SetStateAction<Workflow>> = (action) => {
  if (typeof action === "function") {
    useStore.setState((s) => ({ workflow: action(s.workflow) }));
  } else {
    useStore.setState({ workflow: action });
  }
};

const setSelectedNodeDispatch: React.Dispatch<React.SetStateAction<string | null>> = (action) => {
  if (typeof action === "function") {
    useStore.setState((s) => ({ selectedNode: action(s.selectedNode) }));
  } else {
    useStore.setState({ selectedNode: action });
  }
};

// ── Public hook ──────────────────────────────────────────────────

export function useAppStore(): [AppState, AppActions] {
  const store = useStore();

  // Fire one-time loaders on mount
  useEffect(() => {
    store.loadSettingsFromDisk();
    store.loadNodeTypes();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Workflow mutations (keeps useWorkflowMutations as-is)
  const { addNode, removeNode, removeNodes, removeEdgesOnly, updateNodePositions, updateNode, addEdge, removeEdge } =
    useWorkflowMutations(setWorkflowDispatch, setSelectedNodeDispatch, store.workflow.nodes.length, store.pushHistory);

  const state: AppState = {
    workflow: store.workflow,
    projectPath: store.projectPath,
    nodeTypes: store.nodeTypes,
    selectedNode: store.selectedNode,
    activeNode: store.activeNode,
    executorState: store.executorState,
    detailTab: store.detailTab,
    sidebarCollapsed: store.sidebarCollapsed,
    logsDrawerOpen: store.logsDrawerOpen,
    nodeSearch: store.nodeSearch,
    showSettings: store.showSettings,
    isNewWorkflow: store.isNewWorkflow,
    allowAiTransforms: store.allowAiTransforms,
    allowAgentSteps: store.allowAgentSteps,
    assistantOpen: store.assistantOpen,
    assistantLoading: store.assistantLoading,
    assistantRetrying: store.assistantRetrying,
    assistantError: store.assistantError,
    conversation: store.conversation,
    pendingPatch: store.pendingPatch,
    pendingPatchWarnings: store.pendingPatchWarnings,
    logs: store.logs,
    plannerConfig: store.plannerConfig,
    agentConfig: store.agentConfig,
    vlmConfig: store.vlmConfig,
    vlmEnabled: store.vlmEnabled,
    mcpCommand: store.mcpCommand,
    maxRepairAttempts: store.maxRepairAttempts,
  };

  // Stable action references — Zustand actions don't change identity
  const actions: AppActions = {
    setWorkflow: store.setWorkflow,
    selectNode: store.selectNode,
    setDetailTab: store.setDetailTab,
    toggleSidebar: store.toggleSidebar,
    toggleLogsDrawer: store.toggleLogsDrawer,
    setNodeSearch: store.setNodeSearch,
    setShowSettings: store.setShowSettings,
    pushLog: store.pushLog,
    clearLogs: store.clearLogs,
    addNode,
    removeNode,
    removeNodes,
    removeEdgesOnly,
    updateNodePositions,
    updateNode,
    addEdge,
    removeEdge,
    openProject: store.openProject,
    saveProject: store.saveProject,
    newProject: store.newProject,
    setPlannerConfig: store.setPlannerConfig,
    setAgentConfig: store.setAgentConfig,
    setVlmConfig: store.setVlmConfig,
    setVlmEnabled: store.setVlmEnabled,
    setMcpCommand: store.setMcpCommand,
    setMaxRepairAttempts: store.setMaxRepairAttempts,
    setActiveNode: store.setActiveNode,
    setExecutorState: store.setExecutorState,
    runWorkflow: store.runWorkflow,
    stopWorkflow: store.stopWorkflow,
    setAllowAiTransforms: store.setAllowAiTransforms,
    setAllowAgentSteps: store.setAllowAgentSteps,
    skipIntentEntry: store.skipIntentEntry,
    setAssistantOpen: store.setAssistantOpen,
    toggleAssistant: store.toggleAssistant,
    sendAssistantMessage: store.sendAssistantMessage,
    resendMessage: store.resendMessage,
    applyPendingPatch: store.applyPendingPatch,
    discardPendingPatch: store.discardPendingPatch,
    cancelAssistantChat: store.cancelAssistantChat,
    clearConversation: store.clearConversation,
    setVerdicts: store.setVerdicts,
    clearVerdicts: store.clearVerdicts,
    pushHistory: store.pushHistory,
    undo: store.undo,
    redo: store.redo,
  };

  return [state, actions];
}
