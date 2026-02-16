import { useCallback } from "react";
import type { Edge, EdgeOutput, Node, NodeType, Workflow } from "../bindings";
import { handleToEdgeOutput, edgeOutputsEqual } from "../utils/edgeHandles";

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

  const removeNodes = useCallback(
    (ids: string[]) => {
      const idSet = new Set(ids);
      setWorkflow((prev) => ({
        ...prev,
        nodes: prev.nodes.filter((n) => !idSet.has(n.id)),
        edges: prev.edges.filter((e) => !idSet.has(e.from) && !idSet.has(e.to)),
      }));
      setSelectedNode((prev) => (prev !== null && idSet.has(prev) ? null : prev));
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
        const output = sourceHandle ? handleToEdgeOutput(sourceHandle) : null;
        // For control flow nodes, replace the edge from the exact same output port.
        const filtered = output
          ? prev.edges.filter((e) => e.from !== from || !edgeOutputsEqual(e.output, output))
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
            return !edgeOutputsEqual(e.output, output ?? null);
          }
          return false;
        }),
      }));
    },
    [setWorkflow],
  );

  const removeNode = useCallback(
    (id: string) => removeNodes([id]),
    [removeNodes],
  );

  return { addNode, removeNode, removeNodes, updateNodePositions, updateNode, addEdge, removeEdge };
}
