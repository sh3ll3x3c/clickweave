import { useState } from "react";
import type { WorkflowPatch } from "../bindings";

interface AssistantModalProps {
  open: boolean;
  loading: boolean;
  error: string | null;
  patch: WorkflowPatch | null;
  onSubmit: (prompt: string) => void;
  onApply: () => void;
  onDiscard: () => void;
}

export function AssistantModal({
  open,
  loading,
  error,
  patch,
  onSubmit,
  onApply,
  onDiscard,
}: AssistantModalProps) {
  const [prompt, setPrompt] = useState("");

  if (!open) return null;

  const hasPatch = patch !== null;
  const isEmpty =
    hasPatch &&
    patch.added_nodes.length === 0 &&
    patch.removed_node_ids.length === 0 &&
    patch.updated_nodes.length === 0;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="flex max-h-[80vh] w-[560px] flex-col rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-3">
          <h2 className="text-sm font-medium text-[var(--text-primary)]">
            Assistant
          </h2>
          <button
            onClick={onDiscard}
            className="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
          >
            &times;
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4">
          {/* Prompt input */}
          <div>
            <label className="mb-1.5 block text-xs text-[var(--text-secondary)]">
              What would you like to change?
            </label>
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="e.g. Add a screenshot step before the click, remove the scroll step"
              rows={3}
              disabled={loading}
              className="w-full rounded border border-[var(--border)] bg-[var(--bg-input)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-coral)]"
            />
          </div>

          {/* Submit button */}
          {!hasPatch && (
            <button
              onClick={() => onSubmit(prompt)}
              disabled={loading || !prompt.trim()}
              className="w-full rounded bg-[var(--accent-coral)] px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50"
            >
              {loading ? "Generating patch..." : "Generate Changes"}
            </button>
          )}

          {/* Loading */}
          {loading && (
            <div className="flex items-center justify-center py-4">
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-[var(--accent-coral)] border-t-transparent" />
              <span className="ml-2 text-xs text-[var(--text-secondary)]">
                Generating changes...
              </span>
            </div>
          )}

          {/* Error */}
          {error && (
            <div className="rounded border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
              {error}
            </div>
          )}

          {/* Warnings */}
          {hasPatch && patch.warnings.length > 0 && (
            <div className="rounded border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-400">
              {patch.warnings.map((w, i) => (
                <div key={i}>{w}</div>
              ))}
            </div>
          )}

          {/* Patch preview */}
          {hasPatch && !isEmpty && (
            <div className="space-y-2">
              <h3 className="text-xs font-medium text-[var(--text-secondary)]">
                Changes
              </h3>
              <div className="rounded border border-[var(--border)] bg-[var(--bg-dark)] divide-y divide-[var(--border)]">
                {patch.added_nodes.map((node) => (
                  <div
                    key={node.id}
                    className="flex items-center gap-2 px-3 py-2"
                  >
                    <span className="text-[10px] font-medium text-[var(--accent-green)]">
                      + ADD
                    </span>
                    <span className="rounded bg-[var(--bg-hover)] px-1.5 py-0.5 text-[10px] text-[var(--accent-blue)]">
                      {node.node_type.type}
                    </span>
                    <span className="text-xs text-[var(--text-primary)]">
                      {node.name}
                    </span>
                  </div>
                ))}
                {patch.removed_node_ids.map((id) => (
                  <div
                    key={id}
                    className="flex items-center gap-2 px-3 py-2"
                  >
                    <span className="text-[10px] font-medium text-red-400">
                      - DEL
                    </span>
                    <span className="text-xs text-[var(--text-muted)]">
                      {id.slice(0, 8)}...
                    </span>
                  </div>
                ))}
                {patch.updated_nodes.map((node) => (
                  <div
                    key={node.id}
                    className="flex items-center gap-2 px-3 py-2"
                  >
                    <span className="text-[10px] font-medium text-yellow-400">
                      ~ UPD
                    </span>
                    <span className="rounded bg-[var(--bg-hover)] px-1.5 py-0.5 text-[10px] text-[var(--accent-blue)]">
                      {node.node_type.type}
                    </span>
                    <span className="text-xs text-[var(--text-primary)]">
                      {node.name}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          {hasPatch && isEmpty && (
            <div className="text-center text-xs text-[var(--text-muted)] py-4">
              No changes generated. Try a different prompt.
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 border-t border-[var(--border)] px-5 py-3">
          {hasPatch && !isEmpty ? (
            <>
              <button
                onClick={() => {
                  if (!prompt.trim()) return;
                  onSubmit(prompt);
                }}
                disabled={loading || !prompt.trim()}
                className="rounded px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
              >
                Regenerate
              </button>
              <button
                onClick={onDiscard}
                className="rounded px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
              >
                Cancel
              </button>
              <button
                onClick={onApply}
                className="rounded bg-[var(--accent-green)] px-4 py-1.5 text-xs font-medium text-white hover:opacity-90"
              >
                Apply Changes
              </button>
            </>
          ) : (
            <button
              onClick={onDiscard}
              className="rounded px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              Cancel
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
