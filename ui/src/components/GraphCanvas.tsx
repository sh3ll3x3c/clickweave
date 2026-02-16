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
        id: `${edge.from}-${edge.to}-${edgeOutputToHandle(edge.output) ?? "default"}`,
        source: edge.from,
        target: edge.to,
        sourceHandle: edgeOutputToHandle(edge.output),
        label: getEdgeLabel(edge.output),
        labelStyle: { fill: "var(--text-muted)", fontSize: 10 },
        labelBgStyle: { fill: "var(--bg-panel)", opacity: 0.8 },
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
