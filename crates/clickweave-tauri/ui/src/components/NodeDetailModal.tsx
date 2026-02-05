import { useCallback, useEffect, useState } from "react";
import { commands } from "../bindings";
import type {
  Artifact,
  Node,
  NodeRun,
  NodeType,
  Check,
  CheckType,
  TraceEvent,
} from "../bindings";
import type { DetailTab } from "../store/useAppStore";

interface NodeDetailModalProps {
  node: Node | null;
  projectPath: string | null;
  workflowId: string;
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
  tab,
  onTabChange,
  onUpdate,
  onClose,
}: NodeDetailModalProps) {
  if (!node) return null;

  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center pt-16 bg-black/40">
      <div className="w-[560px] max-h-[80vh] flex flex-col rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] shadow-xl">
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
              nodeId={node.id}
              projectPath={projectPath}
              workflowId={workflowId}
            />
          )}
          {tab === "checks" && (
            <ChecksTab node={node} onUpdate={(u) => onUpdate(node.id, u)} />
          )}
          {tab === "runs" && (
            <RunsTab
              nodeId={node.id}
              projectPath={projectPath}
              workflowId={workflowId}
              onSelectRun={() => onTabChange("trace")}
            />
          )}
        </div>
      </div>
    </div>
  );
}

// ============================================================
// Setup Tab
// ============================================================

function SetupTab({
  node,
  onUpdate,
  projectPath,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
  projectPath: string | null;
}) {
  return (
    <div className="space-y-4">
      <FieldGroup title="General">
        <TextField
          label="Name"
          value={node.name}
          onChange={(name) => onUpdate({ name })}
        />
        <CheckboxField
          label="Enabled"
          value={node.enabled}
          onChange={(enabled) => onUpdate({ enabled })}
        />
        <NumberField
          label="Timeout (ms)"
          value={node.timeout_ms ?? 0}
          onChange={(v) => onUpdate({ timeout_ms: v === 0 ? null : v })}
        />
        <NumberField
          label="Retries"
          value={node.retries}
          min={0}
          max={10}
          onChange={(retries) => onUpdate({ retries })}
        />
        <SelectField
          label="Trace Level"
          value={node.trace_level}
          options={["Off", "Minimal", "Full"]}
          onChange={(trace_level) =>
            onUpdate({ trace_level: trace_level as Node["trace_level"] })
          }
        />
        <TextField
          label="Expected Outcome"
          value={node.expected_outcome ?? ""}
          onChange={(v) =>
            onUpdate({ expected_outcome: v === "" ? null : v })
          }
          placeholder="Optional"
        />
      </FieldGroup>

      <NodeTypeFields
        node={node}
        onUpdate={onUpdate}
        projectPath={projectPath}
      />
    </div>
  );
}

function NodeTypeFields({
  node,
  onUpdate,
  projectPath,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
  projectPath: string | null;
}) {
  const nt = node.node_type;

  const updateType = (patch: Partial<NodeType>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  switch (nt.type) {
    case "AiStep":
      return (
        <FieldGroup title="AI Step">
          <TextAreaField
            label="Prompt"
            value={nt.prompt}
            onChange={(prompt) => updateType({ prompt } as Partial<NodeType>)}
          />
          <TextField
            label="Button Text"
            value={nt.button_text ?? ""}
            onChange={(v) =>
              updateType({ button_text: v === "" ? null : v } as Partial<NodeType>)
            }
            placeholder="Optional"
          />
          <ImagePathField
            label="Template Image"
            value={nt.template_image ?? ""}
            projectPath={projectPath}
            onChange={(v) =>
              updateType({
                template_image: v === "" ? null : v,
              } as Partial<NodeType>)
            }
          />
          <NumberField
            label="Max Tool Calls"
            value={nt.max_tool_calls ?? 10}
            min={1}
            max={100}
            onChange={(v) =>
              updateType({ max_tool_calls: v } as Partial<NodeType>)
            }
          />
          <TextField
            label="Allowed Tools"
            value={nt.allowed_tools?.join(", ") ?? ""}
            onChange={(v) =>
              updateType({
                allowed_tools:
                  v === ""
                    ? null
                    : v.split(",").map((s) => s.trim()),
              } as Partial<NodeType>)
            }
            placeholder="Comma-separated, blank = all"
          />
        </FieldGroup>
      );

    case "TakeScreenshot":
      return (
        <FieldGroup title="Take Screenshot">
          <SelectField
            label="Mode"
            value={nt.mode}
            options={["Screen", "Window", "Region"]}
            onChange={(v) => updateType({ mode: v } as Partial<NodeType>)}
          />
          <TextField
            label="Target"
            value={nt.target ?? ""}
            onChange={(v) =>
              updateType({ target: v === "" ? null : v } as Partial<NodeType>)
            }
            placeholder="App name or window ID"
          />
          <CheckboxField
            label="Include OCR"
            value={nt.include_ocr}
            onChange={(v) =>
              updateType({ include_ocr: v } as Partial<NodeType>)
            }
          />
        </FieldGroup>
      );

    case "FindText":
      return (
        <FieldGroup title="Find Text">
          <TextField
            label="Search Text"
            value={nt.search_text}
            onChange={(v) =>
              updateType({ search_text: v } as Partial<NodeType>)
            }
          />
          <SelectField
            label="Match Mode"
            value={nt.match_mode}
            options={["Contains", "Exact"]}
            onChange={(v) =>
              updateType({ match_mode: v } as Partial<NodeType>)
            }
          />
          <TextField
            label="Scope"
            value={nt.scope ?? ""}
            onChange={(v) =>
              updateType({ scope: v === "" ? null : v } as Partial<NodeType>)
            }
            placeholder="Optional"
          />
          <TextField
            label="Select Result"
            value={nt.select_result ?? ""}
            onChange={(v) =>
              updateType({
                select_result: v === "" ? null : v,
              } as Partial<NodeType>)
            }
            placeholder="Optional"
          />
        </FieldGroup>
      );

    case "FindImage":
      return (
        <FieldGroup title="Find Image">
          <ImagePathField
            label="Template Image"
            value={nt.template_image ?? ""}
            projectPath={projectPath}
            onChange={(v) =>
              updateType({
                template_image: v === "" ? null : v,
              } as Partial<NodeType>)
            }
          />
          <NumberField
            label="Threshold"
            value={nt.threshold}
            min={0}
            max={1}
            step={0.01}
            onChange={(v) =>
              updateType({ threshold: v } as Partial<NodeType>)
            }
          />
          <NumberField
            label="Max Results"
            value={nt.max_results}
            min={1}
            max={20}
            onChange={(v) =>
              updateType({ max_results: v } as Partial<NodeType>)
            }
          />
        </FieldGroup>
      );

    case "Click":
      return (
        <FieldGroup title="Click">
          <TextField
            label="Target"
            value={nt.target ?? ""}
            onChange={(v) =>
              updateType({ target: v === "" ? null : v } as Partial<NodeType>)
            }
            placeholder="Coordinates or element"
          />
          <SelectField
            label="Button"
            value={nt.button}
            options={["Left", "Right", "Center"]}
            onChange={(v) => updateType({ button: v } as Partial<NodeType>)}
          />
          <NumberField
            label="Click Count"
            value={nt.click_count}
            min={1}
            max={3}
            onChange={(v) =>
              updateType({ click_count: v } as Partial<NodeType>)
            }
          />
        </FieldGroup>
      );

    case "TypeText":
      return (
        <FieldGroup title="Type Text">
          <TextAreaField
            label="Text"
            value={nt.text}
            onChange={(v) => updateType({ text: v } as Partial<NodeType>)}
          />
          <CheckboxField
            label="Press Enter After"
            value={nt.press_enter}
            onChange={(v) =>
              updateType({ press_enter: v } as Partial<NodeType>)
            }
          />
        </FieldGroup>
      );

    case "Scroll":
      return (
        <FieldGroup title="Scroll">
          <NumberField
            label="Delta Y"
            value={nt.delta_y}
            min={-1000}
            max={1000}
            onChange={(v) => updateType({ delta_y: v } as Partial<NodeType>)}
          />
          <NumberField
            label="X Position"
            value={nt.x ?? 0}
            onChange={(v) =>
              updateType({ x: v === 0 ? null : v } as Partial<NodeType>)
            }
          />
          <NumberField
            label="Y Position"
            value={nt.y ?? 0}
            onChange={(v) =>
              updateType({ y: v === 0 ? null : v } as Partial<NodeType>)
            }
          />
        </FieldGroup>
      );

    case "ListWindows":
      return (
        <FieldGroup title="List Windows">
          <TextField
            label="App Name Filter"
            value={nt.app_name ?? ""}
            onChange={(v) =>
              updateType({
                app_name: v === "" ? null : v,
              } as Partial<NodeType>)
            }
            placeholder="Optional"
          />
          <TextField
            label="Title Pattern"
            value={nt.title_pattern ?? ""}
            onChange={(v) =>
              updateType({
                title_pattern: v === "" ? null : v,
              } as Partial<NodeType>)
            }
            placeholder="Optional"
          />
        </FieldGroup>
      );

    case "FocusWindow":
      return (
        <FieldGroup title="Focus Window">
          <SelectField
            label="Method"
            value={nt.method}
            options={["WindowId", "AppName", "TitlePattern"]}
            onChange={(v) => updateType({ method: v } as Partial<NodeType>)}
          />
          <TextField
            label={
              { WindowId: "Window ID", AppName: "App Name", TitlePattern: "Title Pattern" }[nt.method] ?? nt.method
            }
            value={nt.value ?? ""}
            onChange={(v) =>
              updateType({ value: v === "" ? null : v } as Partial<NodeType>)
            }
          />
          <CheckboxField
            label="Bring to Front"
            value={nt.bring_to_front}
            onChange={(v) =>
              updateType({ bring_to_front: v } as Partial<NodeType>)
            }
          />
        </FieldGroup>
      );

    case "AppDebugKitOp":
      return (
        <FieldGroup title="AppDebugKit">
          <TextField
            label="Operation Name"
            value={nt.operation_name}
            onChange={(v) =>
              updateType({ operation_name: v } as Partial<NodeType>)
            }
          />
          <TextAreaField
            label="Parameters (JSON)"
            value={
              typeof nt.parameters === "string"
                ? nt.parameters
                : JSON.stringify(nt.parameters, null, 2)
            }
            onChange={(v) => {
              try {
                const parsed = JSON.parse(v);
                updateType({ parameters: parsed } as Partial<NodeType>);
              } catch {
                // Keep raw text during editing
              }
            }}
          />
        </FieldGroup>
      );
  }
}

// ============================================================
// Checks Tab
// ============================================================

function ChecksTab({
  node,
  onUpdate,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
}) {
  const checks = node.checks;

  const addCheck = (checkType: CheckType) => {
    const newCheck: Check = {
      name: `Check ${checks.length + 1}`,
      check_type: checkType,
      params: {},
      on_fail: "FailNode",
    };
    onUpdate({ checks: [...checks, newCheck] });
  };

  const removeCheck = (index: number) => {
    onUpdate({ checks: checks.filter((_, i) => i !== index) });
  };

  return (
    <div className="space-y-4">
      {checks.map((check, i) => (
        <div
          key={i}
          className="rounded border border-[var(--border)] bg-[var(--bg-input)] p-3"
        >
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--text-primary)]">
              {check.name} ({check.check_type})
            </span>
            <button
              onClick={() => removeCheck(i)}
              className="text-xs text-red-400 hover:text-red-300"
            >
              Delete
            </button>
          </div>
          <div className="mt-1 text-[10px] text-[var(--text-muted)]">
            On fail: {check.on_fail}
          </div>
        </div>
      ))}

      <div>
        <h4 className="mb-2 text-xs font-semibold text-[var(--text-muted)]">
          Add Check
        </h4>
        <div className="flex flex-wrap gap-1">
          {(
            [
              "TextPresent",
              "TextAbsent",
              "TemplateFound",
              "WindowTitleMatches",
            ] as CheckType[]
          ).map((ct) => (
            <button
              key={ct}
              onClick={() => addCheck(ct)}
              className="rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              + {ct}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

// ============================================================
// Shared helpers
// ============================================================

function runDuration(run: NodeRun): string | null {
  if (!run.ended_at) return null;
  return ((run.ended_at - run.started_at) / 1000).toFixed(1);
}

function EmptyState({ message }: { message: string }) {
  return (
    <div className="flex h-32 items-center justify-center text-xs text-[var(--text-muted)]">
      {message}
    </div>
  );
}

// ============================================================
// Shared hook for loading runs
// ============================================================

function useNodeRuns(
  projectPath: string | null,
  workflowId: string,
  nodeId: string,
): NodeRun[] {
  const [runs, setRuns] = useState<NodeRun[]>([]);

  useEffect(() => {
    if (!projectPath) return;
    commands
      .listRuns({
        project_path: projectPath,
        workflow_id: workflowId,
        node_id: nodeId,
      })
      .then((result) => {
        if (result.status === "ok") {
          setRuns([...result.data].reverse());
        }
      });
  }, [projectPath, workflowId, nodeId]);

  return runs;
}

// ============================================================
// Trace Tab
// ============================================================

function TraceTab({
  nodeId,
  projectPath,
  workflowId,
}: {
  nodeId: string;
  projectPath: string | null;
  workflowId: string;
}) {
  const runs = useNodeRuns(projectPath, workflowId, nodeId);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [events, setEvents] = useState<TraceEvent[]>([]);
  const [artifactPreviews, setArtifactPreviews] = useState<
    Record<string, string>
  >({});

  // Auto-select first run when runs load
  useEffect(() => {
    if (runs.length > 0 && !selectedRunId) {
      setSelectedRunId(runs[0].run_id);
    }
  }, [runs, selectedRunId]);

  // Load events for selected run
  useEffect(() => {
    if (!projectPath || !selectedRunId) {
      setEvents([]);
      return;
    }
    commands
      .loadRunEvents({
        project_path: projectPath,
        workflow_id: workflowId,
        node_id: nodeId,
        run_id: selectedRunId,
      })
      .then((result) => {
        if (result.status === "ok") {
          setEvents(result.data);
        }
      });
  }, [projectPath, workflowId, nodeId, selectedRunId]);

  const selectedRun = runs.find((r) => r.run_id === selectedRunId) ?? null;

  // Load artifact previews for selected run
  useEffect(() => {
    if (!selectedRun) return;
    const screenshots = selectedRun.artifacts.filter(
      (a) => a.kind === "Screenshot",
    );
    for (const art of screenshots) {
      if (artifactPreviews[art.artifact_id]) continue;
      commands.readArtifactBase64(art.path).then((result) => {
        if (result.status === "ok") {
          setArtifactPreviews((prev) => ({
            ...prev,
            [art.artifact_id]: result.data,
          }));
        }
      });
    }
  }, [selectedRun]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!projectPath) {
    return <EmptyState message="Save the project first to see trace data." />;
  }

  if (runs.length === 0) {
    return <EmptyState message="No runs yet. Execute the workflow to see trace data." />;
  }

  const duration = selectedRun ? runDuration(selectedRun) : null;

  return (
    <div className="space-y-4">
      {/* Run selector */}
      <div className="flex items-center gap-2">
        <label className="text-xs text-[var(--text-secondary)]">Run:</label>
        <select
          value={selectedRunId ?? ""}
          onChange={(e) => setSelectedRunId(e.target.value)}
          className="flex-1 rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
        >
          {runs.map((run) => (
            <option key={run.run_id} value={run.run_id}>
              {new Date(run.started_at).toLocaleString()} — {run.status}
            </option>
          ))}
        </select>
      </div>

      {/* Run summary */}
      {selectedRun && (
        <div className="flex items-center gap-3 rounded bg-[var(--bg-input)] px-3 py-2">
          <StatusBadge status={selectedRun.status} />
          {duration && (
            <span className="text-xs text-[var(--text-secondary)]">
              {duration}s
            </span>
          )}
          <span className="text-xs text-[var(--text-muted)]">
            {events.length} events
          </span>
          <span className="text-xs text-[var(--text-muted)]">
            {selectedRun.artifacts.length} artifacts
          </span>
        </div>
      )}

      {/* Events timeline */}
      {events.length > 0 && (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
            Events
          </h4>
          <div className="max-h-48 space-y-1 overflow-y-auto">
            {events.map((event, i) => (
              <div
                key={i}
                className="flex items-start gap-2 rounded bg-[var(--bg-input)] px-2.5 py-1.5"
              >
                <span className="mt-px shrink-0 text-[10px] font-mono text-[var(--text-muted)]">
                  {new Date(event.timestamp).toLocaleTimeString()}
                </span>
                <span
                  className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium ${eventTypeColor(event.event_type)}`}
                >
                  {event.event_type}
                </span>
                <span className="text-[11px] text-[var(--text-secondary)] truncate">
                  {formatEventPayload(event.payload)}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Artifacts */}
      {selectedRun && selectedRun.artifacts.length > 0 && (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
            Artifacts
          </h4>
          <div className="grid grid-cols-2 gap-2">
            {selectedRun.artifacts.map((art) => (
              <ArtifactCard
                key={art.artifact_id}
                artifact={art}
                preview={artifactPreviews[art.artifact_id]}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

const statusColors: Record<string, string> = {
  Ok: "bg-[var(--accent-green)]/20 text-[var(--accent-green)]",
  Failed: "bg-red-500/20 text-red-400",
};

function StatusBadge({ status }: { status: string }) {
  const colors = statusColors[status] ?? "bg-yellow-500/20 text-yellow-400";
  return (
    <span className={`rounded px-2 py-0.5 text-[10px] font-medium ${colors}`}>
      {status}
    </span>
  );
}

const eventTypeColors: Record<string, string> = {
  node_started: "bg-blue-500/20 text-blue-400",
  tool_call: "bg-purple-500/20 text-purple-400",
  tool_result: "bg-green-500/20 text-green-400",
  retry: "bg-yellow-500/20 text-yellow-400",
};

function eventTypeColor(eventType: string): string {
  return eventTypeColors[eventType] ?? "bg-[var(--bg-hover)] text-[var(--text-secondary)]";
}

const payloadFormatters: [string, (v: unknown) => string][] = [
  ["name", (v) => String(v)],
  ["type", (v) => String(v)],
  ["error", (v) => `error: ${v}`],
  ["attempt", (v) => `attempt ${v}`],
  ["text_len", (v) => `${v} chars`],
  ["image_count", (v) => `${v} images`],
];

function formatEventPayload(payload: unknown): string {
  if (payload == null) return "";
  if (typeof payload === "string") return payload;
  if (typeof payload !== "object") return String(payload);

  const obj = payload as Record<string, unknown>;
  const parts = payloadFormatters
    .filter(([key]) => obj[key])
    .map(([key, fmt]) => fmt(obj[key]));
  return parts.join(" | ") || JSON.stringify(payload);
}

function ArtifactCard({
  artifact,
  preview,
}: {
  artifact: Artifact;
  preview?: string;
}) {
  const filename = artifact.path.split("/").pop() ?? artifact.path;
  const isImage = artifact.kind === "Screenshot" || artifact.kind === "TemplateMatch";

  return (
    <div className="rounded border border-[var(--border)] bg-[var(--bg-input)] p-2">
      {isImage && preview ? (
        <img
          src={`data:image/png;base64,${preview}`}
          alt={filename}
          className="mb-1.5 w-full rounded object-contain"
          style={{ maxHeight: 120 }}
        />
      ) : (
        <div className="mb-1.5 flex h-16 items-center justify-center rounded bg-[var(--bg-dark)] text-xs text-[var(--text-muted)]">
          {artifact.kind}
        </div>
      )}
      <div className="truncate text-[10px] text-[var(--text-secondary)]">
        {filename}
      </div>
    </div>
  );
}

// ============================================================
// Runs Tab
// ============================================================

function RunsTab({
  nodeId,
  projectPath,
  workflowId,
  onSelectRun,
}: {
  nodeId: string;
  projectPath: string | null;
  workflowId: string;
  onSelectRun: () => void;
}) {
  const runs = useNodeRuns(projectPath, workflowId, nodeId);

  if (!projectPath) {
    return <EmptyState message="Save the project first to see run history." />;
  }

  if (runs.length === 0) {
    return <EmptyState message="No runs yet. Execute the workflow to create runs." />;
  }

  return (
    <div className="space-y-1">
      {runs.map((run) => {
        const duration = runDuration(run);

        return (
          <button
            key={run.run_id}
            onClick={onSelectRun}
            className="flex w-full items-center gap-3 rounded bg-[var(--bg-input)] px-3 py-2 text-left transition-colors hover:bg-[var(--bg-hover)]"
          >
            <StatusBadge status={run.status} />
            <span className="flex-1 text-xs text-[var(--text-primary)]">
              {new Date(run.started_at).toLocaleString()}
            </span>
            {duration && (
              <span className="text-xs text-[var(--text-muted)]">
                {duration}s
              </span>
            )}
            <span className="text-xs text-[var(--text-muted)]">
              {run.artifacts.length} artifacts
            </span>
          </button>
        );
      })}
    </div>
  );
}

// ============================================================
// Reusable Field Components
// ============================================================

function FieldGroup({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
        {title}
      </h3>
      <div className="space-y-2">{children}</div>
    </div>
  );
}

function TextField({
  label,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-[var(--text-secondary)]">
        {label}
      </label>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] placeholder-[var(--text-muted)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
      />
    </div>
  );
}

function TextAreaField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-[var(--text-secondary)]">
        {label}
      </label>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        rows={4}
        className="w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)] font-mono resize-y"
      />
    </div>
  );
}

function NumberField({
  label,
  value,
  onChange,
  min,
  max,
  step,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  min?: number;
  max?: number;
  step?: number;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-[var(--text-secondary)]">
        {label}
      </label>
      <input
        type="number"
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        min={min}
        max={max}
        step={step}
        className="w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
      />
    </div>
  );
}

function CheckboxField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 cursor-pointer">
      <input
        type="checkbox"
        checked={value}
        onChange={(e) => onChange(e.target.checked)}
        className="rounded border-[var(--border)] bg-[var(--bg-input)] accent-[var(--accent-coral)]"
      />
      <span className="text-xs text-[var(--text-secondary)]">{label}</span>
    </label>
  );
}

function SelectField({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: string[];
  onChange: (v: string) => void;
}) {
  return (
    <div>
      <label className="mb-1 block text-xs text-[var(--text-secondary)]">
        {label}
      </label>
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
      >
        {options.map((opt) => (
          <option key={opt} value={opt}>
            {opt}
          </option>
        ))}
      </select>
    </div>
  );
}

function ImagePathField({
  label,
  value,
  projectPath,
  onChange,
}: {
  label: string;
  value: string;
  projectPath: string | null;
  onChange: (v: string) => void;
}) {
  const handleBrowse = useCallback(async () => {
    if (!projectPath) return;
    const result = await commands.importAsset(projectPath);
    if (result.status === "ok" && result.data) {
      onChange(result.data.relative_path);
    }
  }, [projectPath, onChange]);

  return (
    <div>
      <label className="mb-1 block text-xs text-[var(--text-secondary)]">
        {label}
      </label>
      <div className="flex gap-1.5">
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={projectPath ? "Select an image..." : "Save project first"}
          className="flex-1 rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] placeholder-[var(--text-muted)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
        />
        <button
          onClick={handleBrowse}
          disabled={!projectPath}
          className="rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Browse
        </button>
        {value && (
          <button
            onClick={() => onChange("")}
            className="rounded bg-[var(--bg-input)] px-2 py-1.5 text-xs text-red-400 hover:bg-red-500/20"
          >
            Clear
          </button>
        )}
      </div>
    </div>
  );
}
