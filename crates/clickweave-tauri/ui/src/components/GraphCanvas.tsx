import { useCallback, useMemo } from "react";
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
import type { Workflow, Node, Edge } from "../bindings";
import { WorkflowNode } from "./WorkflowNode";

interface GraphCanvasProps {
  workflow: Workflow;
  selectedNode: string | null;
  activeNode: string | null;
  onSelectNode: (id: string | null) => void;
  onNodesChange: (nodes: Node[]) => void;
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

export function GraphCanvas({
  workflow,
  selectedNode,
  activeNode,
  onSelectNode,
  onNodesChange,
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

  // Convert workflow nodes to React Flow nodes
  const rfNodes: RFNode[] = useMemo(
    () =>
      workflow.nodes.map((node) => ({
        id: node.id,
        type: "workflow",
        position: { x: node.position.x, y: node.position.y },
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
      })),
    [workflow.nodes, selectedNode, activeNode, onDeleteNode],
  );

  // Convert workflow edges to React Flow edges
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

  const handleNodesChange: OnNodesChange = useCallback(
    (changes) => {
      // Apply changes to get updated RF nodes
      const updated = applyNodeChanges(changes, rfNodes);
      // Convert back to workflow nodes, preserving data
      const newNodes = updated
        .map((rfn) => {
          const original = workflow.nodes.find((n) => n.id === rfn.id);
          if (!original) return null;
          return {
            ...original,
            position: {
              x: rfn.position?.x ?? original.position.x,
              y: rfn.position?.y ?? original.position.y,
            },
          };
        })
        .filter(Boolean) as Node[];
      onNodesChange(newNodes);

      // Handle selection
      for (const change of changes) {
        if (change.type === "select") {
          if (change.selected) {
            onSelectNode(change.id);
          }
        }
      }
    },
    [rfNodes, workflow.nodes, onNodesChange, onSelectNode],
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
