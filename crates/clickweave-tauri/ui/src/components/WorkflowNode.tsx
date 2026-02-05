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
  [key: string]: unknown;
}

export const WorkflowNode = memo(function WorkflowNode({
  data,
  selected,
}: NodeProps) {
  const { label, icon, color, isActive, enabled, onDelete } =
    data as unknown as WorkflowNodeData;

  return (
    <div
      className={`group relative min-w-[140px] rounded-lg border-2 bg-[var(--bg-panel)] transition-shadow ${
        !enabled ? "opacity-50" : ""
      }`}
      style={{
        borderColor: selected ? color : "var(--border)",
        boxShadow: selected ? `0 0 12px ${color}33` : "none",
      }}
    >
      {/* Input handle */}
      <Handle
        type="target"
        position={Position.Left}
        className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
        style={{ borderColor: "var(--accent-green)" }}
      />

      {/* Active indicator */}
      {isActive && (
        <span className="absolute -right-1 -top-1 h-3 w-3 animate-pulse rounded-full bg-[var(--accent-green)]" />
      )}

      {/* Delete button */}
      <button
        onClick={(e) => {
          e.stopPropagation();
          onDelete();
        }}
        className="absolute -right-2 -top-2 hidden h-5 w-5 items-center justify-center rounded-full bg-red-500 text-[10px] text-white group-hover:flex"
      >
        x
      </button>

      {/* Node content */}
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

      {/* Output handle */}
      <Handle
        type="source"
        position={Position.Right}
        className="!h-3 !w-3 !rounded-full !border-2 !bg-[var(--bg-panel)]"
        style={{ borderColor: "var(--accent-coral)" }}
      />
    </div>
  );
});
