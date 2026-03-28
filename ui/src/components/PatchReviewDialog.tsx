import { useEffect } from "react";
import type { WorkflowPatch } from "../bindings";

interface PatchReviewDialogProps {
  patch: WorkflowPatch;
  reason: string;
  screenshot?: string;
  onApprove: () => void;
  onReject: () => void;
}

export function PatchReviewDialog({
  patch,
  reason,
  screenshot,
  onApprove,
  onReject,
}: PatchReviewDialogProps) {
  const addedCount = patch.added_nodes.length;
  const updatedCount = patch.updated_nodes.length;
  const removedCount = patch.removed_node_ids.length;

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onReject();
      if (e.key === "Enter" && !e.shiftKey) onApprove();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onApprove, onReject]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-neutral-900 border border-neutral-700 rounded-lg shadow-xl max-w-lg w-full mx-4">
        {/* Header */}
        <div className="px-4 py-3 border-b border-neutral-700 flex items-center justify-between">
          <h3 className="text-sm font-medium text-neutral-100">Proposed Changes</h3>
          <div className="flex gap-2">
            {addedCount > 0 && (
              <span className="text-xs px-2 py-0.5 rounded-full bg-green-900/50 text-green-400">
                +{addedCount} added
              </span>
            )}
            {updatedCount > 0 && (
              <span className="text-xs px-2 py-0.5 rounded-full bg-yellow-900/50 text-yellow-400">
                ~{updatedCount} modified
              </span>
            )}
            {removedCount > 0 && (
              <span className="text-xs px-2 py-0.5 rounded-full bg-red-900/50 text-red-400">
                -{removedCount} removed
              </span>
            )}
          </div>
        </div>

        {/* Reason */}
        <div className="px-4 py-3 text-sm text-neutral-300">
          {reason}
        </div>

        {/* Screenshot */}
        {screenshot && (
          <div className="px-4 pb-3">
            <img
              src={`data:image/png;base64,${screenshot}`}
              alt="Current screen"
              className="rounded border border-neutral-700 max-h-48 w-full object-contain"
            />
          </div>
        )}

        {/* Changeset */}
        <div className="px-4 pb-3 space-y-2">
          {addedCount > 0 && (
            <div>
              <div className="flex items-center gap-2 text-xs text-green-400 mb-1">
                <span className="w-2 h-2 rounded-full bg-green-500" />
                Added ({addedCount})
              </div>
              <div className="flex flex-wrap gap-1">
                {patch.added_nodes.map((n) => (
                  <span key={n.id} className="text-xs px-2 py-0.5 rounded bg-green-900/30 text-green-300">
                    {n.name}
                  </span>
                ))}
              </div>
            </div>
          )}
          {updatedCount > 0 && (
            <div>
              <div className="flex items-center gap-2 text-xs text-yellow-400 mb-1">
                <span className="w-2 h-2 rounded-full bg-yellow-500" />
                Modified ({updatedCount})
              </div>
              <div className="flex flex-wrap gap-1">
                {patch.updated_nodes.map((n) => (
                  <span key={n.id} className="text-xs px-2 py-0.5 rounded bg-yellow-900/30 text-yellow-300">
                    {n.name}
                  </span>
                ))}
              </div>
            </div>
          )}
          {removedCount > 0 && (
            <div>
              <div className="flex items-center gap-2 text-xs text-red-400 mb-1">
                <span className="w-2 h-2 rounded-full bg-red-500" />
                Removed ({removedCount})
              </div>
              <div className="flex flex-wrap gap-1">
                {patch.removed_node_ids.map((id) => (
                  <span key={id} className="text-xs px-2 py-0.5 rounded bg-red-900/30 text-red-300 line-through">
                    {id.slice(0, 8)}...
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="px-4 py-3 border-t border-neutral-700 flex items-center justify-between">
          <span className="text-xs text-neutral-500">
            {addedCount + updatedCount + removedCount} nodes affected
          </span>
          <div className="flex gap-2">
            <button
              onClick={onReject}
              className="px-3 py-1.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300"
            >
              Reject (Esc)
            </button>
            <button
              onClick={onApprove}
              className="px-3 py-1.5 text-xs rounded bg-blue-600 hover:bg-blue-500 text-white"
            >
              Approve (Enter)
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
