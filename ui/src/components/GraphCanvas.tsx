import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ReactFlow,
  Background,
  Controls,
  SelectionMode,
  type Node as RFNode,
  type Edge as RFEdge,
  type NodeTypes,
  type OnNodesChange,
  type OnEdgesChange,
  type OnConnect,
  type Connection,
  applyNodeChanges,
  applyEdgeChanges,
  MarkerType,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { Workflow, Edge, EdgeOutput } from "../bindings";
import { edgeOutputToHandle } from "../utils/edgeHandles";
import { computeLoopMembers } from "../utils/loopMembers";
import { LoopGroupNode } from "./LoopGroupNode";
import { WorkflowNode } from "./WorkflowNode";

interface GraphCanvasProps {
  workflow: Workflow;
  selectedNode: string | null;
  activeNode: string | null;
  onSelectNode: (id: string | null) => void;
  onNodePositionsChange: (updates: Map<string, { x: number; y: number }>) => void;
  onEdgesChange: (edges: Edge[]) => void;
  onConnect: (from: string, to: string, sourceHandle?: string) => void;
  onDeleteNodes: (ids: string[]) => void;
  onRemoveExtraEdges: (edges: Edge[]) => void;
  onBeforeNodeDrag?: () => void;
}

const nodeMetadata: Record<string, { color: string; icon: string }> = {
  AiStep:         { color: "#4c9ee8", icon: "AI" },
  TakeScreenshot: { color: "#a855f7", icon: "SS" },
  FindText:       { color: "#a855f7", icon: "FT" },
  FindImage:      { color: "#a855f7", icon: "FI" },
  Click:          { color: "#f59e0b", icon: "CK" },
  TypeText:       { color: "#f59e0b", icon: "TT" },
  Scroll:         { color: "#f59e0b", icon: "SC" },
  ListWindows:    { color: "#50c878", icon: "LW" },
  FocusWindow:    { color: "#50c878", icon: "FW" },
  PressKey:       { color: "#f59e0b", icon: "PK" },
  McpToolCall:    { color: "#666",    icon: "MC" },
  AppDebugKitOp:  { color: "#ef4444", icon: "DK" },
  If:             { color: "#10b981", icon: "IF" },
  Switch:         { color: "#10b981", icon: "SW" },
  Loop:           { color: "#10b981", icon: "LP" },
  EndLoop:        { color: "#10b981", icon: "EL" },
};

const defaultMetadata = { color: "#666", icon: "??" };

// Layout constants for loop group positioning
const LOOP_HEADER_HEIGHT = 40;
const LOOP_PADDING = 20;
const APPROX_NODE_WIDTH = 160;
const APPROX_NODE_HEIGHT = 50;
const MIN_GROUP_WIDTH = 300;
const MIN_GROUP_HEIGHT = 150;

function getEdgeLabel(output: EdgeOutput | null): string | undefined {
  if (!output) return undefined;
  switch (output.type) {
    case "IfTrue": return "true";
    case "IfFalse": return "false";
    case "SwitchCase": return output.name;
    case "SwitchDefault": return "default";
    case "LoopBody": return "body";
    case "LoopDone": return "done";
  }
}

function toRFNode(
  node: Workflow["nodes"][number],
  selectedNode: string | null,
  activeNode: string | null,
  onDelete: () => void,
  existing?: RFNode,
): RFNode {
  const meta = nodeMetadata[node.node_type.type] ?? defaultMetadata;
  return {
    ...(existing ?? {}),
    // Reset fields that vary by node role (body child vs regular vs group).
    // Callers override these as needed; without this reset, stale values
    // from a previous role leak through the existing spread.
    parentId: undefined,
    extent: undefined,
    hidden: undefined,
    style: undefined,
    id: node.id,
    type: "workflow",
    position: existing?.position ?? { x: node.position.x, y: node.position.y },
    selected: existing?.selected ?? (node.id === selectedNode),
    data: {
      label: node.name,
      nodeType: node.node_type.type,
      icon: meta.icon,
      color: meta.color,
      isActive: node.id === activeNode,
      enabled: node.enabled,
      onDelete,
      switchCases: node.node_type.type === "Switch"
        ? (node.node_type as { type: "Switch"; cases: { name: string }[] }).cases.map((c) => c.name)
        : [],
    },
  };
}

export function GraphCanvas({
  workflow,
  selectedNode,
  activeNode,
  onSelectNode,
  onNodePositionsChange,
  onEdgesChange,
  onConnect,
  onDeleteNodes,
  onRemoveExtraEdges,
  onBeforeNodeDrag,
}: GraphCanvasProps) {
  const nodeTypes: NodeTypes = useMemo(
    () => ({
      workflow: WorkflowNode,
      loopGroup: LoopGroupNode,
    }),
    [],
  );

  // --- Loop collapse state ---
  const [collapsedLoops, setCollapsedLoops] = useState<Set<string>>(new Set());

  const loopMembers = useMemo(
    () => computeLoopMembers(workflow.nodes, workflow.edges),
    [workflow.nodes, workflow.edges],
  );

  // Invert: for each body node, which loops is it in?
  const nodeToLoops = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const [loopId, bodyIds] of loopMembers) {
      for (const bodyId of bodyIds) {
        const loops = map.get(bodyId) ?? [];
        loops.push(loopId);
        map.set(bodyId, loops);
      }
    }
    return map;
  }, [loopMembers]);

  // Set of EndLoop node IDs — always hidden
  const endLoopIds = useMemo(() => {
    const ids = new Set<string>();
    for (const n of workflow.nodes) {
      if (n.node_type.type === "EndLoop") ids.add(n.id);
    }
    return ids;
  }, [workflow.nodes]);

  // Map from loop ID to its EndLoop node ID (for cascade delete)
  const endLoopForLoop = useMemo(() => {
    const map = new Map<string, string>();
    for (const n of workflow.nodes) {
      if (n.node_type.type === "EndLoop") {
        map.set(n.node_type.loop_id, n.id);
      }
    }
    return map;
  }, [workflow.nodes]);

  const toggleLoopCollapse = useCallback((loopId: string) => {
    setCollapsedLoops((prev) => {
      const next = new Set(prev);
      if (next.has(loopId)) {
        next.delete(loopId);
      } else {
        next.add(loopId);
      }
      return next;
    });
  }, []);

  // Default new loops to collapsed. Use a ref to track known loops
  // (not state — it doesn't drive rendering, only prevents re-collapsing).
  const knownLoopsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    const currentLoopIds = new Set(loopMembers.keys());
    const newLoops: string[] = [];
    for (const loopId of currentLoopIds) {
      if (!knownLoopsRef.current.has(loopId)) {
        newLoops.push(loopId);
      }
    }
    setCollapsedLoops((prev) => {
      const next = new Set(prev);
      // Add newly discovered loops
      for (const loopId of newLoops) {
        next.add(loopId);
      }
      // Clean up loops that no longer exist
      for (const id of next) {
        if (!currentLoopIds.has(id)) {
          next.delete(id);
        }
      }
      return next;
    });
    knownLoopsRef.current = currentLoopIds;
  }, [loopMembers]);

  // Internal RF node state — React Flow fully controls this (including dimensions).
  // We sync workflow data INTO it, preserving RF-internal props like measured/width/height.
  const [rfNodes, setRfNodes] = useState<RFNode[]>([]);
  // When true, the next selectedNode change came from canvas interaction — skip external sync.
  const selectionFromCanvasRef = useRef(false);
  // Tracks an in-progress node deletion so handleEdgesChange can distinguish
  // connected-edge removals (already handled by removeNodes) from independently
  // selected edges that need a separate silent removal.
  const deletedNodeIdsRef = useRef<Set<string> | null>(null);

  useEffect(() => {
    setRfNodes((prev) => {
      const prevMap = new Map(prev.map((n) => [n.id, n]));
      const wfNodeMap = new Map(workflow.nodes.map((n) => [n.id, n]));

      const nodes: RFNode[] = [];
      // Track expanded loop group nodes by ID for sizing after all children are processed
      const groupNodeIndices = new Map<string, number>();
      const expandedLoopChildren = new Map<string, RFNode[]>();

      for (const node of workflow.nodes) {
        const existing = prevMap.get(node.id);

        // EndLoop nodes are always hidden
        if (endLoopIds.has(node.id)) {
          const base = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), existing);
          nodes.push({ ...base, hidden: true });
          continue;
        }

        // Loop nodes: collapsed vs expanded
        if (node.node_type.type === "Loop") {
          const bodyIds = loopMembers.get(node.id) ?? [];
          const bodyCount = bodyIds.length;

          if (collapsedLoops.has(node.id)) {
            // Collapsed: render as regular workflow node with collapse badge
            const endLoopId = endLoopForLoop.get(node.id);
            const base = toRFNode(node, selectedNode, activeNode, () => {
              // Cascade delete: Loop node + all body nodes + EndLoop
              const ids = [...bodyIds];
              if (endLoopId) ids.push(endLoopId);
              ids.push(node.id);
              onDeleteNodes(ids);
            }, existing);
            nodes.push({
              ...base,
              type: "workflow",
              data: {
                ...base.data,
                bodyCount,
                onToggleCollapse: () => toggleLoopCollapse(node.id),
              },
            });
          } else {
            // Expanded: render as loopGroup (dimensions set below after body nodes are processed)
            const base = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), existing);
            expandedLoopChildren.set(node.id, []);
            const idx = nodes.length;
            nodes.push({
              ...base,
              type: "loopGroup",
              data: {
                label: node.name,
                bodyCount,
                isActive: node.id === activeNode,
                enabled: node.enabled,
                onToggleCollapse: () => toggleLoopCollapse(node.id),
              },
            });
            groupNodeIndices.set(node.id, idx);
          }
          continue;
        }

        // Body nodes of a loop
        const parentLoops = nodeToLoops.get(node.id);
        if (parentLoops && parentLoops.length > 0) {
          const base = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), existing);

          // If ANY parent loop is collapsed, hide this body node
          const anyCollapsed = parentLoops.some((lid) => collapsedLoops.has(lid));
          if (anyCollapsed) {
            nodes.push({ ...base, hidden: true });
          } else {
            // Expanded: set parentId to the innermost loop (last in the list)
            const parentId = parentLoops[parentLoops.length - 1];
            const loopWfNode = wfNodeMap.get(parentId);

            // Convert absolute position to parent-relative.
            // If already parented to this loop, keep user-dragged position.
            let relativePosition = base.position;
            if (existing?.parentId === parentId) {
              relativePosition = existing.position;
            } else if (loopWfNode) {
              relativePosition = {
                x: node.position.x - loopWfNode.position.x + LOOP_PADDING,
                y: node.position.y - loopWfNode.position.y + LOOP_HEADER_HEIGHT + LOOP_PADDING,
              };
            }

            const childNode: RFNode = {
              ...base,
              parentId,
              extent: "parent" as const,
              position: relativePosition,
              style: {
                ...base.style,
                transition: "opacity 150ms ease 50ms",
              },
            };
            nodes.push(childNode);
            expandedLoopChildren.get(parentId)?.push(childNode);
          }
          continue;
        }

        // Regular node
        const base = toRFNode(node, selectedNode, activeNode, () => onDeleteNodes([node.id]), existing);
        nodes.push(base);
      }

      // Size each expanded loop group node to contain all its children
      for (const [loopId, children] of expandedLoopChildren) {
        const idx = groupNodeIndices.get(loopId);
        if (idx === undefined) continue;
        const groupNode = nodes[idx];

        let maxX = 0;
        let maxY = 0;
        for (const child of children) {
          const measured = prevMap.get(child.id)?.measured;
          const childW = measured?.width ?? APPROX_NODE_WIDTH;
          const childH = measured?.height ?? APPROX_NODE_HEIGHT;
          maxX = Math.max(maxX, child.position.x + childW);
          maxY = Math.max(maxY, child.position.y + childH);
        }

        groupNode.style = {
          ...groupNode.style,
          width: Math.max(MIN_GROUP_WIDTH, maxX + LOOP_PADDING),
          height: Math.max(MIN_GROUP_HEIGHT, maxY + LOOP_PADDING),
        };
      }

      // React Flow requires parent nodes before children in the array.
      nodes.sort((a, b) => {
        const aHasParent = a.parentId ? 1 : 0;
        const bHasParent = b.parentId ? 1 : 0;
        return aHasParent - bHasParent;
      });

      return nodes;
    });
  }, [
    workflow.nodes,
    activeNode,
    onDeleteNodes,
    collapsedLoops,
    loopMembers,
    nodeToLoops,
    endLoopIds,
    endLoopForLoop,
    toggleLoopCollapse,
  ]);

  // Sync external selectedNode changes (e.g. panel clicks) into RF selection state.
  // Skip when the change originated from the canvas to preserve multi-select.
  useEffect(() => {
    if (selectionFromCanvasRef.current) {
      selectionFromCanvasRef.current = false;
      return;
    }
    setRfNodes((prev) =>
      prev.map((n) => {
        const shouldBeSelected = n.id === selectedNode;
        if (n.selected === shouldBeSelected) return n;
        return { ...n, selected: shouldBeSelected };
      }),
    );
  }, [selectedNode]);

  // Build set of hidden node IDs for edge filtering
  const hiddenNodeIds = useMemo(() => {
    const ids = new Set<string>(endLoopIds);
    for (const [nodeId, parentLoops] of nodeToLoops) {
      if (parentLoops.some((lid) => collapsedLoops.has(lid))) {
        ids.add(nodeId);
      }
    }
    return ids;
  }, [endLoopIds, nodeToLoops, collapsedLoops]);

  const rfEdges: RFEdge[] = useMemo(
    () =>
      workflow.edges
        .filter((edge) => {
          if (hiddenNodeIds.has(edge.from) || hiddenNodeIds.has(edge.to)) return false;
          if (edge.output?.type === "LoopBody" && collapsedLoops.has(edge.from)) return false;
          return true;
        })
        .map((edge) => ({
          id: `${edge.from}-${edge.to}-${edgeOutputToHandle(edge.output) ?? "default"}`,
          source: edge.from,
          target: edge.to,
          sourceHandle: edgeOutputToHandle(edge.output),
          label: getEdgeLabel(edge.output),
          labelStyle: { fill: "var(--text-muted)", fontSize: 10 },
          labelBgStyle: { fill: "var(--bg-panel)", opacity: 0.8 },
        })),
    [workflow.edges, hiddenNodeIds, collapsedLoops],
  );

  // Internal RF edge state — preserves selection state across renders.
  const [rfEdgeState, setRfEdgeState] = useState<RFEdge[]>([]);
  useEffect(() => {
    setRfEdgeState(rfEdges);
  }, [rfEdges]);

  // Apply changes to internal state and propagate position/remove changes back to workflow.
  const handleNodesChange: OnNodesChange = useCallback(
    (changes) => {
      // Collect node removals to propagate to workflow store outside the state updater.
      const removeIds: string[] = [];
      for (const change of changes) {
        if (change.type === "remove") {
          removeIds.push(change.id);
        }
      }
      if (removeIds.length > 0) {
        // Record which nodes are being deleted so handleEdgesChange can tell
        // connected-edge removals (already handled) from extra selected edges.
        deletedNodeIdsRef.current = new Set(removeIds);
        onDeleteNodes(removeIds);
        // Safety: clear the ref on the next microtask in case handleEdgesChange
        // is never called (e.g., nodes with no connected edges).
        queueMicrotask(() => { deletedNodeIdsRef.current = null; });
        return; // Workflow store update will trigger RF node rebuild
      }

      setRfNodes((prev) => {
        const updatedNodes = applyNodeChanges(changes, prev);

        // Compute position updates inside the updater where state is fresh
        const nodeMap = new Map(updatedNodes.map((n) => [n.id, n]));
        const posUpdates = new Map<string, { x: number; y: number }>();
        const affectedGroups = new Set<string>();
        for (const change of changes) {
          if (change.type === "position" && change.position) {
            const rfNode = nodeMap.get(change.id);
            if (rfNode?.parentId) {
              affectedGroups.add(rfNode.parentId);
              // Child node dragged within parent — convert to absolute
              const parentRfNode = nodeMap.get(rfNode.parentId);
              if (parentRfNode) {
                posUpdates.set(change.id, {
                  x: change.position.x + parentRfNode.position.x - LOOP_PADDING,
                  y: change.position.y + parentRfNode.position.y - LOOP_HEADER_HEIGHT - LOOP_PADDING,
                });
              }
            } else {
              posUpdates.set(change.id, change.position);
              // If this is a group node being dragged, propagate to children
              // so their absolute positions in workflow data stay in sync.
              for (const child of updatedNodes) {
                if (child.parentId === change.id) {
                  posUpdates.set(child.id, {
                    x: child.position.x + change.position.x - LOOP_PADDING,
                    y: child.position.y + change.position.y - LOOP_HEADER_HEIGHT - LOOP_PADDING,
                  });
                }
              }
            }
          } else if (change.type === "select" && change.selected) {
            selectionFromCanvasRef.current = true;
            onSelectNode(change.id);
          } else if (change.type === "dimensions") {
            const rfNode = nodeMap.get(change.id);
            if (rfNode?.parentId) {
              affectedGroups.add(rfNode.parentId);
            }
          }
        }
        if (posUpdates.size > 0) {
          onNodePositionsChange(posUpdates);
        }

        // Resize loop groups when child dimensions or positions change
        if (affectedGroups.size > 0) {
          for (const groupId of affectedGroups) {
            const groupIdx = updatedNodes.findIndex((n) => n.id === groupId);
            if (groupIdx === -1) continue;
            const children = updatedNodes.filter((n) => n.parentId === groupId);
            let maxX = 0;
            let maxY = 0;
            for (const child of children) {
              const childW = child.measured?.width ?? APPROX_NODE_WIDTH;
              const childH = child.measured?.height ?? APPROX_NODE_HEIGHT;
              maxX = Math.max(maxX, child.position.x + childW);
              maxY = Math.max(maxY, child.position.y + childH);
            }
            updatedNodes[groupIdx] = {
              ...updatedNodes[groupIdx],
              style: {
                ...updatedNodes[groupIdx].style,
                width: Math.max(MIN_GROUP_WIDTH, maxX + LOOP_PADDING),
                height: Math.max(MIN_GROUP_HEIGHT, maxY + LOOP_PADDING),
              },
            };
          }
        }

        return updatedNodes;
      });
    },
    [onNodePositionsChange, onSelectNode, onDeleteNodes],
  );

  const handleEdgesChange: OnEdgesChange = useCallback(
    (changes) => {
      const removals = changes.filter((c) => c.type === "remove");

      // If a node deletion just happened, React Flow fires edge removals for
      // connected edges (already handled by removeNodes) and any independently
      // selected edges.  Identify the extras and remove them silently — the
      // history snapshot was already captured by removeNodes.
      if (deletedNodeIdsRef.current) {
        const deletedIds = deletedNodeIdsRef.current;
        deletedNodeIdsRef.current = null;

        if (removals.length > 0) {
          const extraEdges: Edge[] = [];
          for (const removal of removals) {
            const rfEdge = rfEdgeState.find((e) => e.id === removal.id);
            if (rfEdge && !deletedIds.has(rfEdge.source) && !deletedIds.has(rfEdge.target)) {
              const handle = rfEdge.sourceHandle ?? undefined;
              // workflow.edges still has pre-deletion state here because
              // removeNodes' setWorkflow hasn't triggered a re-render yet.
              // This is correct — we need the old edges to identify extras.
              const original = workflow.edges.find(
                (e) =>
                  e.from === rfEdge.source &&
                  e.to === rfEdge.target &&
                  edgeOutputToHandle(e.output) === handle,
              );
              if (original) {
                extraEdges.push(original);
              }
            }
          }
          if (extraEdges.length > 0) {
            onRemoveExtraEdges(extraEdges);
          }
        }
        // Apply all changes to local RF edge state.
        setRfEdgeState((prev) => applyEdgeChanges(changes, prev));
        return;
      }

      // Normal path — propagate removals to the workflow store.
      // Selection changes are handled internally by React Flow via rfEdgeState.
      if (removals.length > 0) {
        const updated = applyEdgeChanges(removals, rfEdgeState);
        const visibleEdges: Edge[] = updated.map((rfe) => {
          const handle = rfe.sourceHandle ?? undefined;
          const original = workflow.edges.find(
            (e) =>
              e.from === rfe.source &&
              e.to === rfe.target &&
              edgeOutputToHandle(e.output) === handle,
          );
          return { from: rfe.source, to: rfe.target, output: original?.output ?? null };
        });
        const hiddenEdges = workflow.edges.filter((edge) => {
          if (hiddenNodeIds.has(edge.from) || hiddenNodeIds.has(edge.to)) return true;
          if (edge.output?.type === "LoopBody" && collapsedLoops.has(edge.from)) return true;
          return false;
        });
        onEdgesChange([...visibleEdges, ...hiddenEdges]);
      }
      // Apply all changes (including select) to local RF edge state.
      setRfEdgeState((prev) => applyEdgeChanges(changes, prev));
    },
    [workflow.edges, onEdgesChange, onRemoveExtraEdges, hiddenNodeIds, collapsedLoops, rfEdgeState],
  );

  const handleConnect: OnConnect = useCallback(
    (connection: Connection) => {
      if (connection.source && connection.target) {
        onConnect(connection.source, connection.target, connection.sourceHandle ?? undefined);
      }
    },
    [onConnect],
  );

  const handleNodeDragStart = useCallback(() => {
    onBeforeNodeDrag?.();
  }, [onBeforeNodeDrag]);

  const handlePaneClick = useCallback(() => {
    onSelectNode(null);
  }, [onSelectNode]);

  return (
    <div className="h-full w-full">
      <ReactFlow
        nodes={rfNodes}
        edges={rfEdgeState}
        nodeTypes={nodeTypes}
        onNodesChange={handleNodesChange}
        onEdgesChange={handleEdgesChange}
        onConnect={handleConnect}
        onNodeDragStart={handleNodeDragStart}
        onPaneClick={handlePaneClick}
        deleteKeyCode={["Backspace", "Delete"]}
        selectionOnDrag
        selectionMode={SelectionMode.Partial}
        panOnDrag={[1]}
        panOnScroll
        fitView
        fitViewOptions={{ maxZoom: 1 }}
        snapToGrid
        snapGrid={[20, 20]}
        defaultEdgeOptions={{
          type: "default",
          selectable: true,
          markerEnd: { type: MarkerType.ArrowClosed, color: "#666" },
          style: { stroke: "#555", strokeWidth: 2 },
        }}
        proOptions={{ hideAttribution: true }}
        style={{ background: "var(--bg-dark)" }}
      >
        <Background color="#333" gap={20} />
        <Controls
          showInteractive={false}
          style={{
            background: "var(--bg-panel)",
            borderColor: "var(--border)",
          }}
        />
      </ReactFlow>
    </div>
  );
}
