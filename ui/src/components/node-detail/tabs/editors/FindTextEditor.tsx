import { FieldGroup, TextField } from "../../fields";
import { type NodeEditorProps, optionalString } from "./types";
import { useNodeTypeUpdater } from "./useNodeTypeUpdater";

export function FindTextEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "FindText") return null;

  const updateType = useNodeTypeUpdater(nt, onUpdate);

  return (
    <FieldGroup title="Find Text">
      <TextField
        label="Search Text"
        value={nt.search_text}
        onChange={(v) => updateType({ search_text: v })}
      />
      <TextField
        label="Scope"
        value={nt.scope ?? ""}
        onChange={(v) => updateType({ scope: optionalString(v) })}
        placeholder="Optional"
      />
    </FieldGroup>
  );
}
