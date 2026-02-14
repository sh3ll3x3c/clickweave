import type { ChatEntryLocal } from "../store/state";
import type { WorkflowPatch } from "../bindings";

interface ChatMessageProps {
  entry: ChatEntryLocal;
  isLastAssistant: boolean;
  pendingPatch: WorkflowPatch | null;
  pendingPatchWarnings: string[];
  onApplyPatch: () => void;
  onDiscardPatch: () => void;
}

export function ChatMessage({
  entry,
  isLastAssistant,
  pendingPatch,
  pendingPatchWarnings,
  onApplyPatch,
  onDiscardPatch,
}: ChatMessageProps) {
  const isUser = entry.role === "user";
  const showPatchActions = isLastAssistant && pendingPatch !== null;

  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"}`}>
      <div
        className={`max-w-[85%] rounded-lg px-3 py-2 text-sm ${
          isUser
            ? "bg-[var(--accent-coral)]/15 text-[var(--text-primary)]"
            : "bg-[var(--bg-hover)] text-[var(--text-primary)]"
        }`}
      >
        {/* Message content */}
        <div className="whitespace-pre-wrap break-words leading-relaxed">
          {entry.content}
        </div>

        {/* Patch summary */}
        {entry.patchSummary && (
          <div className="mt-2 flex flex-wrap gap-2 border-t border-[var(--border)] pt-2">
            {entry.patchSummary.added > 0 && (
              <span className="rounded bg-[var(--accent-green)]/20 px-1.5 py-0.5 text-[10px] font-medium text-[var(--accent-green)]">
                +{entry.patchSummary.added} added
              </span>
            )}
            {entry.patchSummary.removed > 0 && (
              <span className="rounded bg-red-500/20 px-1.5 py-0.5 text-[10px] font-medium text-red-400">
                -{entry.patchSummary.removed} removed
              </span>
            )}
            {entry.patchSummary.updated > 0 && (
              <span className="rounded bg-yellow-500/20 px-1.5 py-0.5 text-[10px] font-medium text-yellow-400">
                ~{entry.patchSummary.updated} updated
              </span>
            )}
          </div>
        )}

        {/* Warnings */}
        {showPatchActions && pendingPatchWarnings.length > 0 && (
          <div className="mt-2 rounded border border-yellow-500/30 bg-yellow-500/10 px-2 py-1.5 text-[11px] text-yellow-400">
            {pendingPatchWarnings.map((w, i) => (
              <div key={i}>{w}</div>
            ))}
          </div>
        )}

        {/* Apply/Discard buttons */}
        {showPatchActions && (
          <div className="mt-2 flex items-center gap-2 border-t border-[var(--border)] pt-2">
            <button
              onClick={onApplyPatch}
              className="rounded bg-[var(--accent-green)] px-3 py-1 text-[11px] font-medium text-white hover:opacity-90"
            >
              Apply Changes
            </button>
            <button
              onClick={onDiscardPatch}
              className="rounded px-2 py-1 text-[11px] text-[var(--text-secondary)] hover:bg-[var(--bg-dark)] hover:text-[var(--text-primary)]"
            >
              Discard
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
