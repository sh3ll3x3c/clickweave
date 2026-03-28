import { useMemo, useState } from "react";
import type { Condition, LiteralValue, Operator } from "../../../../bindings";
import { useStore } from "../../../../store/useAppStore";
import { getFullOutputSchema, nodeTypeName } from "../../../../utils/outputSchema";

// The Rust Condition type uses:
//   left: OutputRef { node: string, field: string }
//   right: ConditionValue = { type: "Literal", value: LiteralValue } | { type: "Ref", node: string, field: string }
// bindings.ts may not have these yet (regenerated on debug build), so we use
// inline types that match the Rust serde shapes.

type OutputRef = { node: string; field: string };
type ConditionValue =
  | { type: "Literal"; value: LiteralValue }
  | { type: "Ref"; node: string; field: string };

// The Condition type from bindings may still reference old ValueRef types.
// Cast to the real shape at the boundary.
interface RealCondition {
  left: OutputRef;
  operator: Operator;
  right: ConditionValue;
}

function literalDisplayValue(cv: ConditionValue): string {
  if (cv.type !== "Literal") return "";
  return String((cv.value as { value: unknown }).value);
}

interface OutputRefPickerProps {
  nodeAutoId: string;
  fieldName: string;
  onChangeNode: (autoId: string) => void;
  onChangeField: (field: string) => void;
  nodeOptions: Array<{ autoId: string; typeName: string; nodeType: Record<string, unknown> }>;
}

function OutputRefPicker({
  nodeAutoId,
  fieldName,
  onChangeNode,
  onChangeField,
  nodeOptions,
}: OutputRefPickerProps) {
  const selected = nodeOptions.find((n) => n.autoId === nodeAutoId);
  const fields = selected ? getFullOutputSchema(selected.nodeType) : [];

  return (
    <div className="flex gap-1.5">
      <select
        value={nodeAutoId}
        onChange={(e) => {
          onChangeNode(e.target.value);
          const newOpt = nodeOptions.find((n) => n.autoId === e.target.value);
          const newFields = newOpt ? getFullOutputSchema(newOpt.nodeType) : [];
          onChangeField(newFields.length > 0 ? newFields[0].name : "");
        }}
        className="flex-1 rounded bg-[var(--bg-input)] px-2 py-1 text-xs text-[var(--text-primary)] outline-none"
      >
        <option value="">Select node...</option>
        {nodeOptions.map((opt) => (
          <option key={opt.autoId} value={opt.autoId}>
            {opt.autoId}
          </option>
        ))}
      </select>
      <select
        value={fieldName}
        onChange={(e) => onChangeField(e.target.value)}
        className="flex-1 rounded bg-[var(--bg-input)] px-2 py-1 text-xs text-[var(--text-primary)] outline-none"
        disabled={fields.length === 0}
      >
        {fields.length === 0 ? (
          <option value="">No fields</option>
        ) : (
          fields.map((f) => (
            <option key={f.name} value={f.name}>
              {f.name} ({f.type})
            </option>
          ))
        )}
      </select>
    </div>
  );
}

export function ConditionEditor({
  condition,
  onChange,
}: {
  condition: Condition;
  onChange: (c: Condition) => void;
}) {
  // Cast to the real shape (bindings may be stale until debug rebuild)
  const cond = condition as unknown as RealCondition;
  const emit = (c: RealCondition) => onChange(c as unknown as Condition);

  const workflowNodes = useStore((s) => s.workflow.nodes);

  const nodeOptions = useMemo(() => {
    const options: Array<{ autoId: string; typeName: string; nodeType: Record<string, unknown> }> = [];
    for (const node of workflowNodes) {
      if (!node.auto_id) continue;
      const nt = node.node_type as unknown as Record<string, unknown>;
      const typeName = nodeTypeName(nt);
      const schema = getFullOutputSchema(nt);
      if (schema.length > 0) {
        options.push({ autoId: node.auto_id, typeName, nodeType: nt });
      }
    }
    return options;
  }, [workflowNodes]);

  const leftNode = cond.left.node ?? "";
  const leftField = cond.left.field ?? "";

  const isRightRef = cond.right.type === "Ref";
  const [rightMode, setRightMode] = useState<"literal" | "variable">(
    isRightRef ? "variable" : "literal",
  );

  const rightNode = isRightRef ? (cond.right as { node: string }).node : "";
  const rightField = isRightRef ? (cond.right as { field: string }).field : "";

  return (
    <div className="space-y-2">
      {/* Left side: OutputRef picker */}
      <div>
        <label className="text-[10px] text-[var(--text-muted)]">Variable</label>
        <OutputRefPicker
          nodeAutoId={leftNode}
          fieldName={leftField}
          onChangeNode={(autoId) => {
            const opt = nodeOptions.find((n) => n.autoId === autoId);
            const fields = opt ? getFullOutputSchema(opt.nodeType) : [];
            const field = fields.length > 0 ? fields[0].name : "";
            emit({ ...cond, left: { node: autoId, field } });
          }}
          onChangeField={(field) => {
            emit({ ...cond, left: { node: leftNode, field } });
          }}
          nodeOptions={nodeOptions}
        />
      </div>

      {/* Operator */}
      <div className="flex gap-2">
        <div className="flex-1">
          <label className="text-[10px] text-[var(--text-muted)]">Operator</label>
          <select
            value={cond.operator}
            onChange={(e) =>
              emit({ ...cond, operator: e.target.value as Operator })
            }
            className="w-full rounded bg-[var(--bg-input)] px-2 py-1 text-xs text-[var(--text-primary)] outline-none"
          >
            <option value="Equals">Equals</option>
            <option value="NotEquals">Not Equals</option>
            <option value="GreaterThan">Greater Than</option>
            <option value="LessThan">Less Than</option>
            <option value="GreaterThanOrEqual">&ge;</option>
            <option value="LessThanOrEqual">&le;</option>
            <option value="Contains">Contains</option>
            <option value="NotContains">Not Contains</option>
            <option value="IsEmpty">Is Empty</option>
            <option value="IsNotEmpty">Is Not Empty</option>
          </select>
        </div>
      </div>

      {/* Right side: mode toggle + editor */}
      <div>
        <div className="flex items-center justify-between mb-0.5">
          <label className="text-[10px] text-[var(--text-muted)]">Compare To</label>
          <div className="flex rounded bg-[var(--bg-input)] overflow-hidden">
            <button
              type="button"
              onClick={() => {
                setRightMode("literal");
                if (cond.right.type !== "Literal") {
                  emit({
                    ...cond,
                    right: { type: "Literal", value: { type: "String", value: "" } },
                  });
                }
              }}
              className={`px-2 py-0.5 text-[10px] transition-colors ${
                rightMode === "literal"
                  ? "bg-[var(--accent-coral)] text-white"
                  : "text-[var(--text-muted)] hover:text-[var(--text-primary)]"
              }`}
            >
              Literal
            </button>
            <button
              type="button"
              onClick={() => {
                setRightMode("variable");
                if (cond.right.type !== "Ref") {
                  emit({
                    ...cond,
                    right: { type: "Ref", node: "", field: "" },
                  });
                }
              }}
              className={`px-2 py-0.5 text-[10px] transition-colors ${
                rightMode === "variable"
                  ? "bg-[var(--accent-coral)] text-white"
                  : "text-[var(--text-muted)] hover:text-[var(--text-primary)]"
              }`}
            >
              Variable
            </button>
          </div>
        </div>
        {rightMode === "literal" ? (
          <input
            type="text"
            value={literalDisplayValue(cond.right)}
            onChange={(e) => {
              const raw = e.target.value;
              let value: LiteralValue;
              if (raw === "true" || raw === "false") {
                value = { type: "Bool", value: raw === "true" };
              } else if (!isNaN(Number(raw)) && raw !== "") {
                value = { type: "Number", value: Number(raw) };
              } else {
                value = { type: "String", value: raw };
              }
              emit({ ...cond, right: { type: "Literal", value } });
            }}
            placeholder="value"
            className="w-full rounded bg-[var(--bg-input)] px-2 py-1 text-xs text-[var(--text-primary)] outline-none"
          />
        ) : (
          <OutputRefPicker
            nodeAutoId={rightNode}
            fieldName={rightField}
            onChangeNode={(autoId) => {
              const opt = nodeOptions.find((n) => n.autoId === autoId);
              const fields = opt ? getFullOutputSchema(opt.nodeType) : [];
              const field = fields.length > 0 ? fields[0].name : "";
              emit({ ...cond, right: { type: "Ref", node: autoId, field } });
            }}
            onChangeField={(field) => {
              emit({ ...cond, right: { type: "Ref", node: rightNode, field } });
            }}
            nodeOptions={nodeOptions}
          />
        )}
      </div>
    </div>
  );
}
