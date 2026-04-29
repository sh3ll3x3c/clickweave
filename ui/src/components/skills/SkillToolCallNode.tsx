import { memo } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";

export interface SkillToolCallNodeData {
  tool: string;
  args: unknown;
}

const HANDLE_CLS = "!h-2 !w-2 !rounded-full !border !bg-[var(--bg-panel)]";
const HANDLE_STYLE = { borderColor: "var(--text-muted)" };

export const SkillToolCallNode = memo(function SkillToolCallNode({
  data,
  selected,
}: NodeProps) {
  const d = data as unknown as SkillToolCallNodeData;
  const compact = compactJson(d.args);
  const chips = extractBindingChips(compact);

  return (
    <div
      className="min-w-[190px] rounded-lg border bg-[var(--bg-panel)] px-3 py-2 text-xs shadow-sm"
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
        <span className="h-2 w-2 rounded-full bg-[var(--accent-green)]" />
        <span className="font-semibold text-[var(--text-primary)]">
          {d.tool}
        </span>
      </div>
      {chips.length > 0 && (
        <div className="mb-1 flex max-w-[220px] flex-wrap gap-1">
          {chips.map((chip) => (
            <span
              key={chip}
              className="rounded border border-[var(--border)] bg-[var(--bg-input)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--text-secondary)]"
            >
              {chip}
            </span>
          ))}
        </div>
      )}
      <pre className="max-w-[240px] overflow-hidden whitespace-pre-wrap break-words font-mono text-[10px] leading-4 text-[var(--text-muted)]">
        {compact}
      </pre>
      <Handle
        type="source"
        position={Position.Right}
        className={HANDLE_CLS}
        style={HANDLE_STYLE}
      />
    </div>
  );
});

function compactJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function extractBindingChips(text: string): string[] {
  const chips = new Set<string>();
  const re = /{{\s*(params|captured)\.([^}\s]+)\s*}}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(text)) != null) {
    chips.add(`${match[1]}.${match[2]}`);
  }
  return [...chips];
}
