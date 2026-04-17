import type { Workflow } from "../bindings";
import type { AssistantMessage } from "../store/slices/assistantSlice";

/**
 * Canonical string used to replace an assistant summary when at least
 * one node from that turn has been deleted but the turn is not fully
 * gone. Per spec decision D1.M3 (resolved 2026-04-17).
 */
export const REDACTED_SUMMARY = "(partially deleted by user)";

export interface PriorTurn {
  goal: string;
  summary: string;
  run_id: string;
}

/**
 * Walk messages in order, pair each `user` message with the next
 * `assistant` message that shares the same `runId`, and return only
 * those pairs whose runId still has at least one surviving node in
 * `workflow`. System annotations are ignored.
 *
 * Redaction is NOT applied here. The selective-delete handler is
 * responsible for calling `mapMessagesByRunIds` with `REDACTED_SUMMARY`
 * before building prior turns — this keeps the helper a pure filter.
 */
export function buildPriorTurns(
  messages: AssistantMessage[],
  workflow: Workflow,
): PriorTurn[] {
  const liveRunIds = new Set<string>();
  for (const node of workflow.nodes ?? []) {
    if (node.source_run_id) liveRunIds.add(node.source_run_id);
  }

  const turns: PriorTurn[] = [];
  for (let i = 0; i < messages.length; i += 1) {
    const m = messages[i];
    if (m.role !== "user" || !m.runId) continue;
    let assistant: AssistantMessage | null = null;
    for (let j = i + 1; j < messages.length; j += 1) {
      const n = messages[j];
      if (n.role === "assistant" && n.runId === m.runId) {
        assistant = n;
        break;
      }
    }
    if (assistant && liveRunIds.has(m.runId)) {
      turns.push({
        goal: m.content,
        summary: assistant.content,
        run_id: m.runId,
      });
    }
  }
  return turns;
}
