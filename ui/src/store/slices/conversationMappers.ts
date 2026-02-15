import type { ChatEntryDto } from "../../bindings";
import type { ChatEntryLocal } from "../state";

export function localEntryToDto(e: ChatEntryLocal): ChatEntryDto {
  return {
    role: e.role,
    content: e.content,
    timestamp: e.timestamp,
    patch_summary: e.patchSummary
      ? {
          added: e.patchSummary.added,
          removed: e.patchSummary.removed,
          updated: e.patchSummary.updated,
          added_names: e.patchSummary.addedNames,
          removed_names: e.patchSummary.removedNames,
          updated_names: e.patchSummary.updatedNames,
          description: e.patchSummary.description ?? null,
        }
      : null,
    run_context: e.runContext
      ? {
          execution_dir: e.runContext.executionDir,
          node_results: e.runContext.nodeResults.map((nr) => ({
            node_name: nr.nodeName,
            status: nr.status,
            error: nr.error ?? null,
          })),
        }
      : null,
  };
}

export function dtoEntryToLocal(m: ChatEntryDto): ChatEntryLocal {
  return {
    role: m.role as "user" | "assistant",
    content: m.content,
    timestamp: m.timestamp,
    patchSummary: m.patch_summary
      ? {
          added: m.patch_summary.added,
          removed: m.patch_summary.removed,
          updated: m.patch_summary.updated,
          addedNames: m.patch_summary.added_names ?? [],
          removedNames: m.patch_summary.removed_names ?? [],
          updatedNames: m.patch_summary.updated_names ?? [],
          description: m.patch_summary.description ?? undefined,
        }
      : undefined,
    runContext: m.run_context
      ? {
          executionDir: m.run_context.execution_dir,
          nodeResults: m.run_context.node_results.map((nr) => ({
            nodeName: nr.node_name,
            status: nr.status,
            error: nr.error ?? undefined,
          })),
        }
      : undefined,
  };
}
