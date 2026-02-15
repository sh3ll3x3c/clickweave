import type { Condition, LiteralValue, Operator } from "../../../../bindings";

export function ConditionEditor({
  condition,
  onChange,
}: {
  condition: Condition;
  onChange: (c: Condition) => void;
}) {
  return (
    <div className="space-y-2">
      <div className="flex gap-2">
        <div className="flex-1">
          <label className="text-[10px] text-[var(--text-muted)]">Variable</label>
          <input
            type="text"
            value={condition.left.type === "Variable" ? condition.left.name : ""}
            onChange={(e) =>
              onChange({
                ...condition,
                left: { type: "Variable", name: e.target.value },
              })
            }
            placeholder="e.g. find_text_1.found"
            className="w-full rounded bg-[var(--bg-input)] px-2 py-1 text-xs text-[var(--text-primary)] outline-none"
          />
        </div>
      </div>
      <div className="flex gap-2">
        <div className="flex-1">
          <label className="text-[10px] text-[var(--text-muted)]">Operator</label>
          <select
            value={condition.operator}
            onChange={(e) =>
              onChange({ ...condition, operator: e.target.value as Operator })
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
        <div className="flex-1">
          <label className="text-[10px] text-[var(--text-muted)]">Value</label>
          <input
            type="text"
            value={
              condition.right.type === "Literal"
                ? String(
                    condition.right.value.type === "String"
                      ? condition.right.value.value
                      : condition.right.value.type === "Number"
                        ? condition.right.value.value
                        : condition.right.value.type === "Bool"
                          ? condition.right.value.value
                          : "",
                  )
                : ""
            }
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
              onChange({
                ...condition,
                right: { type: "Literal", value },
              });
            }}
            placeholder="value"
            className="w-full rounded bg-[var(--bg-input)] px-2 py-1 text-xs text-[var(--text-primary)] outline-none"
          />
        </div>
      </div>
    </div>
  );
}
