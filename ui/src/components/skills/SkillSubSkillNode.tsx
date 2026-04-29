import { memo } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";

export interface SkillSubSkillNodeData {
  skillId: string;
  version: number;
  name?: string;
  parameters: unknown;
  bindOutputsAs: Record<string, string>;
  onOpen?: () => void;
}

const HANDLE_CLS = "!h-2 !w-2 !rounded-full !border !bg-[var(--bg-panel)]";
const HANDLE_STYLE = { borderColor: "var(--text-muted)" };

export const SkillSubSkillNode = memo(function SkillSubSkillNode({
  data,
  selected,
}: NodeProps) {
  const d = data as unknown as SkillSubSkillNodeData;
  const bindings = Object.entries(d.bindOutputsAs ?? {});

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        d.onOpen?.();
      }}
      className="relative min-w-[200px] rounded-lg border bg-[var(--bg-panel)] px-3 py-2 text-left text-xs shadow-sm"
      style={{
        borderColor: selected ? "var(--accent-coral)" : "var(--border)",
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className={HANDLE_CLS}
        style={HANDLE_STYLE}
      />
      <div className="mb-1 flex items-center gap-2">
        <span className="h-2 w-2 rounded-full bg-[var(--accent-blue)]" />
        <span className="font-semibold text-[var(--text-primary)]">
          {d.name ?? d.skillId}
        </span>
        <span className="text-[10px] text-[var(--text-muted)]">
          v{d.version}
        </span>
      </div>
      <pre className="max-w-[240px] overflow-hidden whitespace-pre-wrap break-words font-mono text-[10px] leading-4 text-[var(--text-muted)]">
        {JSON.stringify(d.parameters ?? {}, null, 2)}
      </pre>
      {bindings.length > 0 && (
        <div className="mt-2 space-y-1">
          {bindings.map(([from, to]) => (
            <div
              key={`${from}-${to}`}
              className="rounded bg-[var(--bg-input)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-secondary)]"
            >
              {from} -&gt; {to}
            </div>
          ))}
        </div>
      )}
      <Handle
        type="source"
        position={Position.Right}
        className={HANDLE_CLS}
        style={HANDLE_STYLE}
      />
    </button>
  );
});
