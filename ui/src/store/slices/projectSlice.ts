import type { StateCreator } from "zustand";
import type { Workflow, ConversationData } from "../../bindings";
import { commands } from "../../bindings";
import { makeDefaultWorkflow, makeEmptyConversation } from "../state";
import type { StoreState } from "./types";
import { localEntryToDto, dtoEntryToLocal } from "./conversationMappers";

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
    const { pushLog } = get();
    const result = await commands.pickWorkflowFile();
    if (result.status !== "ok" || !result.data) return;
    const filePath = result.data;
    const projectResult = await commands.openProject(filePath);
    if (projectResult.status !== "ok") {
      pushLog(`Failed to open: ${projectResult.error}`);
      return;
    }
    set({
      projectPath: projectResult.data.path,
      workflow: projectResult.data.workflow,
      selectedNode: null,
      isNewWorkflow: false,
    });

    // Load conversation
    try {
      const convResult = await commands.loadConversation(filePath);
      if (convResult.status === "ok" && convResult.data) {
        set({
          conversation: {
            messages: convResult.data.messages.map(dtoEntryToLocal),
            summary: convResult.data.summary,
            summaryCutoff: convResult.data.summary_cutoff,
          },
        });
      } else {
        set({ conversation: makeEmptyConversation() });
      }
    } catch {
      set({ conversation: makeEmptyConversation() });
    }

    pushLog(`Opened: ${filePath}`);
  },

  saveProject: async () => {
    const { projectPath, workflow, conversation, pushLog } = get();
    let savePath = projectPath;
    if (!savePath) {
      const result = await commands.pickSaveFile();
      if (result.status !== "ok" || !result.data) return;
      savePath = result.data;
      set({ projectPath: savePath });
    }
    const saveResult = await commands.saveProject(savePath, workflow);
    if (saveResult.status !== "ok") {
      pushLog(`Failed to save: ${saveResult.error}`);
      return;
    }

    // Save conversation alongside the project
    if (savePath) {
      try {
        const convDto: ConversationData = {
          messages: conversation.messages.map(localEntryToDto),
          summary: conversation.summary,
          summary_cutoff: conversation.summaryCutoff,
        };
        await commands.saveConversation(savePath, convDto);
      } catch (e) {
        console.error("Failed to save conversation:", e);
      }
    }

    pushLog(projectPath ? "Saved" : `Saved to: ${savePath}`);
  },

  newProject: () => {
    const { pushLog } = get();
    set({
      workflow: makeDefaultWorkflow(),
      projectPath: null,
      selectedNode: null,
      isNewWorkflow: true,
      conversation: makeEmptyConversation(),
      pendingPatch: null,
      pendingPatchWarnings: [],
      assistantError: null,
    });
    pushLog("New project created");
  },

  skipIntentEntry: () => set({ isNewWorkflow: false }),
});
