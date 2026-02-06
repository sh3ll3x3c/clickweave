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
import type { Workflow, Edge } from "../bindings";
import { WorkflowNode } from "./WorkflowNode";

interface GraphCanvasProps {
  workflow: Workflow;
  selectedNode: string | null;
  activeNode: string | null;
  onSelectNode: (id: string | null) => void;
  onNodePositionsChange: (updates: Map<string, { x: number; y: number }>) => void;
  onEdgesChange: (edges: Edge[]) => void;
  onConnect: (from: string, to: string) => void;
  onDeleteNode: (id: string) => void;
}

const categoryColors: Record<string, string> = {
  AiStep: "#4c9ee8",
  TakeScreenshot: "#a855f7",
  FindText: "#a855f7",
  FindImage: "#a855f7",
  Click: "#f59e0b",
  TypeText: "#f59e0b",
  Scroll: "#f59e0b",
  ListWindows: "#50c878",
  FocusWindow: "#50c878",
  AppDebugKitOp: "#ef4444",
};

function toRFNode(
  node: Workflow["nodes"][number],
  selectedNode: string | null,
  activeNode: string | null,
  onDeleteNode: (id: string) => void,
  existing?: RFNode,
): RFNode {
  return {
    ...(existing ?? {}),
    id: node.id,
    type: "workflow",
    position: existing?.position ?? { x: node.position.x, y: node.position.y },
    selected: node.id === selectedNode,
    data: {
      label: node.name,
      nodeType: node.node_type.type,
      icon: getNodeIcon(node.node_type.type),
      color: categoryColors[node.node_type.type] || "#666",
      isActive: node.id === activeNode,
      enabled: node.enabled,
      onDelete: () => onDeleteNode(node.id),
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
    }),
    [],
  );

  // Internal RF node state — React Flow fully controls this (including dimensions).
  // We sync workflow data INTO it, preserving RF-internal props like measured/width/height.
  const [rfNodes, setRfNodes] = useState<RFNode[]>([]);

  useEffect(() => {
    setRfNodes((prev) => {
      const prevMap = new Map(prev.map((n) => [n.id, n]));
      return workflow.nodes.map((node) =>
        toRFNode(node, selectedNode, activeNode, onDeleteNode, prevMap.get(node.id)),
      );
    });
  }, [workflow.nodes, selectedNode, activeNode, onDeleteNode]);

  const rfEdges: RFEdge[] = useMemo(
    () =>
      workflow.edges.map((edge) => ({
        id: `${edge.from}-${edge.to}`,
        source: edge.from,
        target: edge.to,
        type: "default",
        markerEnd: { type: MarkerType.ArrowClosed, color: "#666" },
        style: { stroke: "#555", strokeWidth: 2 },
      })),
    [workflow.edges],
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
      const newEdges: Edge[] = updated.map((rfe) => ({
        from: rfe.source,
        to: rfe.target,
      }));
      onEdgesChange(newEdges);
    },
    [rfEdges, onEdgesChange],
  );

  const handleConnect: OnConnect = useCallback(
    (connection: Connection) => {
      if (connection.source && connection.target) {
        onConnect(connection.source, connection.target);
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

function getNodeIcon(type: string): string {
  const icons: Record<string, string> = {
    AiStep: "AI",
    TakeScreenshot: "SS",
    FindText: "FT",
    FindImage: "FI",
    Click: "CK",
    TypeText: "TT",
    Scroll: "SC",
    ListWindows: "LW",
    FocusWindow: "FW",
    AppDebugKitOp: "DK",
  };
  return icons[type] || "??";
}
