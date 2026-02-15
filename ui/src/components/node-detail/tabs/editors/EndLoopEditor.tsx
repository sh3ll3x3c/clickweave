import type { NodeType } from "../../../../bindings";
import { FieldGroup, TextField } from "../../fields";
import type { NodeEditorProps } from "./types";

export function EndLoopEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "EndLoop") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  return (
    <FieldGroup title="End Loop">
      <p className="text-xs text-[var(--text-muted)]">
        Paired with Loop node. This node jumps back to the loop to
        re-evaluate its exit condition.
      </p>
      <TextField
        label="Loop ID"
        value={nt.loop_id}
        onChange={(loop_id) => updateType({ loop_id })}
        placeholder="UUID of paired Loop node"
      />
    </FieldGroup>
  );
}
