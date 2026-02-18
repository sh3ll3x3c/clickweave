import { useState } from "react";

interface FloatingToolbarProps {
  executorState: "idle" | "running";
  logsOpen: boolean;
  hasAiNodes: boolean;
  onToggleLogs: () => void;
  onRunStop: () => void;
  onAssistant: () => void;
}

export function FloatingToolbar({
  executorState,
  logsOpen,
  hasAiNodes,
  onToggleLogs,
  onRunStop,
  onAssistant,
}: FloatingToolbarProps) {
  const isRunning = executorState === "running";
  const [showConfirm, setShowConfirm] = useState(false);

  const handleRunStop = () => {
    if (isRunning) {
      onRunStop();
      return;
    }
    if (hasAiNodes) {
      setShowConfirm(true);
    } else {
      onRunStop();
    }
  };

  return (
    <>
      <div className="absolute bottom-14 left-1/2 z-20 flex -translate-x-1/2 items-center gap-1 rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] px-2 py-1 shadow-lg">
        <button
          onClick={onAssistant}
          className="rounded px-2.5 py-1.5 text-xs text-[var(--accent-blue)] hover:bg-[var(--bg-hover)]"
        >
          Assistant
        </button>
        <div className="mx-1 h-4 w-px bg-[var(--border)]" />
        <button
          onClick={onToggleLogs}
          className={`rounded px-2.5 py-1.5 text-xs transition-colors ${
            logsOpen
              ? "bg-[var(--bg-hover)] text-[var(--text-primary)]"
              : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]"
          }`}
        >
          Logs
        </button>
        <div className="mx-1 h-4 w-px bg-[var(--border)]" />
        {hasAiNodes && !isRunning && (
          <span className="rounded bg-[var(--accent-blue)]/20 px-1.5 py-0.5 text-[10px] font-medium text-[var(--accent-blue)]">
            AI
          </span>
        )}
        <button
          onClick={handleRunStop}
          title={isRunning ? "Stop workflow (⌘⇧Esc works globally)" : "Run workflow (⌘R)"}
          className={`rounded px-3 py-1.5 text-xs font-medium transition-colors ${
            isRunning
              ? "bg-red-500/20 text-red-400 hover:bg-red-500/30"
              : "bg-[var(--accent-green)]/20 text-[var(--accent-green)] hover:bg-[var(--accent-green)]/30"
          }`}
        >
          {isRunning ? "Stop" : "Test Workflow"}
        </button>
      </div>
      {isRunning && (
        <div className="absolute bottom-8 left-1/2 z-20 -translate-x-1/2 animate-pulse text-center text-[10px] text-red-400/70">
          Press <kbd className="rounded border border-red-500/30 bg-red-500/10 px-1 py-0.5 font-mono text-[9px] text-red-400">⌘⇧Esc</kbd> to stop from any app
        </div>
      )}

      {showConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
          <div className="w-[400px] rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] p-5 shadow-2xl">
            <h3 className="text-sm font-medium text-[var(--text-primary)]">
              Workflow contains AI nodes
            </h3>
            <p className="mt-2 text-xs text-[var(--text-secondary)]">
              This workflow includes non-deterministic AI steps that will make
              LLM calls during execution. Results may vary between runs.
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                onClick={() => setShowConfirm(false)}
                className="rounded px-3 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  setShowConfirm(false);
                  onRunStop();
                }}
                className="rounded bg-[var(--accent-green)] px-4 py-1.5 text-xs font-medium text-white hover:opacity-90"
              >
                Run Anyway
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
