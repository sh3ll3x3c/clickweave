import type {
  Node,
  NodeType,
  Condition,
  LiteralValue,
  Operator,
  SwitchCase,
} from "../../../bindings";
import {
  CheckboxField,
  FieldGroup,
  ImagePathField,
  NumberField,
  SelectField,
  TextAreaField,
  TextField,
} from "../fields";

function ConditionEditor({
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

export function SetupTab({
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
          label="Settle (ms)"
          value={node.settle_ms ?? 0}
          onChange={(v) => onUpdate({ settle_ms: v === 0 ? null : v })}
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

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const updateType = (patch: Record<string, any>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  const optionalString = (v: string) => (v === "" ? null : v);

  switch (nt.type) {
    case "AiStep":
      return (
        <FieldGroup title="AI Step">
          <TextAreaField
            label="Prompt"
            value={nt.prompt}
            onChange={(prompt) => updateType({ prompt })}
          />
          <TextField
            label="Button Text"
            value={nt.button_text ?? ""}
            onChange={(v) => updateType({ button_text: optionalString(v) })}
            placeholder="Optional"
          />
          <ImagePathField
            label="Template Image"
            value={nt.template_image ?? ""}
            projectPath={projectPath}
            onChange={(v) => updateType({ template_image: optionalString(v) })}
          />
          <NumberField
            label="Max Tool Calls"
            value={nt.max_tool_calls ?? 10}
            min={1}
            max={100}
            onChange={(v) => updateType({ max_tool_calls: v })}
          />
          <TextField
            label="Allowed Tools"
            value={nt.allowed_tools?.join(", ") ?? ""}
            onChange={(v) =>
              updateType({
                allowed_tools: v === "" ? null : v.split(",").map((s) => s.trim()),
              })
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
            onChange={(v) => updateType({ mode: v })}
          />
          <TextField
            label="Target"
            value={nt.target ?? ""}
            onChange={(v) => updateType({ target: optionalString(v) })}
            placeholder="App name or window ID"
          />
          <CheckboxField
            label="Include OCR"
            value={nt.include_ocr}
            onChange={(v) => updateType({ include_ocr: v })}
          />
        </FieldGroup>
      );

    case "FindText":
      return (
        <FieldGroup title="Find Text">
          <TextField
            label="Search Text"
            value={nt.search_text}
            onChange={(v) => updateType({ search_text: v })}
          />
          <SelectField
            label="Match Mode"
            value={nt.match_mode}
            options={["Contains", "Exact"]}
            onChange={(v) => updateType({ match_mode: v })}
          />
          <TextField
            label="Scope"
            value={nt.scope ?? ""}
            onChange={(v) => updateType({ scope: optionalString(v) })}
            placeholder="Optional"
          />
          <TextField
            label="Select Result"
            value={nt.select_result ?? ""}
            onChange={(v) => updateType({ select_result: optionalString(v) })}
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
            onChange={(v) => updateType({ template_image: optionalString(v) })}
          />
          <NumberField
            label="Threshold"
            value={nt.threshold}
            min={0}
            max={1}
            step={0.01}
            onChange={(v) => updateType({ threshold: v })}
          />
          <NumberField
            label="Max Results"
            value={nt.max_results}
            min={1}
            max={20}
            onChange={(v) => updateType({ max_results: v })}
          />
        </FieldGroup>
      );

    case "Click":
      return (
        <FieldGroup title="Click">
          <TextField
            label="Target"
            value={nt.target ?? ""}
            onChange={(v) => updateType({ target: optionalString(v) })}
            placeholder="Text to find and click (auto-resolves coordinates)"
          />
          <NumberField
            label="X"
            value={nt.x ?? 0}
            onChange={(v) => updateType({ x: v ?? null })}
          />
          <NumberField
            label="Y"
            value={nt.y ?? 0}
            onChange={(v) => updateType({ y: v ?? null })}
          />
          <SelectField
            label="Button"
            value={nt.button}
            options={["Left", "Right", "Center"]}
            onChange={(v) => updateType({ button: v })}
          />
          <NumberField
            label="Click Count"
            value={nt.click_count}
            min={1}
            max={3}
            onChange={(v) => updateType({ click_count: v })}
          />
        </FieldGroup>
      );

    case "TypeText":
      return (
        <FieldGroup title="Type Text">
          <TextAreaField
            label="Text"
            value={nt.text}
            onChange={(v) => updateType({ text: v })}
          />
        </FieldGroup>
      );

    case "PressKey":
      return (
        <FieldGroup title="Press Key">
          <TextField
            label="Key"
            value={nt.key}
            onChange={(v) => updateType({ key: v })}
            placeholder="e.g. return, tab, escape, a"
          />
          <TextField
            label="Modifiers"
            value={nt.modifiers.join(", ")}
            onChange={(v) =>
              updateType({
                modifiers: v ? v.split(",").map((s: string) => s.trim()).filter(Boolean) : [],
              })
            }
            placeholder="e.g. command, shift, control, option"
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
            onChange={(v) => updateType({ delta_y: v })}
          />
          <NumberField
            label="X Position"
            value={nt.x ?? 0}
            onChange={(v) => updateType({ x: v === 0 ? null : v })}
          />
          <NumberField
            label="Y Position"
            value={nt.y ?? 0}
            onChange={(v) => updateType({ y: v === 0 ? null : v })}
          />
        </FieldGroup>
      );

    case "ListWindows":
      return (
        <FieldGroup title="List Windows">
          <TextField
            label="App Name Filter"
            value={nt.app_name ?? ""}
            onChange={(v) => updateType({ app_name: optionalString(v) })}
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
            options={["WindowId", "AppName", "Pid"]}
            onChange={(v) => updateType({ method: v })}
          />
          <TextField
            label={
              { WindowId: "Window ID", AppName: "App Name", Pid: "Process ID" }[nt.method] ?? nt.method
            }
            value={nt.value ?? ""}
            onChange={(v) => updateType({ value: optionalString(v) })}
          />
          <CheckboxField
            label="Bring to Front"
            value={nt.bring_to_front}
            onChange={(v) => updateType({ bring_to_front: v })}
          />
        </FieldGroup>
      );

    case "McpToolCall":
      return (
        <FieldGroup title="MCP Tool Call">
          <TextField
            label="Tool Name"
            value={nt.tool_name}
            onChange={(v) => updateType({ tool_name: v })}
          />
          <TextAreaField
            label="Arguments (JSON)"
            value={
              typeof nt.arguments === "string"
                ? nt.arguments
                : JSON.stringify(nt.arguments ?? {}, null, 2)
            }
            onChange={(v) => {
              try {
                updateType({ arguments: JSON.parse(v) });
              } catch {
                // keep raw string while user is editing
              }
            }}
          />
        </FieldGroup>
      );

    case "AppDebugKitOp":
      return (
        <FieldGroup title="AppDebugKit">
          <TextField
            label="Operation Name"
            value={nt.operation_name}
            onChange={(v) => updateType({ operation_name: v })}
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
                updateType({ parameters: JSON.parse(v) });
              } catch {
                // Keep raw text during editing
              }
            }}
          />
        </FieldGroup>
      );

    case "If":
      return (
        <FieldGroup title="If Condition">
          <ConditionEditor
            condition={nt.condition}
            onChange={(condition) => updateType({ condition })}
          />
        </FieldGroup>
      );

    case "Switch":
      return (
        <FieldGroup title="Switch Cases">
          {nt.cases.map((c: SwitchCase, i: number) => (
            <div
              key={i}
              className="mb-3 border-b border-[var(--border)] pb-3"
            >
              <div className="mb-1 flex items-center justify-between">
                <TextField
                  label={`Case ${i + 1} Name`}
                  value={c.name}
                  onChange={(name) => {
                    const cases = [...nt.cases];
                    cases[i] = { ...cases[i], name };
                    updateType({ cases });
                  }}
                />
                <button
                  onClick={() => {
                    const cases = nt.cases.filter(
                      (_: SwitchCase, j: number) => j !== i,
                    );
                    updateType({ cases });
                  }}
                  className="ml-2 text-xs text-red-400 hover:text-red-300"
                >
                  Remove
                </button>
              </div>
              <ConditionEditor
                condition={c.condition}
                onChange={(condition) => {
                  const cases = [...nt.cases];
                  cases[i] = { ...cases[i], condition };
                  updateType({ cases });
                }}
              />
            </div>
          ))}
          <button
            onClick={() => {
              const newCase: SwitchCase = {
                name: `Case ${nt.cases.length + 1}`,
                condition: {
                  left: { type: "Variable" as const, name: "" },
                  operator: "Equals" as Operator,
                  right: {
                    type: "Literal" as const,
                    value: { type: "Bool" as const, value: true },
                  },
                },
              };
              updateType({ cases: [...nt.cases, newCase] });
            }}
            className="text-xs text-[var(--accent-coral)] hover:underline"
          >
            + Add Case
          </button>
        </FieldGroup>
      );

    case "Loop":
      return (
        <FieldGroup title="Loop">
          <p className="mb-2 text-[10px] text-[var(--text-muted)]">
            Loop body runs at least once (do-while). Exit condition checked from
            iteration 2 onward.
          </p>
          <ConditionEditor
            condition={nt.exit_condition}
            onChange={(exit_condition) => updateType({ exit_condition })}
          />
          <NumberField
            label="Max Iterations"
            value={nt.max_iterations}
            min={1}
            max={10000}
            onChange={(max_iterations) => updateType({ max_iterations })}
          />
        </FieldGroup>
      );

    case "EndLoop":
      return (
        <FieldGroup title="End Loop">
          <p className="text-xs text-[var(--text-muted)]">
            Paired with Loop node. This node jumps back to the loop to
            re-evaluate its exit condition.
          </p>
          <TextField
            label="Loop ID"
            value={nt.loop_id}
            onChange={(loop_id) => updateType({ loop_id })}
            placeholder="UUID of paired Loop node"
          />
        </FieldGroup>
      );
  }
}
