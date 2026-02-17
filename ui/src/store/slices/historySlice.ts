import type { StateCreator } from "zustand";
import type { Workflow } from "../../bindings";
import type { StoreState } from "./types";

export const MAX_HISTORY = 50;

export interface HistoryEntry {
  label: string;
  workflow: Workflow;
}

export function createHistoryEntry(label: string, workflow: Workflow): HistoryEntry {
  return { label, workflow: structuredClone(workflow) };
}

export function pushToStack(stack: HistoryEntry[], entry: HistoryEntry): HistoryEntry[] {
  const next = [...stack, entry];
  return next.length > MAX_HISTORY ? next.slice(next.length - MAX_HISTORY) : next;
}

/** Preserve selectedNode only if it exists in the target workflow. */
export function preserveSelection(selectedNode: string | null, workflow: Workflow): string | null {
  if (!selectedNode) return null;
  return workflow.nodes.some((n) => n.id === selectedNode) ? selectedNode : null;
}

export interface HistorySlice {
  past: HistoryEntry[];
  future: HistoryEntry[];

  pushHistory: (label: string) => void;
  undo: () => void;
  redo: () => void;
  clearHistory: () => void;
}

export const createHistorySlice: StateCreator<StoreState, [], [], HistorySlice> = (set, get) => ({
  past: [],
  future: [],

  pushHistory: (label) => {
    const entry = createHistoryEntry(label, get().workflow);
    set((s) => ({
      past: pushToStack(s.past, entry),
      future: [],
    }));
  },

  undo: () => {
    const { past, workflow, selectedNode } = get();
    if (past.length === 0) return;
    const prev = past[past.length - 1];
    const currentEntry = createHistoryEntry("", workflow);
    set((s) => ({
      past: s.past.slice(0, -1),
      future: [...s.future, currentEntry],
      workflow: prev.workflow,
      selectedNode: preserveSelection(selectedNode, prev.workflow),
    }));
  },

  redo: () => {
    const { future, workflow, selectedNode } = get();
    if (future.length === 0) return;
    const next = future[future.length - 1];
    const currentEntry = createHistoryEntry("", workflow);
    set((s) => ({
      future: s.future.slice(0, -1),
      past: pushToStack(s.past, currentEntry),
      workflow: next.workflow,
      selectedNode: preserveSelection(selectedNode, next.workflow),
    }));
  },

  clearHistory: () => set({ past: [], future: [] }),
});
