import type { Workflow, NodeTypeInfo, WorkflowPatch } from "../bindings";

export type DetailTab = "setup" | "trace" | "checks" | "runs";

export interface EndpointConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

export interface AppState {
  workflow: Workflow;
  projectPath: string | null;
  nodeTypes: NodeTypeInfo[];
  selectedNode: string | null;
  activeNode: string | null;
  executorState: "idle" | "running";
  detailTab: DetailTab;
  sidebarCollapsed: boolean;
  logsDrawerOpen: boolean;
  nodeSearch: string;
  showSettings: boolean;
  isNewWorkflow: boolean;
  showPlannerModal: boolean;
  plannerLoading: boolean;
  plannerError: string | null;
  pendingWorkflow: Workflow | null;
  plannerWarnings: string[];
  allowAiTransforms: boolean;
  allowAgentSteps: boolean;
  showAssistant: boolean;
  assistantLoading: boolean;
  assistantError: string | null;
  assistantPatch: WorkflowPatch | null;
  logs: string[];
  plannerConfig: EndpointConfig;
  agentConfig: EndpointConfig;
  vlmConfig: EndpointConfig;
  vlmEnabled: boolean;
  mcpCommand: string;
}

export interface AppActions {
  setWorkflow: (w: Workflow) => void;
  selectNode: (id: string | null) => void;
  setDetailTab: (tab: DetailTab) => void;
  toggleSidebar: () => void;
  toggleLogsDrawer: () => void;
  setNodeSearch: (s: string) => void;
  setShowSettings: (show: boolean) => void;
  pushLog: (msg: string) => void;
  clearLogs: () => void;
  addNode: (nodeType: import("../bindings").NodeType) => void;
  removeNode: (id: string) => void;
  updateNodePositions: (updates: Map<string, { x: number; y: number }>) => void;
  updateNode: (id: string, updates: Partial<import("../bindings").Node>) => void;
  addEdge: (from: string, to: string) => void;
  removeEdge: (from: string, to: string) => void;
  openProject: () => Promise<void>;
  saveProject: () => Promise<void>;
  newProject: () => void;
  setPlannerConfig: (config: EndpointConfig) => void;
  setAgentConfig: (config: EndpointConfig) => void;
  setVlmConfig: (config: EndpointConfig) => void;
  setVlmEnabled: (enabled: boolean) => void;
  setMcpCommand: (cmd: string) => void;
  setActiveNode: (id: string | null) => void;
  setExecutorState: (state: "idle" | "running") => void;
  runWorkflow: () => Promise<void>;
  stopWorkflow: () => Promise<void>;
  setAllowAiTransforms: (allow: boolean) => void;
  setAllowAgentSteps: (allow: boolean) => void;
  planWorkflow: (intent: string) => Promise<void>;
  applyPlannedWorkflow: () => void;
  discardPlannedWorkflow: () => void;
  setShowPlannerModal: (show: boolean) => void;
  skipIntentEntry: () => void;
  setShowAssistant: (show: boolean) => void;
  patchWorkflow: (prompt: string) => Promise<void>;
  applyPatch: () => void;
  discardPatch: () => void;
}

export const DEFAULT_ENDPOINT: EndpointConfig = {
  baseUrl: "http://localhost:1234/v1",
  apiKey: "",
  model: "local",
};

export const DEFAULT_VLM_ENABLED = false;
export const DEFAULT_MCP_COMMAND = "npx";

export function makeDefaultWorkflow(): Workflow {
  return {
    id: crypto.randomUUID(),
    name: "New Workflow",
    nodes: [],
    edges: [],
  };
}
