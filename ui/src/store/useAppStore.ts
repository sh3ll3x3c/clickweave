import { useState, useCallback, useRef, useEffect } from "react";
import { commands } from "../bindings";
import type { Workflow, RunRequest, AssistantChatRequest, WorkflowPatch, ConversationData, ChatEntryDto, NodeTypeInfo } from "../bindings";
import type { AppState, AppActions, DetailTab, EndpointConfig, ChatEntryLocal } from "./state";
import { DEFAULT_ENDPOINT, DEFAULT_MCP_COMMAND, DEFAULT_VLM_ENABLED, makeDefaultWorkflow, makeEmptyConversation } from "./state";
import type { ConversationSession } from "./state";
import { loadSettings, saveSetting, toEndpoint } from "./settings";
import type { PersistedSettings } from "./settings";
import { useWorkflowMutations } from "./useWorkflowMutations";

export type { DetailTab, EndpointConfig, AppState, AppActions } from "./state";

function localEntryToDto(e: ChatEntryLocal): ChatEntryDto {
  return {
    role: e.role,
    content: e.content,
    timestamp: e.timestamp,
    patch_summary: e.patchSummary ? {
      added: e.patchSummary.added,
      removed: e.patchSummary.removed,
      updated: e.patchSummary.updated,
      added_names: e.patchSummary.addedNames,
      removed_names: e.patchSummary.removedNames,
      updated_names: e.patchSummary.updatedNames,
      description: e.patchSummary.description ?? null,
    } : null,
    run_context: e.runContext ? {
      execution_dir: e.runContext.executionDir,
      node_results: e.runContext.nodeResults.map(nr => ({
        node_name: nr.nodeName,
        status: nr.status,
        error: nr.error ?? null,
      })),
    } : null,
  };
}

function dtoEntryToLocal(m: ChatEntryDto): ChatEntryLocal {
  return {
    role: m.role as "user" | "assistant",
    content: m.content,
    timestamp: m.timestamp,
    patchSummary: m.patch_summary ? {
      added: m.patch_summary.added,
      removed: m.patch_summary.removed,
      updated: m.patch_summary.updated,
      addedNames: m.patch_summary.added_names ?? [],
      removedNames: m.patch_summary.removed_names ?? [],
      updatedNames: m.patch_summary.updated_names ?? [],
      description: m.patch_summary.description ?? undefined,
    } : undefined,
    runContext: m.run_context ? {
      executionDir: m.run_context.execution_dir,
      nodeResults: m.run_context.node_results.map(nr => ({
        nodeName: nr.node_name,
        status: nr.status,
        error: nr.error ?? undefined,
      })),
    } : undefined,
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
  const [allowAiTransforms, setAllowAiTransforms] = useState(true);
  const [allowAgentSteps, setAllowAgentSteps] = useState(false);
  const [assistantOpen, setAssistantOpen] = useState(false);
  const [assistantLoading, setAssistantLoading] = useState(false);
  const [assistantError, setAssistantError] = useState<string | null>(null);
  const [conversation, setConversation] = useState<ConversationSession>(makeEmptyConversation);
  const [pendingPatch, setPendingPatch] = useState<WorkflowPatch | null>(null);
  const [pendingPatchWarnings, setPendingPatchWarnings] = useState<string[]>([]);
  const [logs, setLogs] = useState<string[]>(["Clickweave started"]);
  const [plannerConfig, setPlannerConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [agentConfig, setAgentConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [vlmConfig, setVlmConfig] = useState<EndpointConfig>(DEFAULT_ENDPOINT);
  const [vlmEnabled, setVlmEnabled] = useState(DEFAULT_VLM_ENABLED);
  const [mcpCommand, setMcpCommand] = useState(DEFAULT_MCP_COMMAND);

  // ── Refs for stable callbacks ─────────────────────────────

  const conversationRef = useRef(conversation);
  conversationRef.current = conversation;

  const workflowRef = useRef(workflow);
  workflowRef.current = workflow;

  const plannerRef = useRef({ plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand });
  plannerRef.current = { plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand };

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

  const { addNode, removeNode, updateNodePositions, updateNode, addEdge, removeEdge } =
    useWorkflowMutations(setWorkflow, setSelectedNode, workflow.nodes.length);

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

    // Load conversation
    try {
      const convResult = await commands.loadConversation(filePath);
      if (convResult.status === "ok" && convResult.data) {
        setConversation({
          messages: convResult.data.messages.map(dtoEntryToLocal),
          summary: convResult.data.summary,
          summaryCutoff: convResult.data.summary_cutoff,
        });
      } else {
        setConversation(makeEmptyConversation());
      }
    } catch {
      setConversation(makeEmptyConversation());
    }

    pushLog(`Opened: ${filePath}`);
  }, [pushLog]);

  const projectPathRef = useRef(projectPath);
  projectPathRef.current = projectPath;

  const saveProject = useCallback(async () => {
    let savePath = projectPathRef.current;
    if (!savePath) {
      const result = await commands.pickSaveFile();
      if (result.status !== "ok" || !result.data) return;
      savePath = result.data;
      setProjectPath(savePath);
    }
    const saveResult = await commands.saveProject(savePath, workflowRef.current);
    if (saveResult.status !== "ok") {
      pushLog(`Failed to save: ${saveResult.error}`);
      return;
    }

    // Save conversation alongside the project
    if (savePath) {
      try {
        const conv = conversationRef.current;
        const convDto: ConversationData = {
          messages: conv.messages.map(localEntryToDto),
          summary: conv.summary,
          summary_cutoff: conv.summaryCutoff,
        };
        await commands.saveConversation(savePath, convDto);
      } catch (e) {
        console.error("Failed to save conversation:", e);
      }
    }

    pushLog(projectPathRef.current ? "Saved" : `Saved to: ${savePath}`);
  }, [pushLog]);

  const newProject = useCallback(() => {
    setWorkflow(makeDefaultWorkflow());
    setProjectPath(null);
    setSelectedNode(null);
    setIsNewWorkflow(true);
    setConversation(makeEmptyConversation());
    setPendingPatch(null);
    setPendingPatchWarnings([]);
    setAssistantError(null);
    pushLog("New project created");
  }, [pushLog]);

  // ── UI toggles ─────────────────────────────────────────────

  const toggleSidebar = useCallback(() => setSidebarCollapsed((p) => !p), []);
  const toggleLogsDrawer = useCallback(() => setLogsDrawerOpen((p) => !p), []);
  const toggleAssistant = useCallback(() => setAssistantOpen((p) => !p), []);

  // ── Assistant actions ──────────────────────────────────────

  const sendAssistantMessage = useCallback(async (message: string) => {
    const { plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand } = plannerRef.current;
    setAssistantLoading(true);
    setAssistantError(null);

    // Capture conversation state BEFORE adding the user message — the backend
    // receives the new message separately as `user_message`.
    const conv = conversationRef.current;

    const userEntry: ChatEntryLocal = {
      role: "user",
      content: message,
      timestamp: Date.now(),
    };
    setConversation(prev => ({
      ...prev,
      messages: [...prev.messages, userEntry],
    }));

    try {
      const historyDto = conv.messages.map(localEntryToDto);

      const request: AssistantChatRequest = {
        workflow: workflowRef.current,
        user_message: message,
        history: historyDto,
        summary: conv.summary,
        summary_cutoff: conv.summaryCutoff,
        run_context: null,
        planner: toEndpoint(plannerConfig),
        allow_ai_transforms: allowAiTransforms,
        allow_agent_steps: allowAgentSteps,
        mcp_command: mcpCommand,
      };

      const result = await commands.assistantChat(request);
      if (result.status === "ok") {
        const data = result.data;

        // Build patch summary if there's a patch
        let patchSummary: ChatEntryLocal["patchSummary"] | undefined;
        if (data.patch) {
          const currentNodes = workflowRef.current.nodes;
          const removedNames = data.patch.removed_node_ids.map(id => {
            const node = currentNodes.find(n => n.id === id);
            return node?.name ?? id;
          });
          patchSummary = {
            added: data.patch.added_nodes.length,
            removed: data.patch.removed_node_ids.length,
            updated: data.patch.updated_nodes.length,
            addedNames: data.patch.added_nodes.map(n => n.name),
            removedNames: removedNames,
            updatedNames: data.patch.updated_nodes.map(n => n.name),
          };
        }

        // Add assistant message to conversation
        const assistantEntry: ChatEntryLocal = {
          role: "assistant",
          content: data.assistant_message,
          timestamp: Date.now(),
          patchSummary,
        };

        setConversation(prev => ({
          messages: [...prev.messages, assistantEntry],
          summary: data.new_summary ?? prev.summary,
          summaryCutoff: data.summary_cutoff,
        }));

        if (data.patch) {
          setPendingPatch(data.patch);
          setPendingPatchWarnings(data.warnings);
        }

        pushLog(`Assistant: ${data.patch ? "generated changes" : "responded"}`);
      } else {
        setAssistantError(result.error);
        pushLog(`Assistant error: ${result.error}`);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setAssistantError(msg);
      pushLog(`Assistant error: ${msg}`);
    } finally {
      setAssistantLoading(false);
    }
  }, [pushLog]);

  const applyPendingPatch = useCallback(async () => {
    if (!pendingPatch) return;
    const removedEdgeKeys = new Set(
      pendingPatch.removed_edges.map((e) => `${e.from}-${e.to}`),
    );
    const nodes = [
      ...workflow.nodes
        .filter((n) => !pendingPatch.removed_node_ids.includes(n.id))
        .map((n) => pendingPatch.updated_nodes.find((u) => u.id === n.id) ?? n),
      ...pendingPatch.added_nodes,
    ];
    const edges = [
      ...workflow.edges.filter((e) => !removedEdgeKeys.has(`${e.from}-${e.to}`)),
      ...pendingPatch.added_edges,
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
    setPendingPatch(null);
    setPendingPatchWarnings([]);
    setAssistantError(null);
    setIsNewWorkflow(false);
    pushLog("Applied assistant changes");
  }, [pendingPatch, workflow, pushLog]);

  const discardPendingPatch = useCallback(() => {
    setPendingPatch(null);
    setPendingPatchWarnings([]);
    setAssistantError(null);
  }, []);

  const clearConversation = useCallback(() => {
    setConversation(makeEmptyConversation());
    setPendingPatch(null);
    setPendingPatchWarnings([]);
    setAssistantError(null);
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
    isNewWorkflow, allowAiTransforms, allowAgentSteps,
    assistantOpen, assistantLoading, assistantError,
    conversation, pendingPatch, pendingPatchWarnings,
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
    skipIntentEntry: () => setIsNewWorkflow(false),
    setAssistantOpen,
    toggleAssistant,
    sendAssistantMessage,
    applyPendingPatch,
    discardPendingPatch,
    clearConversation,
  };

  return [state, actions];
}
