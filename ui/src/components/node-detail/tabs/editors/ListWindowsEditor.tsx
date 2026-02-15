import type { NodeType } from "../../../../bindings";
import { FieldGroup, TextField } from "../../fields";
import type { NodeEditorProps } from "./types";

export function ListWindowsEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "ListWindows") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  const optionalString = (v: string) => (v === "" ? null : v);

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
}
