import { commands } from "../bindings";
import type { AssistantMessage } from "./slices/assistantSlice";

interface SaveContext {
  projectPath: string | null;
  workflowName: string;
  workflowId: string;
  storeTraces: boolean;
}

/**
 * Load the saved chat transcript for a workflow. Returns an empty
 * array on any error (missing file, malformed JSON) — transcript is
 * best-effort, never blocks project open.
 */
export async function loadAgentChat(
  ctx: Omit<SaveContext, "storeTraces">,
): Promise<AssistantMessage[]> {
  try {
    const res = await commands.loadAgentChat({
      project_path: ctx.projectPath,
      workflow_name: ctx.workflowName,
      workflow_id: ctx.workflowId,
    });
    // tauri-specta wraps Result<T, E> in a tagged shape.
    const chat =
      res && typeof res === "object" && "status" in res
        ? (res as { status: "ok"; data: { messages?: AssistantChatMessageWire[] } }).data
        : (res as { messages?: AssistantChatMessageWire[] });
    return (chat?.messages ?? []).map((m) => ({
      role: m.role,
      content: m.content,
      timestamp: m.timestamp,
      runId: m.run_id ?? undefined,
    }));
  } catch {
    return [];
  }
}

interface AssistantChatMessageWire {
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: string;
  run_id?: string | null;
}

/**
 * Save the chat transcript. Calls are skipped when a Clear
 * conversation wipe is in flight so the backend's just-deleted file
 * isn't recreated by a stale save. The backend command itself is a
 * no-op when `store_traces === false` (D1.M4).
 */
export async function saveAgentChat(
  ctx: SaveContext,
  messages: AssistantMessage[],
): Promise<void> {
  const { isConversationWipeInProgress } = await import(
    "./slices/assistantSlice"
  );
  if (isConversationWipeInProgress()) return;

  try {
    await commands.saveAgentChat({
      project_path: ctx.projectPath,
      workflow_name: ctx.workflowName,
      workflow_id: ctx.workflowId,
      store_traces: ctx.storeTraces,
      chat: {
        messages: messages.map((m) => ({
          role: m.role,
          content: m.content,
          timestamp: m.timestamp,
          run_id: m.runId ?? null,
        })),
      },
    });
  } catch {
    // Transcript saves are best-effort; swallow errors so the
    // conversation stays responsive even when the disk is hostile.
  }
}
