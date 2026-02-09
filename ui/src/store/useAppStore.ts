import { useState, useCallback, useRef, useEffect } from "react";
import { load } from "@tauri-apps/plugin-store";
import { commands } from "../bindings";
import type { Workflow, NodeTypeInfo, Node, NodeType, Edge, RunRequest } from "../bindings";

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
  logs: string[];
  orchestratorConfig: EndpointConfig;
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
  setOrchestratorConfig: (config: EndpointConfig) => void;
  setVlmConfig: (config: EndpointConfig) => void;
  setVlmEnabled: (enabled: boolean) => void;
  setMcpCommand: (cmd: string) => void;
  setActiveNode: (id: string | null) => void;
  setExecutorState: (state: "idle" | "running") => void;
  runWorkflow: () => Promise<void>;
  stopWorkflow: () => Promise<void>;
}

const DEFAULT_ORCHESTRATOR_CONFIG: EndpointConfig = {
  baseUrl: "http://localhost:1234/v1",
  apiKey: "",
  model: "local",
};

const DEFAULT_VLM_CONFIG: EndpointConfig = {
  baseUrl: "http://localhost:1234/v1",
  apiKey: "",
  model: "local",
};

const DEFAULT_VLM_ENABLED = false;
const DEFAULT_MCP_COMMAND = "npx";

interface PersistedSettings {
  orchestratorConfig: EndpointConfig;
  vlmConfig: EndpointConfig;
  vlmEnabled: boolean;
  mcpCommand: string;
}

const SETTINGS_DEFAULTS: PersistedSettings = {
  orchestratorConfig: DEFAULT_ORCHESTRATOR_CONFIG,
  vlmConfig: DEFAULT_VLM_CONFIG,
  vlmEnabled: DEFAULT_VLM_ENABLED,
  mcpCommand: DEFAULT_MCP_COMMAND,
};

async function loadSettings(): Promise<PersistedSettings> {
  const store = await load("settings.json", { autoSave: false, defaults: {} });
  const orchestratorConfig = await store.get<EndpointConfig>("orchestratorConfig");
  const vlmConfig = await store.get<EndpointConfig>("vlmConfig");
  const vlmEnabled = await store.get<boolean>("vlmEnabled");
  const mcpCommand = await store.get<string>("mcpCommand");
  return {
    orchestratorConfig: orchestratorConfig ?? SETTINGS_DEFAULTS.orchestratorConfig,
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
  const [logs, setLogs] = useState<string[]>(["Clickweave started"]);
  const [orchestratorConfig, setOrchestratorConfig] = useState<EndpointConfig>(DEFAULT_ORCHESTRATOR_CONFIG);
  const [vlmConfig, setVlmConfig] = useState<EndpointConfig>(DEFAULT_VLM_CONFIG);
  const [vlmEnabled, setVlmEnabled] = useState(DEFAULT_VLM_ENABLED);
  const [mcpCommand, setMcpCommand] = useState(DEFAULT_MCP_COMMAND);

  const settingsLoaded = useRef(false);
  useEffect(() => {
    if (settingsLoaded.current) return;
    settingsLoaded.current = true;
    loadSettings().then((s) => {
      setOrchestratorConfig(s.orchestratorConfig);
      setVlmConfig(s.vlmConfig);
      setVlmEnabled(s.vlmEnabled);
      setMcpCommand(s.mcpCommand);
    });
  }, []);

  const nodeTypesLoaded = useRef(false);
  useEffect(() => {
    if (nodeTypesLoaded.current) return;
    nodeTypesLoaded.current = true;
    commands.nodeTypeDefaults().then(setNodeTypes);
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
    pushLog("New project created");
  }, [pushLog]);

  const toggleSidebar = useCallback(() => setSidebarCollapsed((p) => !p), []);
  const toggleLogsDrawer = useCallback(() => setLogsDrawerOpen((p) => !p), []);

  const persistOrchestratorConfig = useCallback((config: EndpointConfig) => {
    setOrchestratorConfig(config);
    saveSetting("orchestratorConfig", config);
  }, []);

  const persistVlmConfig = useCallback((config: EndpointConfig) => {
    setVlmConfig(config);
    saveSetting("vlmConfig", config);
  }, []);

  const persistVlmEnabled = useCallback((enabled: boolean) => {
    setVlmEnabled(enabled);
    saveSetting("vlmEnabled", enabled);
  }, []);

  const persistMcpCommand = useCallback((cmd: string) => {
    setMcpCommand(cmd);
    saveSetting("mcpCommand", cmd);
  }, []);

  const latestRef = useRef({ workflow, projectPath, orchestratorConfig, vlmConfig, vlmEnabled, mcpCommand });
  latestRef.current = { workflow, projectPath, orchestratorConfig, vlmConfig, vlmEnabled, mcpCommand };

  const runWorkflow = useCallback(async () => {
    const { workflow, projectPath, orchestratorConfig, vlmConfig, vlmEnabled, mcpCommand } = latestRef.current;
    const request: RunRequest = {
      workflow,
      project_path: projectPath,
      orchestrator: {
        base_url: orchestratorConfig.baseUrl,
        model: orchestratorConfig.model,
        api_key: orchestratorConfig.apiKey || null,
      },
      vlm: vlmEnabled ? {
        base_url: vlmConfig.baseUrl,
        model: vlmConfig.model,
        api_key: vlmConfig.apiKey || null,
      } : null,
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
    logs,
    orchestratorConfig,
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
    setOrchestratorConfig: persistOrchestratorConfig,
    setVlmConfig: persistVlmConfig,
    setVlmEnabled: persistVlmEnabled,
    setMcpCommand: persistMcpCommand,
    setActiveNode,
    setExecutorState,
    runWorkflow,
    stopWorkflow,
  };

  return [state, actions];
}
