import type { Node, NodeType } from "../../../bindings";
import {
  CheckboxField,
  FieldGroup,
  ImagePathField,
  NumberField,
  SelectField,
  TextAreaField,
  TextField,
} from "../fields";

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
  }
}
