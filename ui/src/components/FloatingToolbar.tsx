import { useState, useRef, useEffect } from "react";
import type { ExecutionMode } from "../bindings";

interface FloatingToolbarProps {
  executorState: "idle" | "running";
  executionMode: ExecutionMode;
  logsOpen: boolean;
  hasAiNodes: boolean;
  onToggleLogs: () => void;
  onRunStop: () => void;
  onAssistant: () => void;
  onSetExecutionMode: (mode: ExecutionMode) => void;
}

export function FloatingToolbar({
  executorState,
  executionMode,
  logsOpen,
  hasAiNodes,
  onToggleLogs,
  onRunStop,
  onAssistant,
  onSetExecutionMode,
}: FloatingToolbarProps) {
  const isRunning = executorState === "running";
  const [showConfirm, setShowConfirm] = useState(false);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!showModeMenu) return;
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setShowModeMenu(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [showModeMenu]);

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

  const runLabel = executionMode === "Test" ? "Test Workflow" : "Run Workflow";

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
        <div className="relative" ref={menuRef}>
          <div className="flex items-center">
            <button
              onClick={handleRunStop}
              title={isRunning ? "Stop workflow (⌘⇧Esc works globally)" : `${runLabel} (⌘R)`}
              className={`rounded-l px-3 py-1.5 text-xs font-medium transition-colors ${
                isRunning
                  ? "bg-red-500/20 text-red-400 hover:bg-red-500/30"
                  : "bg-[var(--accent-green)]/20 text-[var(--accent-green)] hover:bg-[var(--accent-green)]/30"
              }`}
            >
              {isRunning ? "Stop" : runLabel}
            </button>
            {!isRunning && (
              <button
                onClick={() => setShowModeMenu((prev) => !prev)}
                title="Switch execution mode"
                className="rounded-r border-l border-[var(--border)] bg-[var(--accent-green)]/20 px-1.5 py-1.5 text-xs text-[var(--accent-green)] hover:bg-[var(--accent-green)]/30"
              >
                <svg width="8" height="8" viewBox="0 0 8 8" fill="currentColor">
                  <path d="M1 3l3 3 3-3z" />
                </svg>
              </button>
            )}
          </div>
          {showModeMenu && (
            <div className="absolute bottom-full right-0 mb-1 w-40 rounded-md border border-[var(--border)] bg-[var(--bg-panel)] py-1 shadow-lg">
              <button
                onClick={() => {
                  onSetExecutionMode("Test");
                  setShowModeMenu(false);
                }}
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-[var(--bg-hover)] ${
                  executionMode === "Test"
                    ? "text-[var(--accent-green)]"
                    : "text-[var(--text-secondary)]"
                }`}
              >
                {executionMode === "Test" && (
                  <span className="text-[10px]">&#10003;</span>
                )}
                <span className={executionMode === "Test" ? "" : "ml-4"}>
                  Test Workflow
                </span>
              </button>
              <button
                onClick={() => {
                  onSetExecutionMode("Run");
                  setShowModeMenu(false);
                }}
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-[var(--bg-hover)] ${
                  executionMode === "Run"
                    ? "text-[var(--accent-green)]"
                    : "text-[var(--text-secondary)]"
                }`}
              >
                {executionMode === "Run" && (
                  <span className="text-[10px]">&#10003;</span>
                )}
                <span className={executionMode === "Run" ? "" : "ml-4"}>
                  Run Workflow
                </span>
              </button>
            </div>
          )}
        </div>
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
