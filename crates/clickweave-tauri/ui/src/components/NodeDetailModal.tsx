import { useMemo } from "react";
import type { Node, NodeType, Check, CheckType, OnCheckFail } from "../bindings";
import type { DetailTab } from "../store/useAppStore";

interface NodeDetailModalProps {
  node: Node | null;
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
  tab,
  onTabChange,
  onUpdate,
  onClose,
}: NodeDetailModalProps) {
  if (!node) return null;

  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center pt-16 bg-black/40">
      <div className="w-[560px] max-h-[80vh] flex flex-col rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] shadow-xl">
        {/* Header */}
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

        {/* Tabs */}
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

        {/* Tab content */}
        <div className="flex-1 overflow-y-auto p-4">
          {tab === "setup" && (
            <SetupTab node={node} onUpdate={(u) => onUpdate(node.id, u)} />
          )}
          {tab === "trace" && <TracePlaceholder />}
          {tab === "checks" && (
            <ChecksTab node={node} onUpdate={(u) => onUpdate(node.id, u)} />
          )}
          {tab === "runs" && <RunsPlaceholder />}
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
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
}) {
  return (
    <div className="space-y-4">
      {/* Common fields */}
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

      {/* Type-specific fields */}
      <NodeTypeFields node={node} onUpdate={onUpdate} />
    </div>
  );
}

function NodeTypeFields({
  node,
  onUpdate,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
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
          <TextField
            label="Template Image Path"
            value={nt.template_image ?? ""}
            onChange={(v) =>
              updateType({
                template_image: v === "" ? null : v,
              } as Partial<NodeType>)
            }
            placeholder="Optional"
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
          <TextField
            label="Template Image"
            value={nt.template_image ?? ""}
            onChange={(v) =>
              updateType({
                template_image: v === "" ? null : v,
              } as Partial<NodeType>)
            }
            placeholder="Path to template image"
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
              nt.method === "WindowId"
                ? "Window ID"
                : nt.method === "AppName"
                  ? "App Name"
                  : "Title Pattern"
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
      {/* Existing checks */}
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

      {/* Add check buttons */}
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
// Placeholders for Trace and Runs (M8)
// ============================================================

function TracePlaceholder() {
  return (
    <div className="flex h-32 items-center justify-center text-xs text-[var(--text-muted)]">
      Trace events will appear here after running the workflow.
    </div>
  );
}

function RunsPlaceholder() {
  return (
    <div className="flex h-32 items-center justify-center text-xs text-[var(--text-muted)]">
      Run history will appear here after executing nodes.
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
