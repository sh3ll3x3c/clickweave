import type { NodeType } from "../../../../bindings";
import { FieldGroup, TextAreaField, TextField } from "../../fields";
import type { NodeEditorProps } from "./types";

export function McpToolCallEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "McpToolCall") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

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
}
