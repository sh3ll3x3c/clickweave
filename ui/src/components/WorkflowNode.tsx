import { memo } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";

interface WorkflowNodeData {
  label: string;
  nodeType: string;
  icon: string;
  color: string;
  isActive: boolean;
  enabled: boolean;
  onDelete: () => void;
  switchCases: string[];
  [key: string]: unknown;
}

const CONTROL_FLOW_TYPES = new Set(["If", "Switch", "Loop", "EndLoop"]);

function SourceHandles({ data }: { data: WorkflowNodeData }) {
  const { nodeType, switchCases } = data;

  if (nodeType === "If") {
    return (
      <>
        <Handle
          type="source"
          position={Position.Right}
          id="IfTrue"
          className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
          style={{ borderColor: "#10b981", top: "30%" }}
        />
        <span
          className="absolute right-5 text-[8px] text-[var(--text-muted)]"
          style={{ top: "26%" }}
        >
          T
        </span>
        <Handle
          type="source"
          position={Position.Right}
          id="IfFalse"
          className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
          style={{ borderColor: "#ef4444", top: "70%" }}
        />
        <span
          className="absolute right-5 text-[8px] text-[var(--text-muted)]"
          style={{ top: "66%" }}
        >
          F
        </span>
      </>
    );
  }

  if (nodeType === "Loop") {
    return (
      <>
        <Handle
          type="source"
          position={Position.Right}
          id="LoopBody"
          className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
          style={{ borderColor: "#10b981", top: "30%" }}
        />
        <span
          className="absolute right-5 text-[8px] text-[var(--text-muted)]"
          style={{ top: "26%" }}
        >
          body
        </span>
        <Handle
          type="source"
          position={Position.Right}
          id="LoopDone"
          className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
          style={{ borderColor: "#f59e0b", top: "70%" }}
        />
        <span
          className="absolute right-5 text-[8px] text-[var(--text-muted)]"
          style={{ top: "66%" }}
        >
          done
        </span>
      </>
    );
  }

  if (nodeType === "Switch") {
    const totalHandles = switchCases.length + 1; // cases + default
    return (
      <>
        {switchCases.map((caseName, i) => {
          const pct = ((i + 1) / (totalHandles + 1)) * 100;
          return (
            <span key={caseName}>
              <Handle
                type="source"
                position={Position.Right}
                id={`SwitchCase:${caseName}`}
                className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
                style={{ borderColor: "#10b981", top: `${pct}%` }}
              />
              <span
                className="absolute right-5 text-[8px] text-[var(--text-muted)] whitespace-nowrap"
                style={{ top: `${pct - 4}%` }}
              >
                {caseName}
              </span>
            </span>
          );
        })}
        {/* Default handle */}
        {(() => {
          const pct = (totalHandles / (totalHandles + 1)) * 100;
          return (
            <span>
              <Handle
                type="source"
                position={Position.Right}
                id="SwitchDefault"
                className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
                style={{ borderColor: "#666", top: `${pct}%` }}
              />
              <span
                className="absolute right-5 text-[8px] text-[var(--text-muted)]"
                style={{ top: `${pct - 4}%` }}
              >
                default
              </span>
            </span>
          );
        })()}
      </>
    );
  }

  // EndLoop and regular nodes: single source handle
  return (
    <Handle
      type="source"
      position={Position.Right}
      className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
      style={{ borderColor: "var(--accent-coral)" }}
    />
  );
}

export const WorkflowNode = memo(function WorkflowNode({
  data,
  selected,
}: NodeProps) {
  const d = data as unknown as WorkflowNodeData;
  const { label, icon, color, isActive, enabled, onDelete, nodeType } = d;
  const isControlFlow = CONTROL_FLOW_TYPES.has(nodeType);
  const needsTallNode = nodeType === "If" || nodeType === "Loop";
  const needsExtraTallNode = nodeType === "Switch" && d.switchCases.length > 1;

  return (
    <div
      className={`group relative min-w-[140px] rounded-lg border-2 bg-[var(--bg-panel)] transition-shadow ${
        !enabled ? "opacity-50" : ""
      } ${needsTallNode ? "min-h-[60px]" : ""} ${needsExtraTallNode ? "min-h-[80px]" : ""}`}
      style={{
        borderColor: selected ? color : isControlFlow ? "#10b98144" : "var(--border)",
        boxShadow: selected ? `0 0 12px ${color}33` : "none",
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
        style={{ borderColor: "var(--accent-green)" }}
      />

      {isActive && (
        <span className="absolute -right-1 -top-1 h-3 w-3 animate-pulse rounded-full bg-[var(--accent-green)]" />
      )}

      <button
        onClick={(e) => {
          e.stopPropagation();
          onDelete();
        }}
        className="absolute -right-2 -top-2 hidden h-5 w-5 items-center justify-center rounded-full bg-red-500 text-[10px] text-white group-hover:flex"
      >
        x
      </button>

      <div className="flex items-center gap-2 px-3 py-2">
        <div
          className="flex h-7 w-7 items-center justify-center rounded text-[10px] font-bold text-white"
          style={{ backgroundColor: color }}
        >
          {icon}
        </div>
        <span className="text-xs font-medium text-[var(--text-primary)]">
          {label}
        </span>
      </div>

      <SourceHandles data={d} />
    </div>
  );
});
