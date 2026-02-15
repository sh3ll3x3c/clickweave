import type { NodeType } from "../../../../bindings";
import { FieldGroup, NumberField, SelectField, TextField } from "../../fields";
import { type NodeEditorProps, optionalString } from "./types";

export function ClickEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "Click") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

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
}
