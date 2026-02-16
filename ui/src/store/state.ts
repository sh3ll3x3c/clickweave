import type { Workflow, NodeTypeInfo, WorkflowPatch, ConversationSession } from "../bindings";

export type DetailTab = "setup" | "trace" | "checks" | "runs";

export interface EndpointConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

export function makeEmptyConversation(): ConversationSession {
  return { messages: [], summary: null, summary_cutoff: 0 };
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
  allowAiTransforms: boolean;
  allowAgentSteps: boolean;
  assistantOpen: boolean;
  assistantLoading: boolean;
  assistantError: string | null;
  conversation: ConversationSession;
  pendingPatch: WorkflowPatch | null;
  pendingPatchWarnings: string[];
  logs: string[];
  plannerConfig: EndpointConfig;
  agentConfig: EndpointConfig;
  vlmConfig: EndpointConfig;
  vlmEnabled: boolean;
  mcpCommand: string;
  maxRepairAttempts: number;
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
  removeNodes: (ids: string[]) => void;
  updateNodePositions: (updates: Map<string, { x: number; y: number }>) => void;
  updateNode: (id: string, updates: Partial<import("../bindings").Node>) => void;
  addEdge: (from: string, to: string, sourceHandle?: string) => void;
  removeEdge: (from: string, to: string, output?: import("../bindings").EdgeOutput | null) => void;
  openProject: () => Promise<void>;
  saveProject: () => Promise<void>;
  newProject: () => void;
  setPlannerConfig: (config: EndpointConfig) => void;
  setAgentConfig: (config: EndpointConfig) => void;
  setVlmConfig: (config: EndpointConfig) => void;
  setVlmEnabled: (enabled: boolean) => void;
  setMcpCommand: (cmd: string) => void;
  setMaxRepairAttempts: (n: number) => void;
  setActiveNode: (id: string | null) => void;
  setExecutorState: (state: "idle" | "running") => void;
  runWorkflow: () => Promise<void>;
  stopWorkflow: () => Promise<void>;
  setAllowAiTransforms: (allow: boolean) => void;
  setAllowAgentSteps: (allow: boolean) => void;
  skipIntentEntry: () => void;
  setAssistantOpen: (open: boolean) => void;
  toggleAssistant: () => void;
  sendAssistantMessage: (message: string) => Promise<void>;
  resendMessage: (index: number) => Promise<void>;
  applyPendingPatch: () => Promise<void>;
  discardPendingPatch: () => void;
  cancelAssistantChat: () => Promise<void>;
  clearConversation: () => void;
  setVerdicts: (verdicts: import("./slices/verdictSlice").NodeVerdict[]) => void;
  clearVerdicts: () => void;
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
