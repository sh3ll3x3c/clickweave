import type { Edge, Node } from "../bindings";

/**
 * Validates that all enabled nodes form at most one connected graph.
 * Returns error messages if there are 2+ separate connected components
 * each containing 2+ nodes.
 */
export function validateSingleGraph(nodes: Node[], edges: Edge[]): string[] {
  const enabledNodes = nodes.filter((n) => n.enabled);
  if (enabledNodes.length <= 1) return [];

  const nodeIds = new Set(enabledNodes.map((n) => n.id));

  // Build adjacency list (undirected)
  const adj = new Map<string, Set<string>>();
  for (const id of nodeIds) {
    adj.set(id, new Set());
  }
  for (const edge of edges) {
    if (nodeIds.has(edge.from) && nodeIds.has(edge.to)) {
      adj.get(edge.from)!.add(edge.to);
      adj.get(edge.to)!.add(edge.from);
    }
  }

  // BFS to find connected components
  const visited = new Set<string>();
  const components: string[][] = [];

  for (const id of nodeIds) {
    if (visited.has(id)) continue;
    const component: string[] = [];
    const queue = [id];
    visited.add(id);
    while (queue.length > 0) {
      const current = queue.shift()!;
      component.push(current);
      for (const neighbor of adj.get(current)!) {
        if (!visited.has(neighbor)) {
          visited.add(neighbor);
          queue.push(neighbor);
        }
      }
    }
    components.push(component);
  }

  // Only flag if there are 2+ components each with 2+ nodes
  const multiNodeComponents = components.filter((c) => c.length >= 2);
  if (multiNodeComponents.length <= 1) return [];

  const nodeNameMap = new Map(nodes.map((n) => [n.id, n.name]));
  return [
    `Graph has ${multiNodeComponents.length} disconnected subgraphs. ` +
      `All nodes must be connected into a single execution graph. ` +
      `Subgraphs: ${multiNodeComponents
        .map(
          (c) =>
            `[${c.map((id) => nodeNameMap.get(id) ?? id).join(", ")}]`,
        )
        .join(", ")}`,
  ];
}
