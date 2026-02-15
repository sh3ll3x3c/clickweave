import type { NodeType } from "../../../../bindings";
import { FieldGroup, TextAreaField } from "../../fields";
import type { NodeEditorProps } from "./types";

export function TypeTextEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "TypeText") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  return (
    <FieldGroup title="Type Text">
      <TextAreaField
        label="Text"
        value={nt.text}
        onChange={(v) => updateType({ text: v })}
      />
    </FieldGroup>
  );
}
