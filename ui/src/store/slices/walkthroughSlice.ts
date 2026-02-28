import type { StateCreator } from "zustand";
import { commands } from "../../bindings";
import type { ActionRename, Node, NodeType, TargetOverride, VariablePromotion, WalkthroughAction, WalkthroughAnnotations, Workflow } from "../../bindings";
import type { StoreState } from "./types";

export type WalkthroughStatus = "Idle" | "Recording" | "Paused" | "Processing" | "Review" | "Applied" | "Cancelled";

/** Opaque captured event from the backend (serialized WalkthroughEvent). */
export type WalkthroughCapturedEvent = Record<string, unknown>;

/** Upsert an entry into an annotation array, matching by action_id. */
function upsertAnnotation<T extends { action_id: string }>(arr: T[], entry: T): T[] {
  const idx = arr.findIndex((item) => item.action_id === entry.action_id);
  return idx >= 0 ? arr.map((item, i) => (i === idx ? entry : item)) : [...arr, entry];
}

const emptyAnnotations: WalkthroughAnnotations = {
  deleted_action_ids: [],
  renamed_actions: [],
  target_overrides: [],
  variable_promotions: [],
};

export interface WalkthroughSlice {
  walkthroughStatus: WalkthroughStatus;
  walkthroughError: string | null;
  walkthroughEvents: WalkthroughCapturedEvent[];
  walkthroughActions: WalkthroughAction[];
  walkthroughDraft: Workflow | null;
  walkthroughWarnings: string[];
  walkthroughAnnotations: WalkthroughAnnotations;
  walkthroughExpandedAction: string | null;

  setWalkthroughStatus: (status: WalkthroughStatus) => void;
  pushWalkthroughEvent: (event: WalkthroughCapturedEvent) => void;
  setWalkthroughDraft: (payload: { actions: WalkthroughAction[]; draft: Workflow | null; warnings: string[] }) => void;
  fetchWalkthroughDraft: () => Promise<void>;
  startWalkthrough: () => Promise<void>;
  pauseWalkthrough: () => Promise<void>;
  resumeWalkthrough: () => Promise<void>;
  stopWalkthrough: () => Promise<void>;
  cancelWalkthrough: () => Promise<void>;

  setWalkthroughExpandedAction: (id: string | null) => void;
  deleteAction: (actionId: string) => void;
  restoreAction: (actionId: string) => void;
  renameAction: (actionId: string, newName: string) => void;
  overrideTarget: (actionId: string, candidateIndex: number) => void;
  promoteToVariable: (actionId: string, variableName: string) => void;
  removeVariablePromotion: (actionId: string) => void;
  resetAnnotations: () => void;
  applyDraftToCanvas: () => void;
  discardDraft: () => void;
}

export const createWalkthroughSlice: StateCreator<StoreState, [], [], WalkthroughSlice> = (set, get) => ({
  walkthroughStatus: "Idle",
  walkthroughError: null,
  walkthroughEvents: [],
  walkthroughActions: [],
  walkthroughDraft: null,
  walkthroughWarnings: [],
  walkthroughAnnotations: { ...emptyAnnotations },
  walkthroughExpandedAction: null,

  setWalkthroughStatus: (status) => set({ walkthroughStatus: status }),

  pushWalkthroughEvent: (event) => set((s) => ({
    walkthroughEvents: [...s.walkthroughEvents, event],
  })),

  setWalkthroughDraft: ({ actions, draft, warnings }) => set({
    walkthroughActions: actions,
    walkthroughDraft: draft,
    walkthroughWarnings: warnings,
    walkthroughStatus: "Review",
  }),

  fetchWalkthroughDraft: async () => {
    const result = await commands.getWalkthroughDraft();
    if (result.status === "ok") {
      set({
        walkthroughActions: result.data.actions,
        walkthroughDraft: result.data.draft ?? null,
        walkthroughWarnings: result.data.warnings,
        walkthroughStatus: "Review",
      });
    }
  },

  startWalkthrough: async () => {
    const { workflow, mcpCommand, projectPath, pushLog } = get();
    set({
      walkthroughError: null,
      walkthroughEvents: [],

      walkthroughAnnotations: { ...emptyAnnotations },
      walkthroughExpandedAction: null,
      assistantOpen: false,
    });
    const result = await commands.startWalkthrough(workflow.id, mcpCommand, projectPath ?? null);
    if (result.status === "error") {
      set({ walkthroughError: result.error });
      pushLog(`Walkthrough start failed: ${result.error}`);
    }
  },

  pauseWalkthrough: async () => {
    const { pushLog } = get();
    const result = await commands.pauseWalkthrough();
    if (result.status === "error") {
      pushLog(`Walkthrough pause failed: ${result.error}`);
    }
  },

  resumeWalkthrough: async () => {
    const { pushLog } = get();
    const result = await commands.resumeWalkthrough();
    if (result.status === "error") {
      pushLog(`Walkthrough resume failed: ${result.error}`);
    }
  },

  stopWalkthrough: async () => {
    const { pushLog } = get();
    const result = await commands.stopWalkthrough();
    if (result.status === "error") {
      pushLog(`Walkthrough stop failed: ${result.error}`);
    }
  },

  cancelWalkthrough: async () => {
    const { pushLog } = get();
    set({
      walkthroughEvents: [],
      walkthroughActions: [],
      walkthroughDraft: null,
      walkthroughWarnings: [],

      walkthroughAnnotations: { ...emptyAnnotations },
      walkthroughExpandedAction: null,
    });
    const result = await commands.cancelWalkthrough();
    if (result.status === "error") {
      pushLog(`Walkthrough cancel failed: ${result.error}`);
    }
  },

  setWalkthroughExpandedAction: (id) => set((s) => ({
    walkthroughExpandedAction: s.walkthroughExpandedAction === id ? null : id,
  })),

  deleteAction: (actionId) => set((s) => ({
    walkthroughAnnotations: {
      ...s.walkthroughAnnotations,
      deleted_action_ids: [...s.walkthroughAnnotations.deleted_action_ids, actionId],
    },
  })),

  restoreAction: (actionId) => set((s) => ({
    walkthroughAnnotations: {
      ...s.walkthroughAnnotations,
      deleted_action_ids: s.walkthroughAnnotations.deleted_action_ids.filter((id) => id !== actionId),
    },
  })),

  renameAction: (actionId, newName) => set((s) => ({
    walkthroughAnnotations: {
      ...s.walkthroughAnnotations,
      renamed_actions: upsertAnnotation(s.walkthroughAnnotations.renamed_actions, { action_id: actionId, new_name: newName }),
    },
  })),

  overrideTarget: (actionId, candidateIndex) => set((s) => ({
    walkthroughAnnotations: {
      ...s.walkthroughAnnotations,
      target_overrides: upsertAnnotation(s.walkthroughAnnotations.target_overrides, { action_id: actionId, chosen_candidate_index: candidateIndex }),
    },
  })),

  promoteToVariable: (actionId, variableName) => set((s) => ({
    walkthroughAnnotations: {
      ...s.walkthroughAnnotations,
      variable_promotions: upsertAnnotation(s.walkthroughAnnotations.variable_promotions, { action_id: actionId, variable_name: variableName }),
    },
  })),

  removeVariablePromotion: (actionId) => set((s) => ({
    walkthroughAnnotations: {
      ...s.walkthroughAnnotations,
      variable_promotions: s.walkthroughAnnotations.variable_promotions.filter((p) => p.action_id !== actionId),
    },
  })),

  resetAnnotations: () => set({
    walkthroughAnnotations: { ...emptyAnnotations },
    walkthroughExpandedAction: null,
  }),

  applyDraftToCanvas: () => {
    const { walkthroughDraft, walkthroughActions, walkthroughAnnotations: ann } = get();
    if (!walkthroughDraft) return;

    // Build bidirectional action↔node mapping (1:1 order from synthesis)
    const actionToNode = new Map<string, string>();
    const nodeToAction = new Map<string, string>();
    walkthroughActions.forEach((action, i) => {
      if (i < walkthroughDraft.nodes.length) {
        actionToNode.set(action.id, walkthroughDraft.nodes[i].id);
        nodeToAction.set(walkthroughDraft.nodes[i].id, action.id);
      }
    });

    // Collect deleted node IDs
    const deletedNodeIds = new Set(
      ann.deleted_action_ids
        .map((aid) => actionToNode.get(aid))
        .filter((nid): nid is string => !!nid),
    );

    // Filter nodes
    const nodes = walkthroughDraft.nodes
      .filter((n) => !deletedNodeIds.has(n.id))
      .map((n): Node => {
        let updated = { ...n };

        const actionId = nodeToAction.get(n.id);
        if (!actionId) return updated;

        // Apply rename
        const rename = ann.renamed_actions.find((r) => r.action_id === actionId);
        if (rename) {
          updated = { ...updated, name: rename.new_name };
        }

        // Apply target override (Click nodes)
        const targetOvr = ann.target_overrides.find((o) => o.action_id === actionId);
        if (targetOvr && updated.node_type.type === "Click") {
          const action = walkthroughActions.find((a) => a.id === actionId);
          const candidate = action?.target_candidates[targetOvr.chosen_candidate_index];
          if (candidate) {
            let nodeType: NodeType;
            if (candidate.type === "AccessibilityLabel") {
              nodeType = { ...updated.node_type, target: candidate.label, x: null, y: null };
            } else if (candidate.type === "OcrText") {
              nodeType = { ...updated.node_type, target: candidate.text, x: null, y: null };
            } else if (candidate.type === "Coordinates") {
              nodeType = { ...updated.node_type, target: null, x: candidate.x, y: candidate.y };
            } else {
              nodeType = updated.node_type;
            }
            updated = { ...updated, node_type: nodeType };
          }
        }

        // Apply variable promotion (TypeText nodes)
        const varPromo = ann.variable_promotions.find((p) => p.action_id === actionId);
        if (varPromo && varPromo.variable_name && updated.node_type.type === "TypeText") {
          updated = { ...updated, node_type: { ...updated.node_type, text: `{{${varPromo.variable_name}}}` } };
        }

        return updated;
      });

    // Filter edges — remove any referencing deleted nodes
    const edges = walkthroughDraft.edges.filter(
      (e) => !deletedNodeIds.has(e.from) && !deletedNodeIds.has(e.to),
    );

    const modifiedDraft: Workflow = { ...walkthroughDraft, nodes, edges };

    get().pushHistory("Apply Walkthrough");
    get().setWorkflow(modifiedDraft);
    set({
      walkthroughStatus: "Idle",

      walkthroughActions: [],
      walkthroughDraft: null,
      walkthroughWarnings: [],
      walkthroughAnnotations: { ...emptyAnnotations },
      walkthroughExpandedAction: null,
      walkthroughEvents: [],
      isNewWorkflow: false,
    });
  },

  discardDraft: () => set({
    walkthroughStatus: "Idle",
    walkthroughActions: [],
    walkthroughDraft: null,
    walkthroughWarnings: [],
    walkthroughAnnotations: { ...emptyAnnotations },
    walkthroughExpandedAction: null,
    walkthroughEvents: [],
    walkthroughError: null,
  }),
});
