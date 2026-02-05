import { useState, useCallback, useRef } from "react";
import { commands } from "../bindings";
import type { Workflow, NodeTypeInfo, Node, NodeType, Edge, RunRequest } from "../bindings";

export type DetailTab = "setup" | "trace" | "checks" | "runs";

export interface LlmConfig {
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
  llmConfig: LlmConfig;
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
  updateNode: (id: string, updates: Partial<Node>) => void;
  addEdge: (from: string, to: string) => void;
  removeEdge: (from: string, to: string) => void;
  openProject: () => Promise<void>;
  saveProject: () => Promise<void>;
  newProject: () => void;
  setLlmConfig: (config: LlmConfig) => void;
  setMcpCommand: (cmd: string) => void;
  setActiveNode: (id: string | null) => void;
  setExecutorState: (state: "idle" | "running") => void;
  runWorkflow: () => Promise<void>;
  stopWorkflow: () => Promise<void>;
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
  const [llmConfig, setLlmConfig] = useState<LlmConfig>({
    baseUrl: "http://localhost:1234/v1",
    apiKey: "",
    model: "local",
  });
  const [mcpCommand, setMcpCommand] = useState("npx");

  const nodeTypesLoaded = useRef(false);
  if (!nodeTypesLoaded.current) {
    nodeTypesLoaded.current = true;
    commands.nodeTypeDefaults().then(setNodeTypes);
  }

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
    const result = await commands.pickProjectFolder();
    if (result.status !== "ok" || !result.data) return;
    const path = result.data;
    const projectResult = await commands.openProject(path);
    if (projectResult.status !== "ok") {
      pushLog(`Failed to open project: ${projectResult.error}`);
      return;
    }
    setProjectPath(projectResult.data.path);
    setWorkflow(projectResult.data.workflow);
    setSelectedNode(null);
    pushLog(`Opened project: ${path}`);
  }, [pushLog]);

  const saveProject = useCallback(async () => {
    if (!projectPath) {
      const result = await commands.pickProjectFolder();
      if (result.status !== "ok" || !result.data) return;
      setProjectPath(result.data);
      const saveResult = await commands.saveProject(result.data, workflow);
      if (saveResult.status !== "ok") {
        pushLog(`Failed to save: ${saveResult.error}`);
        return;
      }
      pushLog(`Saved to: ${result.data}`);
      return;
    }
    const saveResult = await commands.saveProject(projectPath, workflow);
    if (saveResult.status !== "ok") {
      pushLog(`Failed to save: ${saveResult.error}`);
      return;
    }
    pushLog("Saved");
  }, [projectPath, workflow, pushLog]);

  const newProject = useCallback(() => {
    setWorkflow(makeDefaultWorkflow());
    setProjectPath(null);
    setSelectedNode(null);
    pushLog("New project created");
  }, [pushLog]);

  const workflowRef = useRef(workflow);
  workflowRef.current = workflow;
  const projectPathRef = useRef(projectPath);
  projectPathRef.current = projectPath;
  const llmConfigRef = useRef(llmConfig);
  llmConfigRef.current = llmConfig;
  const mcpCommandRef = useRef(mcpCommand);
  mcpCommandRef.current = mcpCommand;

  const runWorkflow = useCallback(async () => {
    const request: RunRequest = {
      workflow: workflowRef.current,
      project_path: projectPathRef.current,
      llm_base_url: llmConfigRef.current.baseUrl,
      llm_model: llmConfigRef.current.model,
      llm_api_key: llmConfigRef.current.apiKey || null,
      mcp_command: mcpCommandRef.current,
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
    llmConfig,
    mcpCommand,
  };

  const actions: AppActions = {
    setWorkflow,
    selectNode: setSelectedNode,
    setDetailTab,
    toggleSidebar: () => setSidebarCollapsed((p) => !p),
    toggleLogsDrawer: () => setLogsDrawerOpen((p) => !p),
    setNodeSearch,
    setShowSettings,
    pushLog,
    clearLogs,
    addNode,
    removeNode,
    updateNode,
    addEdge,
    removeEdge,
    openProject,
    saveProject,
    newProject,
    setLlmConfig,
    setMcpCommand,
    setActiveNode,
    setExecutorState,
    runWorkflow,
    stopWorkflow,
  };

  return [state, actions];
}
