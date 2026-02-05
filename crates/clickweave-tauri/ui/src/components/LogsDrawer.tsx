import { useRef, useEffect } from "react";

interface LogsDrawerProps {
  open: boolean;
  logs: string[];
  onToggle: () => void;
  onClear: () => void;
}

export function LogsDrawer({ open, logs, onToggle, onClear }: LogsDrawerProps) {
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  return (
    <div
      className={`border-t border-[var(--border)] bg-[var(--bg-panel)] transition-all duration-200 ${
        open ? "h-48" : "h-8"
      }`}
    >
      {/* Toggle bar */}
      <div className="flex h-8 items-center justify-between border-b border-[var(--border)] px-3">
        <button
          onClick={onToggle}
          className="flex items-center gap-2 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
        >
          <span className={`transition-transform ${open ? "rotate-180" : ""}`}>
            ^
          </span>
          <span>Logs ({logs.length})</span>
        </button>
        {open && (
          <button
            onClick={onClear}
            className="text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)]"
          >
            Clear
          </button>
        )}
      </div>

      {/* Log content */}
      {open && (
        <div
          ref={scrollRef}
          className="h-[calc(100%-2rem)] overflow-y-auto p-2 font-mono text-[11px] leading-relaxed"
        >
          {logs.map((log, i) => (
            <div
              key={i}
              className={`py-0.5 ${
                log.includes("Error") || log.includes("failed")
                  ? "text-red-400"
                  : log.includes("completed") || log.includes("Saved")
                    ? "text-[var(--accent-green)]"
                    : "text-[var(--text-secondary)]"
              }`}
            >
              {log}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
