import type { StateCreator } from "zustand";
import { isWalkthroughActive } from "./walkthroughSlice";
import type { StoreState } from "./types";

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
}

export const createAssistantSlice: StateCreator<
  StoreState,
  [],
  [],
  AssistantSlice
> = (set, get) => ({
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
  },

  clearConversation: () => set({ messages: [] }),

  setMessages: (messages) => set({ messages }),

  mapMessagesByRunIds: (runIds, fn) =>
    set((s) => ({
      messages: s.messages.map((m) =>
        m.role !== "system" && m.runId && runIds.has(m.runId) ? fn(m) : m,
      ),
    })),

  dropTurnsByRunIds: (runIds) =>
    set((s) => ({
      messages: s.messages.filter(
        (m) => m.role === "system" || !m.runId || !runIds.has(m.runId),
      ),
    })),
});
