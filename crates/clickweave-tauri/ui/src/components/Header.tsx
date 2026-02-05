interface HeaderProps {
  workflowName: string;
  projectPath: string | null;
  executorState: "idle" | "running";
  onSave: () => void;
  onOpen: () => void;
  onNew: () => void;
  onSettings: () => void;
  onNameChange: (name: string) => void;
}

export function Header({
  workflowName,
  projectPath,
  executorState,
  onSave,
  onOpen,
  onNew,
  onSettings,
  onNameChange,
}: HeaderProps) {
  return (
    <div className="flex h-12 items-center justify-between border-b border-[var(--border)] bg-[var(--bg-panel)] px-4">
      <div className="flex items-center gap-3">
        <input
          type="text"
          value={workflowName}
          onChange={(e) => onNameChange(e.target.value)}
          className="border-none bg-transparent text-sm font-medium text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)] rounded px-1 py-0.5"
        />
        {projectPath && (
          <span className="text-xs text-[var(--text-muted)] truncate max-w-[200px]">
            {projectPath.split("/").pop()}
          </span>
        )}
      </div>

      <div className="flex items-center gap-2">
        {executorState === "running" && (
          <span className="flex items-center gap-1.5 text-xs text-[var(--accent-green)]">
            <span className="h-2 w-2 animate-pulse rounded-full bg-[var(--accent-green)]" />
            Running
          </span>
        )}
      </div>

      <div className="flex items-center gap-1">
        <button
          onClick={onNew}
          className="rounded px-2.5 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        >
          New
        </button>
        <button
          onClick={onOpen}
          className="rounded px-2.5 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        >
          Open
        </button>
        <button
          onClick={onSave}
          className="rounded bg-[var(--accent-coral)] px-3 py-1.5 text-xs font-medium text-white hover:opacity-90"
        >
          Save
        </button>
        <button
          onClick={onSettings}
          className="ml-2 rounded px-2.5 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
        >
          Settings
        </button>
      </div>
    </div>
  );
}
