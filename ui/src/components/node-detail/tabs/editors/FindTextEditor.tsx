import type { NodeType } from "../../../../bindings";
import { FieldGroup, SelectField, TextField } from "../../fields";
import { type NodeEditorProps, optionalString } from "./types";

export function FindTextEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "FindText") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

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
}
