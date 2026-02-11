import { useState } from "react";
import type { Workflow } from "../bindings";

interface PlannerModalProps {
  open: boolean;
  loading: boolean;
  error: string | null;
  pendingWorkflow: Workflow | null;
  warnings: string[];
  allowAiTransforms: boolean;
  allowAgentSteps: boolean;
  onGenerate: (intent: string) => void;
  onApply: () => void;
  onDiscard: () => void;
  onAllowAiTransformsChange: (allow: boolean) => void;
  onAllowAgentStepsChange: (allow: boolean) => void;
}

export function PlannerModal({
  open,
  loading,
  error,
  pendingWorkflow,
  warnings,
  allowAiTransforms,
  allowAgentSteps,
  onGenerate,
  onApply,
  onDiscard,
  onAllowAiTransformsChange,
  onAllowAgentStepsChange,
}: PlannerModalProps) {
  const [intent, setIntent] = useState("");

  if (!open) return null;

  const hasResult = pendingWorkflow !== null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="flex max-h-[80vh] w-[600px] flex-col rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-[var(--border)] px-5 py-3">
          <h2 className="text-sm font-medium text-[var(--text-primary)]">
            Generate Workflow
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
          {/* Intent input */}
          <div>
            <label className="mb-1.5 block text-xs text-[var(--text-secondary)]">
              Describe what this workflow should do
            </label>
            <textarea
              value={intent}
              onChange={(e) => setIntent(e.target.value)}
              placeholder="e.g. Open Safari, navigate to example.com, take a screenshot, find the login button and click it"
              rows={3}
              className="w-full rounded border border-[var(--border)] bg-[var(--bg-input)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-coral)]"
              disabled={loading}
            />
          </div>

          {/* Toggles */}
          <div className="flex items-center gap-4">
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input
                type="checkbox"
                checked={allowAiTransforms}
                onChange={(e) => onAllowAiTransformsChange(e.target.checked)}
                disabled={loading}
                className="accent-[var(--accent-coral)]"
              />
              AI Transforms
            </label>
            <label className="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input
                type="checkbox"
                checked={allowAgentSteps}
                onChange={(e) => onAllowAgentStepsChange(e.target.checked)}
                disabled={loading}
                className="accent-[var(--accent-coral)]"
              />
              Agent Steps
            </label>
          </div>

          {/* Generate button */}
          {!hasResult && (
            <button
              onClick={() => onGenerate(intent)}
              disabled={loading || !intent.trim()}
              className="w-full rounded bg-[var(--accent-coral)] px-4 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50"
            >
              {loading ? "Generating..." : "Generate"}
            </button>
          )}

          {/* Loading spinner */}
          {loading && (
            <div className="flex items-center justify-center py-4">
              <div className="h-5 w-5 animate-spin rounded-full border-2 border-[var(--accent-coral)] border-t-transparent" />
              <span className="ml-2 text-xs text-[var(--text-secondary)]">
                Planning workflow...
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
          {warnings.length > 0 && (
            <div className="rounded border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-400">
              {warnings.map((w, i) => (
                <div key={i}>{w}</div>
              ))}
            </div>
          )}

          {/* Preview */}
          {hasResult && (
            <div className="space-y-2">
              <h3 className="text-xs font-medium text-[var(--text-secondary)]">
                Preview ({pendingWorkflow.nodes.length} nodes)
              </h3>
              <div className="max-h-[300px] overflow-y-auto rounded border border-[var(--border)] bg-[var(--bg-dark)]">
                {pendingWorkflow.nodes.map((node, i) => (
                  <div
                    key={node.id}
                    className="flex items-center gap-3 border-b border-[var(--border)] px-3 py-2 last:border-b-0"
                  >
                    <span className="flex h-5 w-5 items-center justify-center rounded bg-[var(--bg-hover)] text-[10px] text-[var(--text-muted)]">
                      {i + 1}
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
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 border-t border-[var(--border)] px-5 py-3">
          {hasResult ? (
            <>
              <button
                onClick={() => {
                  // Re-generate: clear result and let user edit intent
                  onGenerate(intent);
                }}
                disabled={loading}
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
                Apply Workflow
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
