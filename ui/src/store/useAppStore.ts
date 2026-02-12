import { useState, useCallback, useRef, useEffect } from "react";
import { commands } from "../bindings";
import type { Workflow, Node, NodeType, Edge, RunRequest, PlanRequest, PatchRequest, WorkflowPatch, NodeTypeInfo } from "../bindings";
import type { AppState, AppActions, DetailTab, EndpointConfig } from "./state";
import { DEFAULT_ENDPOINT, DEFAULT_MCP_COMMAND, DEFAULT_VLM_ENABLED, makeDefaultWorkflow } from "./state";
import { loadSettings, saveSetting, toEndpoint } from "./settings";
import type { PersistedSettings } from "./settings";

export type { DetailTab, EndpointConfig, AppState, AppActions } from "./state";

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
  const [vlmConfig, setVlmConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [vlmEnabled, setVlmEnabled] = useState(DEFAULT_VLM_ENABLED);
  const [mcpCommand, setMcpCommand] = useState(DEFAULT_MCP_COMMAND);

  // ── Settings persistence ────────────────────────────────────

  const settingsLoaded = useRef(false);
  useEffect(() => {
    if (settingsLoaded.current) return;
    settingsLoaded.current = true;
    loadSettings()
      .then((s) => {
        setPlannerConfig(s.plannerConfig);
        setAgentConfig(s.agentConfig);
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

  function persist<K extends keyof PersistedSettings>(
    key: K,
    setter: (value: PersistedSettings[K]) => void,
  ) {
    return (value: PersistedSettings[K]) => {
      setter(value);
      saveSetting(key, value).catch((e) =>
        console.error(`Failed to save setting "${key}":`, e),
      );
    };
  }

  // ── Logging ─────────────────────────────────────────────────

  const pushLog = useCallback((msg: string) => {
    setLogs((prev) => {
      const next = [...prev, msg];
      return next.length > 1000 ? next.slice(-1000) : next;
    });
  }, []);

  const clearLogs = useCallback(() => setLogs([]), []);

  // ── Workflow mutations ──────────────────────────────────────

  const addNode = useCallback(
    (nodeType: NodeType) => {
      const id = crypto.randomUUID();
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

  const removeNode = useCallback((id: string) => {
    setWorkflow((prev) => ({
      ...prev,
      nodes: prev.nodes.filter((n) => n.id !== id),
      edges: prev.edges.filter((e) => e.from !== id && e.to !== id),
    }));
    setSelectedNode((prev) => (prev === id ? null : prev));
  }, []);

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

  const addEdge = useCallback((from: string, to: string) => {
    setWorkflow((prev) => {
      const filtered = prev.edges.filter((e) => e.from !== from);
      const edge: Edge = { from, to };
      return { ...prev, edges: [...filtered, edge] };
    });
  }, []);

  const removeEdge = useCallback((from: string, to: string) => {
    setWorkflow((prev) => ({
      ...prev,
      edges: prev.edges.filter((e) => !(e.from === from && e.to === to)),
    }));
  }, []);

  // ── Project IO ──────────────────────────────────────────────

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

  // ── UI toggles ─────────────────────────────────────────────

  const toggleSidebar = useCallback(() => setSidebarCollapsed((p) => !p), []);
  const toggleLogsDrawer = useCallback(() => setLogsDrawerOpen((p) => !p), []);

  // ── Planner actions ─────────────────────────────────────────

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

  // ── Assistant/patcher actions ───────────────────────────────

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

  const applyPatch = useCallback(async () => {
    if (!assistantPatch) return;
    const removedEdgeKeys = new Set(
      assistantPatch.removed_edges.map((e) => `${e.from}-${e.to}`),
    );
    const nodes = [
      ...workflow.nodes
        .filter((n) => !assistantPatch.removed_node_ids.includes(n.id))
        .map((n) => assistantPatch.updated_nodes.find((u) => u.id === n.id) ?? n),
      ...assistantPatch.added_nodes,
    ];
    const edges = [
      ...workflow.edges.filter((e) => !removedEdgeKeys.has(`${e.from}-${e.to}`)),
      ...assistantPatch.added_edges,
    ];
    const patched: Workflow = { ...workflow, nodes, edges };
    try {
      const validation = await commands.validate(patched);
      if (!validation.valid) {
        pushLog(`Patch rejected: ${validation.errors.join(", ")}`);
        return;
      }
    } catch (e) {
      pushLog(`Patch validation failed: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }
    setWorkflow(patched);
    setAssistantPatch(null);
    setAssistantError(null);
    setShowAssistant(false);
    pushLog("Applied assistant changes");
  }, [assistantPatch, workflow, pushLog]);

  const discardPatch = useCallback(() => {
    setAssistantPatch(null);
    setAssistantError(null);
    setShowAssistant(false);
  }, []);

  // ── Executor actions ────────────────────────────────────────

  const latestRef = useRef({ workflow, projectPath, agentConfig, vlmConfig, vlmEnabled, mcpCommand });
  latestRef.current = { workflow, projectPath, agentConfig, vlmConfig, vlmEnabled, mcpCommand };

  const runWorkflow = useCallback(async () => {
    const { workflow, projectPath, agentConfig, vlmConfig, vlmEnabled, mcpCommand } = latestRef.current;
    const request: RunRequest = {
      workflow,
      project_path: projectPath,
      agent: toEndpoint(agentConfig),
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

  // ── Compose state + actions ─────────────────────────────────

  const state: AppState = {
    workflow, projectPath, nodeTypes, selectedNode, activeNode, executorState,
    detailTab, sidebarCollapsed, logsDrawerOpen, nodeSearch, showSettings,
    isNewWorkflow, showPlannerModal, plannerLoading, plannerError,
    pendingWorkflow, plannerWarnings, allowAiTransforms, allowAgentSteps,
    showAssistant, assistantLoading, assistantError, assistantPatch,
    logs, plannerConfig, agentConfig, vlmConfig, vlmEnabled, mcpCommand,
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
