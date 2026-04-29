import { memo } from "react";
import { type NodeProps } from "@xyflow/react";

export interface AgentRunGroupData {
  runId: string;
  summary: string;
  stepCount: number;
  isCollapsed: boolean;
  onToggleCollapse: () => void;
  [key: string]: unknown;
}

export const AgentRunGroupNode = memo(function AgentRunGroupNode({
  data,
  selected,
}: NodeProps) {
  const d = data as unknown as AgentRunGroupData;
  const { runId, summary, stepCount, isCollapsed, onToggleCollapse } = d;
  const accent = "34, 197, 94";

  return (
    <div
      className="agent-run-group relative rounded-[10px] transition-all duration-150"
      data-run-id={runId}
      style={{
        border: `2px dashed rgba(${accent}, ${selected ? 0.75 : 0.42})`,
        backgroundColor: `rgba(${accent}, 0.055)`,
        width: "100%",
        height: "100%",
        minWidth: 300,
        minHeight: 150,
        boxShadow: selected ? `0 0 12px rgba(${accent}, 0.18)` : "none",
      }}
    >
      <div
        className="flex items-center gap-2 px-3 py-1.5"
        style={{ borderBottom: `1px solid rgba(${accent}, 0.15)` }}
      >
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            onToggleCollapse();
          }}
          className="flex h-5 w-5 items-center justify-center rounded text-[10px] text-[var(--text-muted)] hover:bg-[rgba(255,255,255,0.1)] hover:text-[var(--text-primary)]"
          title={isCollapsed ? "Expand run" : "Collapse run"}
        >
          {isCollapsed ? "\u25B6" : "\u25BC"}
        </button>
        <span className="min-w-0 truncate text-xs font-medium text-[var(--text-primary)]">
          {summary}
        </span>
        <span className="ml-auto shrink-0 text-[10px] text-[var(--text-muted)]">
          {stepCount} step{stepCount !== 1 ? "s" : ""}
        </span>
      </div>
    </div>
  );
});
