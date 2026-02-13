import { useState } from "react";
import type { Node } from "../../bindings";
import type { DetailTab } from "../../store/useAppStore";
import { ChecksTab, RunsTab, SetupTab, TraceTab } from "./tabs";

interface NodeDetailModalProps {
  node: Node | null;
  projectPath: string | null;
  workflowId: string;
  workflowName: string;
  tab: DetailTab;
  onTabChange: (tab: DetailTab) => void;
  onUpdate: (id: string, updates: Partial<Node>) => void;
  onClose: () => void;
}

const tabs: { key: DetailTab; label: string }[] = [
  { key: "setup", label: "Setup" },
  { key: "trace", label: "Trace" },
  { key: "checks", label: "Checks" },
  { key: "runs", label: "Runs" },
];

export function NodeDetailModal({
  node,
  projectPath,
  workflowId,
  workflowName,
  tab,
  onTabChange,
  onUpdate,
  onClose,
}: NodeDetailModalProps) {
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);

  if (!node) return null;

  return (
    <div className="fixed right-0 top-0 z-40 flex h-full pt-[theme(spacing.12)]">
      <div className="w-[420px] flex flex-col border-l border-[var(--border)] bg-[var(--bg-panel)] shadow-xl">
        <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold text-[var(--text-primary)]">
              {node.name}
            </span>
            <span className="text-xs text-[var(--text-muted)]">
              {node.node_type.type}
            </span>
          </div>
          <button
            onClick={onClose}
            className="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
          >
            x
          </button>
        </div>

        <div className="flex border-b border-[var(--border)]">
          {tabs.map((t) => (
            <button
              key={t.key}
              onClick={() => onTabChange(t.key)}
              className={`px-4 py-2 text-xs font-medium transition-colors ${
                tab === t.key
                  ? "border-b-2 border-[var(--accent-coral)] text-[var(--text-primary)]"
                  : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto p-4">
          {tab === "setup" && (
            <SetupTab node={node} onUpdate={(u) => onUpdate(node.id, u)} projectPath={projectPath} />
          )}
          {tab === "trace" && (
            <TraceTab
              nodeName={node.name}
              projectPath={projectPath}
              workflowId={workflowId}
              workflowName={workflowName}
              initialRunId={selectedRunId}
            />
          )}
          {tab === "checks" && (
            <ChecksTab node={node} onUpdate={(u) => onUpdate(node.id, u)} />
          )}
          {tab === "runs" && (
            <RunsTab
              nodeName={node.name}
              projectPath={projectPath}
              workflowId={workflowId}
              workflowName={workflowName}
              onSelectRun={(runId) => {
                setSelectedRunId(runId);
                onTabChange("trace");
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
