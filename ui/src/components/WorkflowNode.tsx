import { memo } from "react";
import { Handle, Position, type NodeProps } from "@xyflow/react";
import type { NodeRole } from "../bindings";
import { InlineRenameInput } from "./InlineRenameInput";

interface WorkflowNodeData {
  label: string;
  nodeType: string;
  icon: string;
  color: string;
  isActive: boolean;
  enabled: boolean;
  onDelete: () => void;
  role: NodeRole;
  autoId?: string;
  bodyCount?: number;
  onToggleCollapse?: () => void;
  subtitle?: string;
  isRenaming?: boolean;
  isHypothetical?: boolean;
  hideSourceHandle?: boolean;
  onRenameConfirm?: (newName: string) => void;
  onRenameCancel?: () => void;
  [key: string]: unknown;
}

const EXEC_HANDLE_CLS = "!h-2 !w-2 !rounded-full !border !bg-[var(--bg-panel)]";
const EXEC_STYLE = { borderColor: "var(--text-muted)" };

export const WorkflowNode = memo(function WorkflowNode({
  data,
  selected,
}: NodeProps) {
  const d = data as unknown as WorkflowNodeData;
  const {
    label,
    icon,
    color,
    isActive,
    enabled,
    onDelete,
    role,
    autoId,
    bodyCount,
    onToggleCollapse,
    subtitle,
    isRenaming,
    isHypothetical,
    onRenameConfirm,
    onRenameCancel,
  } = d;
  const isVerification = role === "Verification";
  const isCollapsedGroup = bodyCount != null;

  return (
    <div
      className={`group relative min-w-[140px] rounded-lg border-2 bg-[var(--bg-panel)] transition-shadow ${
        !enabled ? "opacity-50" : ""
      } ${isHypothetical ? "opacity-50 border-dashed" : ""}`}
      style={{
        borderColor: selected ? color : isVerification ? "#f59e0b" : "var(--border)",
        boxShadow: selected ? `0 0 12px ${color}33` : "none",
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className={EXEC_HANDLE_CLS}
        style={EXEC_STYLE}
      />

      {isActive && (
        <span className="absolute -right-1 -top-1 h-3 w-3 animate-pulse rounded-full bg-[var(--accent-green)]" />
      )}

      {isVerification && (
        <span className="absolute -left-1 -top-1 flex h-4 w-4 items-center justify-center rounded-full bg-amber-500 text-[8px] font-bold text-white">
          ✓
        </span>
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
        <div className="flex flex-col min-w-0 max-w-[180px]">
          {isRenaming && onRenameConfirm && onRenameCancel ? (
            <InlineRenameInput label={label} onConfirm={onRenameConfirm} onCancel={onRenameCancel} />
          ) : (
            <span className="text-xs font-medium text-[var(--text-primary)] truncate">
              {label}
            </span>
          )}
          {subtitle && (
            <span className="text-[10px] text-[var(--text-muted)] truncate max-w-full">
              {subtitle}
            </span>
          )}
          {autoId && (
            <span className="text-[9px] font-mono text-[var(--text-muted)] opacity-60">
              {autoId}
            </span>
          )}
        </div>
        {isCollapsedGroup && bodyCount != null && (
          <span className="text-[10px] text-[var(--text-muted)] transition-opacity duration-150">
            {bodyCount} {bodyCount === 1 ? "step" : "steps"}
          </span>
        )}
        {isCollapsedGroup && onToggleCollapse && (
          <button
            onClick={(e) => {
              e.stopPropagation();
              onToggleCollapse();
            }}
            className="ml-auto flex h-5 w-5 items-center justify-center rounded text-[10px] text-[var(--text-muted)] transition-opacity duration-150 hover:bg-[var(--bg-surface)] hover:text-[var(--text-primary)]"
            title="Expand"
          >
            &#x25B6;
          </button>
        )}
      </div>

      <Handle
        type="source"
        position={Position.Right}
        className={EXEC_HANDLE_CLS}
        style={EXEC_STYLE}
        isConnectable={!d.hideSourceHandle}
      />
    </div>
  );
});
