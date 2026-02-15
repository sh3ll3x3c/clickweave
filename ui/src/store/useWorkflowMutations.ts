import { useCallback } from "react";
import type { Edge, EdgeOutput, Node, NodeType, Workflow } from "../bindings";

function sourceHandleToEdgeOutput(handle: string): EdgeOutput | null {
  switch (handle) {
    case "IfTrue": return { type: "IfTrue" };
    case "IfFalse": return { type: "IfFalse" };
    case "SwitchDefault": return { type: "SwitchDefault" };
    case "LoopBody": return { type: "LoopBody" };
    case "LoopDone": return { type: "LoopDone" };
    default:
      if (handle.startsWith("SwitchCase:")) {
        return { type: "SwitchCase", name: handle.slice(11) };
      }
      return null;
  }
}

export function useWorkflowMutations(
  setWorkflow: React.Dispatch<React.SetStateAction<Workflow>>,
  setSelectedNode: React.Dispatch<React.SetStateAction<string | null>>,
  nodesLength: number,
) {
  const addNode = useCallback(
    (nodeType: NodeType) => {
      const id = crypto.randomUUID();
      const offsetX = (nodesLength % 4) * 250;
      const offsetY = Math.floor(nodesLength / 4) * 150;
      const node: Node = {
        id,
        node_type: nodeType,
        position: { x: 200 + offsetX, y: 150 + offsetY },
        name: nodeType.type === "AiStep" ? "AI Step" : nodeType.type.replace(/([A-Z])/g, " $1").trim(),
        enabled: true,
        timeout_ms: null,
        settle_ms: null,
        retries: 0,
        trace_level: "Minimal",
        expected_outcome: null,
        checks: [],
      };
      setWorkflow((prev) => ({ ...prev, nodes: [...prev.nodes, node] }));
      setSelectedNode(id);
    },
    [nodesLength, setWorkflow, setSelectedNode],
  );

  const removeNode = useCallback(
    (id: string) => {
      setWorkflow((prev) => ({
        ...prev,
        nodes: prev.nodes.filter((n) => n.id !== id),
        edges: prev.edges.filter((e) => e.from !== id && e.to !== id),
      }));
      setSelectedNode((prev) => (prev === id ? null : prev));
    },
    [setWorkflow, setSelectedNode],
  );

  const updateNodePositions = useCallback(
    (updates: Map<string, { x: number; y: number }>) => {
      setWorkflow((prev) => ({
        ...prev,
        nodes: prev.nodes.map((n) => {
          const pos = updates.get(n.id);
          return pos ? { ...n, position: { x: pos.x, y: pos.y } } : n;
        }),
      }));
    },
    [setWorkflow],
  );

  const updateNode = useCallback(
    (id: string, updates: Partial<Node>) => {
      setWorkflow((prev) => ({
        ...prev,
        nodes: prev.nodes.map((n) => (n.id === id ? { ...n, ...updates } : n)),
      }));
    },
    [setWorkflow],
  );

  const addEdge = useCallback(
    (from: string, to: string, sourceHandle?: string) => {
      setWorkflow((prev) => {
        const output = sourceHandle ? sourceHandleToEdgeOutput(sourceHandle) : null;
        // For control flow nodes, replace the edge from the exact same output port.
        const filtered = output
          ? prev.edges.filter((e) => e.from !== from || JSON.stringify(e.output) !== JSON.stringify(output))
          : prev.edges.filter((e) => e.from !== from || e.output !== null);
        const edge: Edge = { from, to, output };
        return { ...prev, edges: [...filtered, edge] };
      });
    },
    [setWorkflow],
  );

  const removeEdge = useCallback(
    (from: string, to: string, output?: EdgeOutput | null) => {
      setWorkflow((prev) => ({
        ...prev,
        edges: prev.edges.filter((e) => {
          if (e.from !== from || e.to !== to) return true;
          if (output !== undefined) {
            return JSON.stringify(e.output) !== JSON.stringify(output ?? null);
          }
          return false;
        }),
      }));
    },
    [setWorkflow],
  );

  return { addNode, removeNode, updateNodePositions, updateNode, addEdge, removeEdge };
}
