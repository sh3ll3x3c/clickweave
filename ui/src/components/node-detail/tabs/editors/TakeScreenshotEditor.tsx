import type { NodeType } from "../../../../bindings";
import { CheckboxField, FieldGroup, SelectField, TextField } from "../../fields";
import type { NodeEditorProps } from "./types";

export function TakeScreenshotEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "TakeScreenshot") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  const optionalString = (v: string) => (v === "" ? null : v);

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
}
