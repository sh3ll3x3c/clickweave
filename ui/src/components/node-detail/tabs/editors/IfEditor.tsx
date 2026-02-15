import type { NodeType } from "../../../../bindings";
import { FieldGroup } from "../../fields";
import { ConditionEditor } from "./ConditionEditor";
import type { NodeEditorProps } from "./types";

export function IfEditor({ nodeType, onUpdate }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "If") return null;

  const updateType = (patch: Record<string, unknown>) => {
    onUpdate({ node_type: { ...nt, ...patch } as NodeType });
  };

  return (
    <FieldGroup title="If Condition">
      <ConditionEditor
        condition={nt.condition}
        onChange={(condition) => updateType({ condition })}
      />
    </FieldGroup>
  );
}
