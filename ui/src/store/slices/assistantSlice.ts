import type { StateCreator } from "zustand";
import type { Workflow, AssistantChatRequest, WorkflowPatch, ConversationSession, ChatEntry, Edge, PatchSummary } from "../../bindings";
import { commands } from "../../bindings";
import { makeEmptyConversation } from "../state";
import { toEndpoint } from "../settings";
import { edgeOutputToHandle } from "../../utils/edgeHandles";
import type { StoreState } from "./types";

export interface AssistantSlice {
  conversation: ConversationSession;
  assistantOpen: boolean;
  assistantLoading: boolean;
  assistantError: string | null;
  pendingPatch: WorkflowPatch | null;
  pendingPatchWarnings: string[];

  setAssistantOpen: (open: boolean) => void;
  toggleAssistant: () => void;
  sendAssistantMessage: (message: string) => Promise<void>;
  resendMessage: (index: number) => Promise<void>;
  applyPendingPatch: () => Promise<void>;
  discardPendingPatch: () => void;
  clearConversation: () => void;
}

export const createAssistantSlice: StateCreator<StoreState, [], [], AssistantSlice> = (set, get) => ({
  conversation: makeEmptyConversation(),
  assistantOpen: false,
  assistantLoading: false,
  assistantError: null,
  pendingPatch: null,
  pendingPatchWarnings: [],

  setAssistantOpen: (open) => set({ assistantOpen: open }),
  toggleAssistant: () => set((s) => ({ assistantOpen: !s.assistantOpen })),

  sendAssistantMessage: async (message) => {
    const { plannerConfig, allowAiTransforms, allowAgentSteps, mcpCommand, maxRepairAttempts, pushLog } = get();
    set({ assistantLoading: true, assistantError: null });

    // Capture conversation state BEFORE adding the user message -- the backend
    // receives the new message separately as `user_message`.
    const conv = get().conversation;

    const userEntry: ChatEntry = {
      role: "user",
      content: message,
      timestamp: Date.now(),
      patch_summary: null,
      run_context: null,
    };
    set((s) => ({
      conversation: {
        ...s.conversation,
        messages: [...s.conversation.messages, userEntry],
      },
    }));

    try {
      const request: AssistantChatRequest = {
        workflow: get().workflow,
        user_message: message,
        history: conv.messages,
        summary: conv.summary ?? null,
        summary_cutoff: conv.summary_cutoff ?? 0,
        run_context: null,
        planner: toEndpoint(plannerConfig),
        allow_ai_transforms: allowAiTransforms,
        allow_agent_steps: allowAgentSteps,
        mcp_command: mcpCommand,
        max_repair_attempts: maxRepairAttempts,
      };

      const result = await commands.assistantChat(request);
      if (result.status === "ok") {
        const data = result.data;

        // Build patch summary if there's a patch
        let patchSummary: PatchSummary | null = null;
        if (data.patch) {
          const currentNodes = get().workflow.nodes;
          const removedNames = data.patch.removed_node_ids.map((id) => {
            const node = currentNodes.find((n) => n.id === id);
            return node?.name ?? id;
          });
          patchSummary = {
            added: data.patch.added_nodes.length,
            removed: data.patch.removed_node_ids.length,
            updated: data.patch.updated_nodes.length,
            added_names: data.patch.added_nodes.map((n) => n.name),
            removed_names: removedNames,
            updated_names: data.patch.updated_nodes.map((n) => n.name),
            description: null,
          };
        }

        // Add assistant message to conversation
        const assistantEntry: ChatEntry = {
          role: "assistant",
          content: data.assistant_message,
          timestamp: Date.now(),
          patch_summary: patchSummary,
          run_context: null,
        };

        set((s) => ({
          conversation: {
            messages: [...s.conversation.messages, assistantEntry],
            summary: data.new_summary ?? s.conversation.summary,
            summary_cutoff: data.summary_cutoff,
          },
          pendingPatch: data.patch ?? s.pendingPatch,
          pendingPatchWarnings: data.patch ? data.warnings : s.pendingPatchWarnings,
        }));

        pushLog(`Assistant: ${data.patch ? "generated changes" : "responded"}`);
      } else {
        set({ assistantError: result.error });
        pushLog(`Assistant error: ${result.error}`);
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      set({ assistantError: msg });
      pushLog(`Assistant error: ${msg}`);
    } finally {
      set({ assistantLoading: false });
    }
  },

  resendMessage: async (index) => {
    const conv = get().conversation;
    const entry = conv.messages[index];
    if (!entry || entry.role !== "user") return;
    const content = entry.content;
    // Truncate to just before this user message, discarding it and everything after
    set((s) => ({
      conversation: {
        ...s.conversation,
        messages: s.conversation.messages.slice(0, index),
      },
      pendingPatch: null,
      pendingPatchWarnings: [],
      assistantError: null,
    }));
    await get().sendAssistantMessage(content);
  },

  applyPendingPatch: async () => {
    const { pendingPatch, workflow, pushLog } = get();
    if (!pendingPatch) return;
    const edgeKey = (e: Edge) =>
      `${e.from}-${e.to}-${edgeOutputToHandle(e.output) ?? ""}`;
    const removedEdgeKeys = new Set(
      pendingPatch.removed_edges.map(edgeKey),
    );
    const nodes = [
      ...workflow.nodes
        .filter((n) => !pendingPatch.removed_node_ids.includes(n.id))
        .map((n) => pendingPatch.updated_nodes.find((u) => u.id === n.id) ?? n),
      ...pendingPatch.added_nodes,
    ];
    const edges = [
      ...workflow.edges.filter((e) => !removedEdgeKeys.has(edgeKey(e))),
      ...pendingPatch.added_edges,
    ];
    const patched: Workflow = { ...workflow, nodes, edges };
    try {
      const validation = await commands.validate(patched);
      if (!validation.valid) {
        const msg = `Patch rejected: ${validation.errors.join(", ")}`;
        pushLog(msg);
        set({ assistantError: msg });
        return;
      }
    } catch (e) {
      const msg = `Patch validation failed: ${e instanceof Error ? e.message : String(e)}`;
      pushLog(msg);
      set({ assistantError: msg });
      return;
    }
    set({
      workflow: patched,
      pendingPatch: null,
      pendingPatchWarnings: [],
      assistantError: null,
      isNewWorkflow: false,
    });
    pushLog("Applied assistant changes");
  },

  discardPendingPatch: () => {
    set({
      pendingPatch: null,
      pendingPatchWarnings: [],
      assistantError: null,
    });
  },

  clearConversation: () => {
    set({
      conversation: makeEmptyConversation(),
      pendingPatch: null,
      pendingPatchWarnings: [],
      assistantError: null,
    });
  },
});
