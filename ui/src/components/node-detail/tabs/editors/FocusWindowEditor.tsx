import type { NodeType } from "../../../../bindings";
import { CheckboxField, FieldGroup, SelectField, TextField } from "../../fields";
import { type NodeEditorProps, optionalString } from "./types";

export function FocusWindowEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "FocusWindow") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

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
}
