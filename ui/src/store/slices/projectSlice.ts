import type { StateCreator } from "zustand";
import type { ProjectManifest, Workflow } from "../../bindings";
import { commands } from "../../bindings";
import { makeDefaultWorkflow, PROJECT_SCHEMA_VERSION } from "../state";
import { errorMessage } from "../../utils/commandError";
import type { StoreState } from "./types";
import { loadAgentChat } from "../agentChatPersistence";
import { loadLatestRunTrace } from "../runTracePersistence";
import { isAgentActive } from "./agentSlice";

export interface ProjectSlice {
  workflow: Workflow;
  projectPath: string | null;
  isNewWorkflow: boolean;

  setWorkflow: (w: Workflow) => void;
  openProject: () => Promise<void>;
  saveProject: () => Promise<void>;
  newProject: () => void;
  skipIntentEntry: () => void;
}

export const createProjectSlice: StateCreator<StoreState, [], [], ProjectSlice> = (set, get) => ({
  workflow: makeDefaultWorkflow(),
  projectPath: null,
  isNewWorkflow: true,

  setWorkflow: (w) => set({ workflow: w }),

  openProject: async () => {
    if (get().executorState === "running") {
      console.warn("Cannot open project during execution");
      return;
    }
    // Cross-project corruption guard (D1.C1 review): a live agent run
    // against workflow A would keep emitting events into workflow B's
    // graph/messages if we swapped the project out from under it.
    // Also block while a VLM completion-disagreement resolver is
    // pending — the backend task still owns this workflow's cache +
    // variant-index writes until the operator resolves.
    if (isAgentActive(get().agentStatus, get().completionDisagreement)) {
      get().setAssistantError(
        "Stop the agent before opening another project.",
      );
      return;
    }
    const { pushLog } = get();
    const result = await commands.pickWorkflowFile();
    if (result.status !== "ok" || !result.data) return;
    const filePath = result.data;
    const projectResult = await commands.openProject(filePath);
    if (projectResult.status !== "ok") {
      pushLog(`Failed to open: ${errorMessage(projectResult.error)}`);
      return;
    }
    set({
      projectPath: projectResult.data.path,
      // 1.G TOMBSTONE: deleted with canvas — `workflow.nodes/edges/groups`
      // are no longer carried by `ProjectData`. The slim `ProjectManifest`
      // (id/name/intent) is mirrored back into the legacy `Workflow`-shaped
      // slice field so canvas-tree consumers keep typechecking until 1.G
      // deletes them.
      workflow: {
        ...makeDefaultWorkflow(),
        id: projectResult.data.manifest.id,
        name: projectResult.data.manifest.name,
        intent: projectResult.data.manifest.intent ?? null,
      },
      selectedNode: null,
      isNewWorkflow: false,
      assistantError: null,
      messages: [],
      // Clear the previous project's run context so the Overview
      // cards don't display stale trace/elapsed data for the old project.
      agentRunId: null,
      agentRunStartedAt: null,
      agentRunFinishedAt: null,
      lastRunStatus: null,
      // Also clear the terminal run notice — the destructive-cap card
      // in AssistantThread reads this directly and would otherwise keep
      // showing the previous project's run-halted message.
      consecutiveDestructiveCapHit: null,
    });
    get().clearHistory();
    // Ambiguity resolutions are specific to the prior workflow's nodes.
    get().clearAmbiguityResolutions();

    // Hydrate the per-project chat transcript from disk. Best-effort
    // — missing or malformed files return an empty array.
    const rehydrated = await loadAgentChat({
      projectPath: projectResult.data.path,
      projectName: projectResult.data.manifest.name,
      projectId: projectResult.data.manifest.id,
    });
    if (rehydrated.length > 0) {
      get().setMessages(rehydrated);
    }

    const hydratedTrace = await loadLatestRunTrace({
      projectPath: projectResult.data.path,
      projectName: projectResult.data.manifest.name,
      projectId: projectResult.data.manifest.id,
      storeTraces: get().storeTraces,
    });
    if (hydratedTrace) {
      get().hydrateRunTrace(hydratedTrace);
    }

    pushLog(`Opened: ${filePath}`);
  },

  saveProject: async () => {
    const { projectPath, workflow, pushLog } = get();
    let savePath = projectPath;
    if (!savePath) {
      const result = await commands.pickSaveFile();
      if (result.status !== "ok" || !result.data) return;
      savePath = result.data;
      set({ projectPath: savePath });
    }
    // 1.G TOMBSTONE: legacy `workflow` slice still holds id/name/intent;
    // graph fields (nodes/edges/groups) are dropped on save now that
    // `ProjectManifest` is the on-disk shape.
    const manifest: ProjectManifest = {
      id: workflow.id,
      name: workflow.name,
      intent: workflow.intent ?? null,
      schema_version: PROJECT_SCHEMA_VERSION,
    };
    const saveResult = await commands.saveProject(savePath, manifest);
    if (saveResult.status !== "ok") {
      pushLog(`Failed to save: ${errorMessage(saveResult.error)}`);
      return;
    }

    pushLog(projectPath ? "Saved" : `Saved to: ${savePath}`);
  },

  newProject: () => {
    if (get().executorState === "running") {
      console.warn("Cannot create new project during execution");
      return;
    }
    if (isAgentActive(get().agentStatus, get().completionDisagreement)) {
      get().setAssistantError(
        "Stop the agent before creating a new project.",
      );
      return;
    }
    const { pushLog } = get();
    set({
      workflow: makeDefaultWorkflow(),
      projectPath: null,
      selectedNode: null,
      isNewWorkflow: true,
      messages: [],
      assistantError: null,
      // Clear the previous project's run context so the Overview
      // cards don't display stale trace/elapsed data for the old project.
      agentRunId: null,
      agentRunStartedAt: null,
      agentRunFinishedAt: null,
      lastRunStatus: null,
      // Also clear the terminal run notice — the destructive-cap card
      // in AssistantThread reads this directly and would otherwise keep
      // showing the previous project's run-halted message.
      consecutiveDestructiveCapHit: null,
    });
    get().clearHistory();
    get().clearAmbiguityResolutions();
    pushLog("New project created");
  },

  skipIntentEntry: () => set({ isNewWorkflow: false }),
});
