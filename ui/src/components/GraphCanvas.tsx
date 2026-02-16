import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ReactFlow,
  Background,
  Controls,
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
  onDeleteNode: (id: string) => void;
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
  onDeleteNode: (id: string) => void,
  existing?: RFNode,
): RFNode {
  const meta = nodeMetadata[node.node_type.type] ?? defaultMetadata;
  return {
    ...(existing ?? {}),
    id: node.id,
    type: "workflow",
    position: existing?.position ?? { x: node.position.x, y: node.position.y },
    selected: node.id === selectedNode,
    data: {
      label: node.name,
      nodeType: node.node_type.type,
      icon: meta.icon,
      color: meta.color,
      isActive: node.id === activeNode,
      enabled: node.enabled,
      onDelete: () => onDeleteNode(node.id),
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
  onDeleteNode,
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

  // Default new loops to collapsed — only add loops we haven't seen before.
  // Track known loops so we don't re-collapse loops the user has expanded.
  const [knownLoops, setKnownLoops] = useState<Set<string>>(new Set());
  useEffect(() => {
    const currentLoopIds = new Set(loopMembers.keys());
    const newLoops: string[] = [];
    for (const loopId of currentLoopIds) {
      if (!knownLoops.has(loopId)) {
        newLoops.push(loopId);
      }
    }
    if (newLoops.length > 0) {
      setCollapsedLoops((prev) => {
        const next = new Set(prev);
        for (const loopId of newLoops) {
          next.add(loopId);
        }
        return next;
      });
    }
    setKnownLoops(currentLoopIds);
  }, [loopMembers]); // eslint-disable-line react-hooks/exhaustive-deps

  // Internal RF node state — React Flow fully controls this (including dimensions).
  // We sync workflow data INTO it, preserving RF-internal props like measured/width/height.
  const [rfNodes, setRfNodes] = useState<RFNode[]>([]);

  useEffect(() => {
    setRfNodes((prev) => {
      const prevMap = new Map(prev.map((n) => [n.id, n]));

      // Parent nodes must appear before their children in the array for React Flow.
      // Build the base nodes first, then sort so loop group parents come first.
      const nodes: RFNode[] = [];

      for (const node of workflow.nodes) {
        const existing = prevMap.get(node.id);
        const base = toRFNode(node, selectedNode, activeNode, onDeleteNode, existing);

        // EndLoop nodes are always hidden
        if (endLoopIds.has(node.id)) {
          nodes.push({ ...base, hidden: true });
          continue;
        }

        // Loop nodes: collapsed vs expanded
        if (node.node_type.type === "Loop") {
          const bodyCount = loopMembers.get(node.id)?.length ?? 0;
          if (collapsedLoops.has(node.id)) {
            // Collapsed: render as regular workflow node with collapse badge
            nodes.push({
              ...base,
              type: "workflow",
              data: {
                ...base.data,
                isCollapsedLoop: true,
                bodyCount,
                onToggleCollapse: () => toggleLoopCollapse(node.id),
              },
            });
          } else {
            // Expanded: render as loopGroup
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
          }
          continue;
        }

        // Body nodes of a loop
        const parentLoops = nodeToLoops.get(node.id);
        if (parentLoops && parentLoops.length > 0) {
          // If ANY parent loop is collapsed, hide this body node
          const anyCollapsed = parentLoops.some((lid) => collapsedLoops.has(lid));
          if (anyCollapsed) {
            nodes.push({ ...base, hidden: true });
          } else {
            // Expanded: set parentId to the innermost loop (last in the list)
            // Position conversion is deferred to Task 5
            const parentId = parentLoops[parentLoops.length - 1];
            nodes.push({
              ...base,
              parentId,
              extent: "parent" as const,
            });
          }
          continue;
        }

        // Regular node — no special handling
        nodes.push(base);
      }

      // React Flow requires parent nodes before children in the array.
      // Sort: nodes without parentId first, then nodes with parentId.
      nodes.sort((a, b) => {
        const aHasParent = a.parentId ? 1 : 0;
        const bHasParent = b.parentId ? 1 : 0;
        return aHasParent - bHasParent;
      });

      return nodes;
    });
  }, [
    workflow.nodes,
    selectedNode,
    activeNode,
    onDeleteNode,
    collapsedLoops,
    loopMembers,
    nodeToLoops,
    endLoopIds,
    toggleLoopCollapse,
  ]);

  // Build set of hidden node IDs for edge filtering
  const hiddenNodeIds = useMemo(() => {
    const ids = new Set<string>(endLoopIds);
    // Body nodes of collapsed loops
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
          // Hide edges connected to hidden nodes (EndLoop or collapsed body nodes)
          if (hiddenNodeIds.has(edge.from) || hiddenNodeIds.has(edge.to)) return false;
          // Hide LoopBody edges from collapsed loops (the handle doesn't exist)
          if (
            edge.output?.type === "LoopBody" &&
            collapsedLoops.has(edge.from)
          ) return false;
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

  // Apply ALL changes to internal state so React Flow can track dimensions.
  // Propagate position changes back to workflow state.
  const handleNodesChange: OnNodesChange = useCallback(
    (changes) => {
      setRfNodes((prev) => applyNodeChanges(changes, prev));

      const posUpdates = new Map<string, { x: number; y: number }>();
      for (const change of changes) {
        if (change.type === "position" && change.position) {
          posUpdates.set(change.id, change.position);
        } else if (change.type === "select" && change.selected) {
          onSelectNode(change.id);
        }
      }
      if (posUpdates.size > 0) {
        onNodePositionsChange(posUpdates);
      }
    },
    [onNodePositionsChange, onSelectNode],
  );

  const handleEdgesChange: OnEdgesChange = useCallback(
    (changes) => {
      const updated = applyEdgeChanges(changes, rfEdges);
      const newEdges: Edge[] = updated.map((rfe) => {
        const handle = rfe.sourceHandle ?? undefined;
        const original = workflow.edges.find(
          (e) =>
            e.from === rfe.source &&
            e.to === rfe.target &&
            edgeOutputToHandle(e.output) === handle,
        );
        return { from: rfe.source, to: rfe.target, output: original?.output ?? null };
      });
      onEdgesChange(newEdges);
    },
    [rfEdges, workflow.edges, onEdgesChange],
  );

  const handleConnect: OnConnect = useCallback(
    (connection: Connection) => {
      if (connection.source && connection.target) {
        onConnect(connection.source, connection.target, connection.sourceHandle ?? undefined);
      }
    },
    [onConnect],
  );

  const handlePaneClick = useCallback(() => {
    onSelectNode(null);
  }, [onSelectNode]);

  return (
    <div className="h-full w-full">
      <ReactFlow
        nodes={rfNodes}
        edges={rfEdges}
        nodeTypes={nodeTypes}
        onNodesChange={handleNodesChange}
        onEdgesChange={handleEdgesChange}
        onConnect={handleConnect}
        onPaneClick={handlePaneClick}
        fitView
        fitViewOptions={{ maxZoom: 1 }}
        snapToGrid
        snapGrid={[20, 20]}
        defaultEdgeOptions={{
          type: "default",
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
