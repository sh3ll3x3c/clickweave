import { useState, useCallback, useRef, useEffect } from "react";
import { load } from "@tauri-apps/plugin-store";
import { commands } from "../bindings";
import type { Workflow, NodeTypeInfo, Node, NodeType, Edge, RunRequest, PlanRequest, PatchRequest, WorkflowPatch } from "../bindings";

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
  transformConfig: EndpointConfig;
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
  addNode: (nodeType: NodeType) => void;
  removeNode: (id: string) => void;
  updateNodePositions: (updates: Map<string, { x: number; y: number }>) => void;
  updateNode: (id: string, updates: Partial<Node>) => void;
  addEdge: (from: string, to: string) => void;
  removeEdge: (from: string, to: string) => void;
  openProject: () => Promise<void>;
  saveProject: () => Promise<void>;
  newProject: () => void;
  setPlannerConfig: (config: EndpointConfig) => void;
  setAgentConfig: (config: EndpointConfig) => void;
  setTransformConfig: (config: EndpointConfig) => void;
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

const DEFAULT_ENDPOINT: EndpointConfig = {
  baseUrl: "http://localhost:1234/v1",
  apiKey: "",
  model: "local",
};

const DEFAULT_VLM_ENABLED = false;
const DEFAULT_MCP_COMMAND = "npx";

interface PersistedSettings {
  plannerConfig: EndpointConfig;
  agentConfig: EndpointConfig;
  transformConfig: EndpointConfig;
  vlmConfig: EndpointConfig;
  vlmEnabled: boolean;
  mcpCommand: string;
}

const SETTINGS_DEFAULTS: PersistedSettings = {
  plannerConfig: DEFAULT_ENDPOINT,
  agentConfig: DEFAULT_ENDPOINT,
  transformConfig: DEFAULT_ENDPOINT,
  vlmConfig: DEFAULT_ENDPOINT,
  vlmEnabled: DEFAULT_VLM_ENABLED,
  mcpCommand: DEFAULT_MCP_COMMAND,
};

async function loadSettings(): Promise<PersistedSettings> {
  const store = await load("settings.json", { autoSave: false, defaults: {} });

  // Backward compat: if legacy orchestratorConfig exists, use it as fallback for new configs
  const legacyConfig = await store.get<EndpointConfig>("orchestratorConfig");
  const fallback = legacyConfig ?? SETTINGS_DEFAULTS.agentConfig;

  const plannerConfig = await store.get<EndpointConfig>("plannerConfig");
  const agentConfig = await store.get<EndpointConfig>("agentConfig");
  const transformConfig = await store.get<EndpointConfig>("transformConfig");
  const vlmConfig = await store.get<EndpointConfig>("vlmConfig");
  const vlmEnabled = await store.get<boolean>("vlmEnabled");
  const mcpCommand = await store.get<string>("mcpCommand");
  return {
    plannerConfig: plannerConfig ?? fallback,
    agentConfig: agentConfig ?? fallback,
    transformConfig: transformConfig ?? fallback,
    vlmConfig: vlmConfig ?? SETTINGS_DEFAULTS.vlmConfig,
    vlmEnabled: vlmEnabled ?? SETTINGS_DEFAULTS.vlmEnabled,
    mcpCommand: mcpCommand ?? SETTINGS_DEFAULTS.mcpCommand,
  };
}

async function saveSetting<K extends keyof PersistedSettings>(key: K, value: PersistedSettings[K]): Promise<void> {
  const store = await load("settings.json", { autoSave: false, defaults: {} });
  await store.set(key, value);
  await store.save();
}

function makeDefaultWorkflow(): Workflow {
  return {
    id: crypto.randomUUID(),
    name: "New Workflow",
    nodes: [],
    edges: [],
  };
}

export function useAppStore(): [AppState, AppActions] {
  const [workflow, setWorkflow] = useState<Workflow>(makeDefaultWorkflow);
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [nodeTypes, setNodeTypes] = useState<NodeTypeInfo[]>([]);
  const [selectedNode, setSelectedNode] = useState<string | null>(null);
  const [activeNode, setActiveNode] = useState<string | null>(null);
  const [executorState, setExecutorState] = useState<"idle" | "running">("idle");
  const [detailTab, setDetailTab] = useState<DetailTab>("setup");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [logsDrawerOpen, setLogsDrawerOpen] = useState(false);
  const [nodeSearch, setNodeSearch] = useState("");
  const [showSettings, setShowSettings] = useState(false);
  const [isNewWorkflow, setIsNewWorkflow] = useState(true);
  const [showPlannerModal, setShowPlannerModal] = useState(false);
  const [plannerLoading, setPlannerLoading] = useState(false);
  const [plannerError, setPlannerError] = useState<string | null>(null);
  const [pendingWorkflow, setPendingWorkflow] = useState<Workflow | null>(null);
  const [plannerWarnings, setPlannerWarnings] = useState<string[]>([]);
  const [allowAiTransforms, setAllowAiTransforms] = useState(true);
  const [allowAgentSteps, setAllowAgentSteps] = useState(false);
  const [showAssistant, setShowAssistant] = useState(false);
  const [assistantLoading, setAssistantLoading] = useState(false);
  const [assistantError, setAssistantError] = useState<string | null>(null);
  const [assistantPatch, setAssistantPatch] = useState<WorkflowPatch | null>(null);
  const [logs, setLogs] = useState<string[]>(["Clickweave started"]);
  const [plannerConfig, setPlannerConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [agentConfig, setAgentConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [transformConfig, setTransformConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [vlmConfig, setVlmConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [vlmEnabled, setVlmEnabled] = useState(DEFAULT_VLM_ENABLED);
  const [mcpCommand, setMcpCommand] = useState(DEFAULT_MCP_COMMAND);

  const settingsLoaded = useRef(false);
  useEffect(() => {
    if (settingsLoaded.current) return;
    settingsLoaded.current = true;
    loadSettings()
      .then((s) => {
        setPlannerConfig(s.plannerConfig);
        setAgentConfig(s.agentConfig);
        setTransformConfig(s.transformConfig);
        setVlmConfig(s.vlmConfig);
        setVlmEnabled(s.vlmEnabled);
        setMcpCommand(s.mcpCommand);
      })
      .catch((e) => console.error("Failed to load settings:", e));
  }, []);

  const nodeTypesLoaded = useRef(false);
  useEffect(() => {
    if (nodeTypesLoaded.current) return;
    nodeTypesLoaded.current = true;
    commands
      .nodeTypeDefaults()
      .then(setNodeTypes)
      .catch((e) => console.error("Failed to load node type defaults:", e));
  }, []);

  const pushLog = useCallback((msg: string) => {
    setLogs((prev) => {
      const next = [...prev, msg];
      return next.length > 1000 ? next.slice(-1000) : next;
    });
  }, []);

  const clearLogs = useCallback(() => setLogs([]), []);

  const addNode = useCallback(
    (nodeType: NodeType) => {
      const id = crypto.randomUUID();
      // Position new nodes with some offset based on count
      const offsetX = (workflow.nodes.length % 4) * 250;
      const offsetY = Math.floor(workflow.nodes.length / 4) * 150;
      const node: Node = {
        id,
        node_type: nodeType,
        position: { x: 200 + offsetX, y: 150 + offsetY },
        name: nodeType.type === "AiStep" ? "AI Step" : nodeType.type.replace(/([A-Z])/g, " $1").trim(),
        enabled: true,
        timeout_ms: null,
        retries: 0,
        trace_level: "Minimal",
        expected_outcome: null,
        checks: [],
      };
      setWorkflow((prev) => ({ ...prev, nodes: [...prev.nodes, node] }));
      setSelectedNode(id);
    },
    [workflow.nodes.length],
  );

  const removeNode = useCallback(
    (id: string) => {
      setWorkflow((prev) => ({
        ...prev,
        nodes: prev.nodes.filter((n) => n.id !== id),
        edges: prev.edges.filter((e) => e.from !== id && e.to !== id),
      }));
      setSelectedNode((prev) => (prev === id ? null : prev));
    },
    [],
  );

  const updateNodePositions = useCallback(
    (updates: Map<string, { x: number; y: number }>) => {
      setWorkflow((prev) => ({
        ...prev,
        nodes: prev.nodes.map((n) => {
          const pos = updates.get(n.id);
          return pos ? { ...n, position: { x: pos.x, y: pos.y } } : n;
        }),
      }));
    },
    [],
  );

  const updateNode = useCallback((id: string, updates: Partial<Node>) => {
    setWorkflow((prev) => ({
      ...prev,
      nodes: prev.nodes.map((n) => (n.id === id ? { ...n, ...updates } : n)),
    }));
  }, []);

  const addEdge = useCallback(
    (from: string, to: string) => {
      setWorkflow((prev) => {
        // Enforce max 1 outgoing edge per node
        const filtered = prev.edges.filter((e) => e.from !== from);
        const edge: Edge = { from, to };
        return { ...prev, edges: [...filtered, edge] };
      });
    },
    [],
  );

  const removeEdge = useCallback(
    (from: string, to: string) => {
      setWorkflow((prev) => ({
        ...prev,
        edges: prev.edges.filter((e) => !(e.from === from && e.to === to)),
      }));
    },
    [],
  );

  const openProject = useCallback(async () => {
    const result = await commands.pickWorkflowFile();
    if (result.status !== "ok" || !result.data) return;
    const filePath = result.data;
    const projectResult = await commands.openProject(filePath);
    if (projectResult.status !== "ok") {
      pushLog(`Failed to open: ${projectResult.error}`);
      return;
    }
    setProjectPath(projectResult.data.path);
    setWorkflow(projectResult.data.workflow);
    setSelectedNode(null);
    setIsNewWorkflow(false);
    pushLog(`Opened: ${filePath}`);
  }, [pushLog]);

  const saveProject = useCallback(async () => {
    let savePath = projectPath;
    if (!savePath) {
      const result = await commands.pickSaveFile();
      if (result.status !== "ok" || !result.data) return;
      savePath = result.data;
      setProjectPath(savePath);
    }
    const saveResult = await commands.saveProject(savePath, workflow);
    if (saveResult.status !== "ok") {
      pushLog(`Failed to save: ${saveResult.error}`);
      return;
    }
    pushLog(projectPath ? "Saved" : `Saved to: ${savePath}`);
  }, [projectPath, workflow, pushLog]);

  const newProject = useCallback(() => {
    setWorkflow(makeDefaultWorkflow());
    setProjectPath(null);
    setSelectedNode(null);
    setIsNewWorkflow(true);
    setPendingWorkflow(null);
    setPlannerError(null);
    pushLog("New project created");
  }, [pushLog]);

  const toggleSidebar = useCallback(() => setSidebarCollapsed((p) => !p), []);
  const toggleLogsDrawer = useCallback(() => setLogsDrawerOpen((p) => !p), []);

  function persist<K extends keyof PersistedSettings>(
    key: K,
    setter: (value: PersistedSettings[K]) => void,
  ) {
    return (value: PersistedSettings[K]) => {
      setter(value);
      saveSetting(key, value);
    };
  }

  const toEndpoint = (c: EndpointConfig) => ({
    base_url: c.baseUrl,
    model: c.model,
    api_key: c.apiKey || null,
  });

  const plannerRef = useRef({ plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand });
  plannerRef.current = { plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand };

  const planWorkflowAction = useCallback(async (intent: string) => {
    const { plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand } = plannerRef.current;
    setPlannerLoading(true);
    setPlannerError(null);
    setPendingWorkflow(null);
    setPlannerWarnings([]);
    try {
      const request: PlanRequest = {
        intent,
        planner: toEndpoint(plannerConfig),
        allow_ai_transforms: allowAiTransforms,
        allow_agent_steps: allowAgentSteps,
        mcp_command: mcpCommand,
      };
      const result = await commands.planWorkflow(request);
      if (result.status === "ok") {
        setPendingWorkflow(result.data.workflow);
        setPlannerWarnings(result.data.warnings);
        pushLog(`Planner generated workflow with ${result.data.workflow.nodes.length} nodes`);
      } else {
        setPlannerError(result.error);
        pushLog(`Planning failed: ${result.error}`);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setPlannerError(msg);
      pushLog(`Planning error: ${msg}`);
    } finally {
      setPlannerLoading(false);
    }
  }, [pushLog]);

  const applyPlannedWorkflow = useCallback(() => {
    if (!pendingWorkflow) return;
    setWorkflow(pendingWorkflow);
    setPendingWorkflow(null);
    setPlannerWarnings([]);
    setPlannerError(null);
    setIsNewWorkflow(false);
    setShowPlannerModal(false);
    pushLog("Applied planned workflow");
  }, [pendingWorkflow, pushLog]);

  const discardPlannedWorkflow = useCallback(() => {
    setPendingWorkflow(null);
    setPlannerWarnings([]);
    setPlannerError(null);
    setShowPlannerModal(false);
  }, []);

  const workflowRef = useRef(workflow);
  workflowRef.current = workflow;

  const patchWorkflowAction = useCallback(async (userPrompt: string) => {
    const { plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand } = plannerRef.current;
    setAssistantLoading(true);
    setAssistantError(null);
    setAssistantPatch(null);
    try {
      const request: PatchRequest = {
        workflow: workflowRef.current,
        user_prompt: userPrompt,
        planner: toEndpoint(plannerConfig),
        allow_ai_transforms: allowAiTransforms,
        allow_agent_steps: allowAgentSteps,
        mcp_command: mcpCommand,
      };
      const result = await commands.patchWorkflow(request);
      if (result.status === "ok") {
        setAssistantPatch(result.data);
        const total = result.data.added_nodes.length + result.data.removed_node_ids.length + result.data.updated_nodes.length;
        pushLog(`Assistant generated ${total} changes`);
      } else {
        setAssistantError(result.error);
        pushLog(`Patch failed: ${result.error}`);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setAssistantError(msg);
      pushLog(`Patch error: ${msg}`);
    } finally {
      setAssistantLoading(false);
    }
  }, [pushLog]);

  const applyPatch = useCallback(() => {
    if (!assistantPatch) return;
    setWorkflow((prev) => {
      // Remove nodes
      let nodes = prev.nodes.filter((n) => !assistantPatch.removed_node_ids.includes(n.id));
      // Apply updates
      nodes = nodes.map((n) => {
        const update = assistantPatch.updated_nodes.find((u) => u.id === n.id);
        return update ?? n;
      });
      // Add new nodes
      nodes = [...nodes, ...assistantPatch.added_nodes];
      // Remove edges
      const removedEdgeKeys = new Set(
        assistantPatch.removed_edges.map((e) => `${e.from}-${e.to}`),
      );
      let edges = prev.edges.filter((e) => !removedEdgeKeys.has(`${e.from}-${e.to}`));
      // Add new edges
      edges = [...edges, ...assistantPatch.added_edges];
      return { ...prev, nodes, edges };
    });
    setAssistantPatch(null);
    setAssistantError(null);
    setShowAssistant(false);
    pushLog("Applied assistant changes");
  }, [assistantPatch, pushLog]);

  const discardPatch = useCallback(() => {
    setAssistantPatch(null);
    setAssistantError(null);
    setShowAssistant(false);
  }, []);

  const latestRef = useRef({ workflow, projectPath, agentConfig, transformConfig, vlmConfig, vlmEnabled, mcpCommand });
  latestRef.current = { workflow, projectPath, agentConfig, transformConfig, vlmConfig, vlmEnabled, mcpCommand };

  const runWorkflow = useCallback(async () => {
    const { workflow, projectPath, agentConfig, transformConfig, vlmConfig, vlmEnabled, mcpCommand } = latestRef.current;
    const request: RunRequest = {
      workflow,
      project_path: projectPath,
      agent: toEndpoint(agentConfig),
      transform: toEndpoint(transformConfig),
      vlm: vlmEnabled ? toEndpoint(vlmConfig) : null,
      mcp_command: mcpCommand,
    };
    const result = await commands.runWorkflow(request);
    if (result.status === "error") {
      pushLog(`Run failed: ${result.error}`);
    }
  }, [pushLog]);

  const stopWorkflow = useCallback(async () => {
    const result = await commands.stopWorkflow();
    if (result.status === "error") {
      pushLog(`Stop failed: ${result.error}`);
    }
  }, [pushLog]);

  const state: AppState = {
    workflow,
    projectPath,
    nodeTypes,
    selectedNode,
    activeNode,
    executorState,
    detailTab,
    sidebarCollapsed,
    logsDrawerOpen,
    nodeSearch,
    showSettings,
    isNewWorkflow,
    showPlannerModal,
    plannerLoading,
    plannerError,
    pendingWorkflow,
    plannerWarnings,
    allowAiTransforms,
    allowAgentSteps,
    showAssistant,
    assistantLoading,
    assistantError,
    assistantPatch,
    logs,
    plannerConfig,
    agentConfig,
    transformConfig,
    vlmConfig,
    vlmEnabled,
    mcpCommand,
  };

  const actions: AppActions = {
    setWorkflow,
    selectNode: setSelectedNode,
    setDetailTab,
    toggleSidebar,
    toggleLogsDrawer,
    setNodeSearch,
    setShowSettings,
    pushLog,
    clearLogs,
    addNode,
    removeNode,
    updateNodePositions,
    updateNode,
    addEdge,
    removeEdge,
    openProject,
    saveProject,
    newProject,
    setPlannerConfig: persist("plannerConfig", setPlannerConfig),
    setAgentConfig: persist("agentConfig", setAgentConfig),
    setTransformConfig: persist("transformConfig", setTransformConfig),
    setVlmConfig: persist("vlmConfig", setVlmConfig),
    setVlmEnabled: persist("vlmEnabled", setVlmEnabled),
    setMcpCommand: persist("mcpCommand", setMcpCommand),
    setActiveNode,
    setExecutorState,
    runWorkflow,
    stopWorkflow,
    setAllowAiTransforms,
    setAllowAgentSteps,
    planWorkflow: planWorkflowAction,
    applyPlannedWorkflow,
    discardPlannedWorkflow,
    setShowPlannerModal,
    skipIntentEntry: () => setIsNewWorkflow(false),
    setShowAssistant,
    patchWorkflow: patchWorkflowAction,
    applyPatch,
    discardPatch,
  };

  return [state, actions];
}
