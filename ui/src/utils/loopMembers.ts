import type { Node, Edge } from "../bindings";

/**
 * Walk the graph from each Loop node's LoopBody edge to its paired EndLoop,
 * collecting the IDs of all body nodes in between.
 *
 * Returns Map<loopNodeId, bodyNodeIds[]>.
 */
export function computeLoopMembers(
  nodes: Node[],
  edges: Edge[],
): Map<string, string[]> {
  const result = new Map<string, string[]>();

  // Build adjacency list (outgoing edges per node)
  const adj = new Map<string, { to: string; output: Edge["output"] }[]>();
  for (const e of edges) {
    let list = adj.get(e.from);
    if (!list) {
      list = [];
      adj.set(e.from, list);
    }
    list.push({ to: e.to, output: e.output });
  }

  // Find EndLoop node ID for a given Loop node ID
  const endLoopFor = new Map<string, string>();
  for (const n of nodes) {
    if (n.node_type.type === "EndLoop") {
      endLoopFor.set(n.node_type.loop_id, n.id);
    }
  }

  // For each Loop node, BFS from LoopBody edge to EndLoop
  for (const n of nodes) {
    if (n.node_type.type !== "Loop") continue;

    const endLoopId = endLoopFor.get(n.id);
    const bodyEdge = adj.get(n.id)?.find((e) => e.output?.type === "LoopBody");
    if (!bodyEdge) {
      result.set(n.id, []);
      continue;
    }

    // BFS -- stop at EndLoop, don't cross into it
    const body: string[] = [];
    const visited = new Set<string>();
    const queue: string[] = [bodyEdge.to];
    visited.add(n.id); // don't revisit the Loop node itself

    while (queue.length > 0) {
      const current = queue.shift()!;
      if (visited.has(current)) continue;
      visited.add(current);

      // EndLoop for this loop is a boundary -- don't include it in body
      if (current === endLoopId) continue;

      body.push(current);

      const outgoing = adj.get(current);
      if (outgoing) {
        for (const e of outgoing) {
          if (!visited.has(e.to)) {
            queue.push(e.to);
          }
        }
      }
    }

    result.set(n.id, body);
  }

  return result;
}
