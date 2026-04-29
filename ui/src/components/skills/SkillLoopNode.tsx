import { memo } from "react";
import { type NodeProps } from "@xyflow/react";

export interface SkillLoopNodeData {
  label: string;
  until: string;
  maxIterations: number;
  childCount: number;
}

export const SkillLoopNode = memo(function SkillLoopNode({
  data,
  selected,
}: NodeProps) {
  const d = data as unknown as SkillLoopNodeData;

  return (
    <div
      className="relative rounded-[10px] border bg-[var(--accent-blue)]/5 text-xs"
      style={{
        borderColor: selected ? "var(--accent-coral)" : "var(--accent-blue)",
        width: "100%",
        height: "100%",
        minWidth: 260,
        minHeight: 160,
      }}
    >
      <div className="flex items-center gap-2 border-b border-[var(--accent-blue)]/30 px-3 py-1.5">
        <span className="h-2 w-2 rounded-full bg-[var(--accent-blue)]" />
        <span className="font-semibold text-[var(--text-primary)]">
          {d.label}
        </span>
        <span className="ml-auto text-[10px] text-[var(--text-muted)]">
          {d.childCount} step{d.childCount === 1 ? "" : "s"}
        </span>
      </div>
      <div className="px-3 py-2 text-[10px] text-[var(--text-secondary)]">
        <div>
          <span className="text-[var(--text-muted)]">until:</span> {d.until}
        </div>
        <div>
          <span className="text-[var(--text-muted)]">max:</span>{" "}
          {d.maxIterations}
        </div>
      </div>
    </div>
  );
});
