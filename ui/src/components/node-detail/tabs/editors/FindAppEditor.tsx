import { FieldGroup, TextField } from "../../fields";
import type { NodeEditorProps } from "./types";
import { useNodeTypeUpdater } from "./useNodeTypeUpdater";

export function FindAppEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "FindApp") return null;

  const updateType = useNodeTypeUpdater(nt, onUpdate);

  return (
    <FieldGroup title="Find App">
      <TextField
        label="Search"
        value={nt.search}
        onChange={(v) => updateType({ search: v })}
        placeholder="App name to search for"
      />
    </FieldGroup>
  );
}
