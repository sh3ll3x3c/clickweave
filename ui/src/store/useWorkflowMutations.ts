import { useCallback } from "react";
import type { Edge, Node, NodeType, Workflow } from "../bindings";

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
    (from: string, to: string) => {
      setWorkflow((prev) => {
        const filtered = prev.edges.filter((e) => e.from !== from);
        const edge: Edge = { from, to };
        return { ...prev, edges: [...filtered, edge] };
      });
    },
    [setWorkflow],
  );

  const removeEdge = useCallback(
    (from: string, to: string) => {
      setWorkflow((prev) => ({
        ...prev,
        edges: prev.edges.filter((e) => !(e.from === from && e.to === to)),
      }));
    },
    [setWorkflow],
  );

  return { addNode, removeNode, updateNodePositions, updateNode, addEdge, removeEdge };
}
