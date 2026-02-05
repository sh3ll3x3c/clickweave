interface FloatingToolbarProps {
  executorState: "idle" | "running";
  logsOpen: boolean;
  onToggleLogs: () => void;
  onRunStop: () => void;
}

export function FloatingToolbar({
  executorState,
  logsOpen,
  onToggleLogs,
  onRunStop,
}: FloatingToolbarProps) {
  const isRunning = executorState === "running";

  return (
    <div className="absolute bottom-14 left-1/2 z-20 flex -translate-x-1/2 items-center gap-1 rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] px-2 py-1 shadow-lg">
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
      <button
        onClick={onRunStop}
        className={`rounded px-3 py-1.5 text-xs font-medium transition-colors ${
          isRunning
            ? "bg-red-500/20 text-red-400 hover:bg-red-500/30"
            : "bg-[var(--accent-green)]/20 text-[var(--accent-green)] hover:bg-[var(--accent-green)]/30"
        }`}
      >
        {isRunning ? "Stop" : "Test Workflow"}
      </button>
    </div>
  );
}
