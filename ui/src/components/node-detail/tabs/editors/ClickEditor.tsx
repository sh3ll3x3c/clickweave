import type { NodeType } from "../../../../bindings";
import { FieldGroup, ImagePathField, NumberField, SelectField, TextField } from "../../fields";
import { type NodeEditorProps, optionalString } from "./types";

export function ClickEditor({ nodeType, onUpdate, projectPath }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "Click") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  const hasImage = !!nt.template_image;

  return (
    <FieldGroup title="Click">
      <TextField
        label="Target"
        value={nt.target ?? ""}
        onChange={(v) => updateType({ target: optionalString(v) })}
        placeholder="Text to find and click (auto-resolves coordinates)"
      />
      <ImagePathField
        label="Template Image"
        value={nt.template_image ?? ""}
        projectPath={projectPath}
        onChange={(v) => updateType({ template_image: optionalString(v) })}
      />
      {hasImage && (
        <p className="text-[10px] text-[var(--text-muted)]">
          At runtime this node uses <strong>find_image</strong> to locate the template and click at the matched coordinates.
        </p>
      )}
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
