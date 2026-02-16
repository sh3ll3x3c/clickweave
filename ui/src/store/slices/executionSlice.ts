import type { StateCreator } from "zustand";
import type { RunRequest } from "../../bindings";
import { commands } from "../../bindings";
import { validateSingleGraph } from "../../utils/graphValidation";
import { toEndpoint } from "../settings";
import type { StoreState } from "./types";

export interface ExecutionSlice {
  executorState: "idle" | "running";

  setExecutorState: (state: "idle" | "running") => void;
  runWorkflow: () => Promise<void>;
  stopWorkflow: () => Promise<void>;
}

export const createExecutionSlice: StateCreator<StoreState, [], [], ExecutionSlice> = (set, get) => ({
  executorState: "idle",

  setExecutorState: (state) => set({ executorState: state }),

  runWorkflow: async () => {
    const { workflow, projectPath, agentConfig, vlmConfig, vlmEnabled, mcpCommand, pushLog } = get();

    const graphErrors = validateSingleGraph(workflow.nodes, workflow.edges);
    if (graphErrors.length > 0) {
      for (const err of graphErrors) {
        pushLog(`Validation error: ${err}`);
      }
      return;
    }

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
  },

  stopWorkflow: async () => {
    const { pushLog } = get();
    const result = await commands.stopWorkflow();
    if (result.status === "error") {
      pushLog(`Stop failed: ${result.error}`);
    }
  },
});
