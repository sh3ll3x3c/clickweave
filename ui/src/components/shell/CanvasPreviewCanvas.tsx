import { useMemo } from "react";
import type { ComponentType } from "react";
import { ReactFlow, Background } from "@xyflow/react";
import type { Edge as WorkflowEdge } from "../../bindings";
import { useAppGrouping } from "../../hooks/useAppGrouping";
import { useEdgeSync } from "../../hooks/useEdgeSync";
import { useNodeSync } from "../../hooks/useNodeSync";
import { useUserGrouping } from "../../hooks/useUserGrouping";
import { useStore } from "../../store/useAppStore";
import { WorkflowNode } from "../WorkflowNode";
import { AppGroupNode } from "../AppGroupNode";
import { UserGroupNode } from "../UserGroupNode";
import { AgentRunGroupNode } from "../AgentRunGroupNode";
import "@xyflow/react/dist/style.css";

/**
 * D12 — dedicated read-only React Flow renderer for the Overview's
 * Canvas Preview. Reuses `WorkflowNode` / `AppGroupNode` /
 * `UserGroupNode` / `AgentRunGroupNode` so the rounded-tile node
 * visual stays identical to the editor, and reuses the editor's
 * grouping projection hooks without mounting React Flow mutation
 * listeners. The custom node components themselves still mount their
 * internal `useEffect` hooks (no side-effect mutations verified at
 * write time); a `pointer-events: none` wrapper around each prevents
 * stray click handlers from firing.
 *
 * Critical: do NOT modify `GraphCanvas.tsx` to add a `readOnly` prop —
 * the editor and preview have diverged enough that a single component
 * gated on a flag would tangle the listener wiring.
 */

const nonInteractive = <P extends object>(C: ComponentType<P>) => {
  const Wrapped = (props: P) => (
    <div style={{ pointerEvents: "none" }}>
      <C {...props} />
    </div>
  );
  Wrapped.displayName = `NonInteractive(${C.displayName ?? C.name ?? "Component"})`;
  return Wrapped;
};

// MUST mirror the keys `GraphCanvas.tsx:154-158` registers. The
// `agent_run_group` key is snake_case (matches the `type` value
// emitted by `useRfNodeBuilder`); `appGroup` and `userGroup` are
// camelCase. Do NOT change either.
const PREVIEW_NODE_TYPES = {
  workflow: nonInteractive(WorkflowNode as unknown as ComponentType<object>),
  appGroup: nonInteractive(AppGroupNode as unknown as ComponentType<object>),
  userGroup: nonInteractive(UserGroupNode as unknown as ComponentType<object>),
  agent_run_group: nonInteractive(
    AgentRunGroupNode as unknown as ComponentType<object>,
  ),
};

const noop = () => {};
const noopSelectNode = (_id: string | null) => {};
const noopSelectionChange = (_hasMulti: boolean) => {};
const noopPositionChange = (
  _updates: Map<string, { x: number; y: number }>,
) => {};
const noopDeleteNodes = (_ids: string[]) => {};
const noopEdgesChange = (_edges: WorkflowEdge[]) => {};
const noopConnect = (_from: string, _to: string, _sourceHandle?: string) => {};
const noopRename = (_groupId: string, _newName: string) => {};

export function CanvasPreviewCanvas() {
  const workflow = useStore((s) => s.workflow);
  const appState = useAppGrouping(workflow);
  const userGroupState = useUserGrouping(workflow);
  const agentRunCollapsed = useStore((s) => s.agentRunCollapsed);
  const runTraces = useStore((s) => s.runTraces);
  const toggleAgentRunCollapsed = useStore((s) => s.toggleAgentRunCollapsed);

  const { rfNodes, deletedNodeIdsRef } = useNodeSync({
    workflow,
    selectedNode: null,
    activeNode: null,
    canvasSelectionResetTick: 0,
    collapsedApps: appState.collapsedApps,
    appGroups: appState.appGroups,
    nodeToAppGroup: appState.nodeToAppGroup,
    appGroupMeta: appState.appGroupMeta,
    toggleAppCollapse: appState.toggleAppCollapse,
    agentRunCollapsed,
    runTraces,
    toggleAgentRunCollapsed,
    collapsedUserGroups: userGroupState.collapsedUserGroups,
    nodeToUserGroup: userGroupState.nodeToUserGroup,
    userGroupMeta: userGroupState.userGroupMeta,
    toggleUserGroupCollapse: userGroupState.toggleUserGroupCollapse,
    renamingGroupId: null,
    onRenameConfirm: noopRename,
    onRenameCancel: noop,
    onSelectNode: noopSelectNode,
    onCanvasSelectionChange: noopSelectionChange,
    onNodePositionsChange: noopPositionChange,
    onDeleteNodes: noopDeleteNodes,
    agentStatus: "idle",
    onRejectDeleteDuringRun: noop,
  });

  const hiddenNodeIds = useMemo(() => {
    const ids = new Set<string>();
    for (const id of userGroupState.hiddenUserGroupNodeIds) ids.add(id);
    return ids;
  }, [userGroupState.hiddenUserGroupNodeIds]);

  const { rfEdges } = useEdgeSync({
    workflow,
    hiddenNodeIds,
    collapsedAppEdgeRewrites: appState.collapsedAppEdgeRewrites,
    collapsedUserGroupEdgeRewrites: userGroupState.userGroupEdgeRewrites,
    deletedNodeIdsRef,
    onEdgesChange: noopEdgesChange,
    onRemoveExtraEdges: noopEdgesChange,
    onConnect: noopConnect,
  });

  return (
    <div className="h-full w-full">
      <ReactFlow
        nodes={rfNodes}
        edges={rfEdges}
        nodeTypes={PREVIEW_NODE_TYPES}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        panOnDrag={false}
        zoomOnScroll={false}
        zoomOnPinch={false}
        zoomOnDoubleClick={false}
        fitView
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={24} size={1} color="rgb(var(--bone) / 0.04)" />
      </ReactFlow>
    </div>
  );
}
