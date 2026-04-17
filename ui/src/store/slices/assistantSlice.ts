import type { StateCreator } from "zustand";
import { isWalkthroughActive } from "./walkthroughSlice";
import type { StoreState } from "./types";
import { commands } from "../../bindings";
import { saveAgentChat } from "../agentChatPersistence";
import { autoDissolveGroups } from "../useWorkflowMutations";

// Flag consumed by `saveAgentChat` in `agentChatPersistence.ts` to
// short-circuit writes after Clear begins but before the file is
// removed. Module-scoped so fire-and-forget `void saveAgentChat(...)`
// callers see the latest value when their promise runs.
let conversationWipeInProgress = false;
export function isConversationWipeInProgress(): boolean {
  return conversationWipeInProgress;
}

export interface AssistantMessage {
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: string;
  /**
   * Run-generation ID this message belongs to. Present for user and
   * assistant messages that bracket a single agent turn; `undefined`
   * for system annotations (e.g., deletion notes).
   */
  runId?: string;
}

export interface AssistantSlice {
  messages: AssistantMessage[];
  assistantOpen: boolean;
  assistantError: string | null;

  setAssistantOpen: (open: boolean) => void;
  toggleAssistant: () => void;
  setAssistantError: (error: string | null) => void;
  pushAssistantMessage: (
    role: "user" | "assistant",
    content: string,
    runId?: string,
  ) => void;
  /** Append a centered, muted system annotation (deletion notes). */
  pushSystemAnnotation: (content: string) => void;
  /** Wipe all messages in memory. Used by Clear conversation. */
  clearConversation: () => void;
  /** Replace the full messages array (used by agent_chat.json hydrate). */
  setMessages: (messages: AssistantMessage[]) => void;
  /**
   * Update any user/assistant message whose `runId` is in `runIds`.
   * The callback receives the existing message and returns a new one
   * (used for redacting partial-turn summaries). System messages are
   * never touched.
   */
  mapMessagesByRunIds: (
    runIds: Set<string>,
    fn: (msg: AssistantMessage) => AssistantMessage,
  ) => void;
  /** Drop user/assistant messages whose runId is in `runIds`. System annotations survive. */
  dropTurnsByRunIds: (runIds: Set<string>) => void;
  /**
   * Full Clear-conversation flow (D1.C1): delete every agent-built
   * node, wipe the cache + variant-index + transcript files via the
   * Tauri command, and empty the local messages array. Not undoable.
   */
  clearConversationFlow: () => Promise<void>;
}

export const createAssistantSlice: StateCreator<
  StoreState,
  [],
  [],
  AssistantSlice
> = (set, get) => {
  // Helper: persist the current transcript to `agent_chat.json` via
  // the Tauri command. Fire-and-forget; the command short-circuits
  // on `storeTraces === false` (D1.M4). Only invoked by mutations
  // that changed the messages array.
  const persist = () => {
    const s = get();
    void saveAgentChat(
      {
        projectPath: s.projectPath,
        workflowName: s.workflow.name,
        workflowId: s.workflow.id,
        storeTraces: s.storeTraces,
      },
      s.messages,
    );
  };

  return {
  messages: [],
  assistantOpen: false,
  assistantError: null,

  setAssistantOpen: (open) => {
    if (open && isWalkthroughActive(get().walkthroughStatus)) {
      const status = get().walkthroughStatus;
      if (status === "Recording" || status === "Paused") {
        get().cancelWalkthrough();
      }
      // Review/Processing: don't discard — just hide the walkthrough panel
      // while the assistant is open. Closing the assistant restores it.
    }
    set({ assistantOpen: open });
  },
  toggleAssistant: () => {
    const opening = !get().assistantOpen;
    if (opening && isWalkthroughActive(get().walkthroughStatus)) {
      const status = get().walkthroughStatus;
      if (status === "Recording" || status === "Paused") {
        get().cancelWalkthrough();
      }
      // Review/Processing: don't discard — just hide the walkthrough panel
      // while the assistant is open. Closing the assistant restores it.
    }
    set({ assistantOpen: opening });
  },

  setAssistantError: (error) => set({ assistantError: error }),

  pushAssistantMessage: (role, content, runId) => {
    const trimmed = content.trim();
    if (!trimmed) return;
    set((s) => ({
      messages: [
        ...s.messages,
        {
          role,
          content: trimmed,
          timestamp: new Date().toISOString(),
          runId,
        },
      ],
    }));
    persist();
  },

  pushSystemAnnotation: (content) => {
    const trimmed = content.trim();
    if (!trimmed) return;
    set((s) => ({
      messages: [
        ...s.messages,
        {
          role: "system",
          content: trimmed,
          timestamp: new Date().toISOString(),
        },
      ],
    }));
    persist();
  },

  clearConversation: () => {
    set({ messages: [] });
    persist();
  },

  setMessages: (messages) => set({ messages }),

  mapMessagesByRunIds: (runIds, fn) => {
    set((s) => ({
      messages: s.messages.map((m) =>
        m.role !== "system" && m.runId && runIds.has(m.runId) ? fn(m) : m,
      ),
    }));
    persist();
  },

  dropTurnsByRunIds: (runIds) => {
    set((s) => ({
      messages: s.messages.filter(
        (m) => m.role === "system" || !m.runId || !runIds.has(m.runId),
      ),
    }));
    persist();
  },

  clearConversationFlow: async () => {
    const state = get();
    const agentNodeIds = state.workflow.nodes
      .filter((n) => n.source_run_id != null)
      .map((n) => n.id);

    // (1) Remove agent nodes from the workflow WITHOUT a history push
    //     (D1.C1 — Clear is not undoable; writing a history entry here
    //     would resurrect deleted nodes via Cmd+Z while the cache/
    //     variant/transcript files stay wiped). Also strip deleted
    //     ids from any user groups and auto-dissolve groups that
    //     drop below their minimum membership — otherwise user-group
    //     metadata keeps referencing nodes that no longer exist and
    //     the canvas renders ghost/empty group containers.
    if (agentNodeIds.length > 0) {
      const idSet = new Set(agentNodeIds);
      const updatedGroups = (state.workflow.groups ?? []).map((g) => ({
        ...g,
        node_ids: g.node_ids.filter((id) => !idSet.has(id)),
      }));
      state.setWorkflow({
        ...state.workflow,
        nodes: state.workflow.nodes.filter((n) => !idSet.has(n.id)),
        edges: state.workflow.edges.filter(
          (e) => !idSet.has(e.from) && !idSet.has(e.to),
        ),
        groups: autoDissolveGroups(updatedGroups),
      });
      // Also clear history stacks so Cmd+Z cannot partial-undo the
      // graph mutation we just performed.
      state.clearHistory();
    }

    // (2) Wipe messages in memory before the file wipe so any
    //     concurrent saveAgentChat has nothing to replay. The flag
    //     short-circuits any in-flight save that races this call.
    conversationWipeInProgress = true;
    set({ messages: [] });

    // (3) Wipe files on disk. Respects `store_traces` privacy flag
    //     inside the command body (D1.M4).
    try {
      await commands.clearAgentConversation({
        project_path: state.projectPath,
        workflow_name: state.workflow.name,
        workflow_id: state.workflow.id,
        store_traces: state.storeTraces,
      });
    } catch (e) {
      state.setAssistantError(`Failed to clear conversation: ${String(e)}`);
    } finally {
      conversationWipeInProgress = false;
    }
  },
  };
};
