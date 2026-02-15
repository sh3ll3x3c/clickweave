import type { Node } from "../../../bindings";
import {
  CheckboxField,
  FieldGroup,
  NumberField,
  SelectField,
  TextField,
} from "../fields";
import { editorRegistry } from "./editors";

export function SetupTab({
  node,
  onUpdate,
  projectPath,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
  projectPath: string | null;
}) {
  const Editor = editorRegistry[node.node_type.type];

  return (
    <div className="space-y-4">
      <FieldGroup title="General">
        <TextField
          label="Name"
          value={node.name}
          onChange={(name) => onUpdate({ name })}
        />
        <CheckboxField
          label="Enabled"
          value={node.enabled}
          onChange={(enabled) => onUpdate({ enabled })}
        />
        <NumberField
          label="Timeout (ms)"
          value={node.timeout_ms ?? 0}
          onChange={(v) => onUpdate({ timeout_ms: v === 0 ? null : v })}
        />
        <NumberField
          label="Settle (ms)"
          value={node.settle_ms ?? 0}
          onChange={(v) => onUpdate({ settle_ms: v === 0 ? null : v })}
        />
        <NumberField
          label="Retries"
          value={node.retries}
          min={0}
          max={10}
          onChange={(retries) => onUpdate({ retries })}
        />
        <SelectField
          label="Trace Level"
          value={node.trace_level}
          options={["Off", "Minimal", "Full"]}
          onChange={(trace_level) =>
            onUpdate({ trace_level: trace_level as Node["trace_level"] })
          }
        />
        <TextField
          label="Expected Outcome"
          value={node.expected_outcome ?? ""}
          onChange={(v) =>
            onUpdate({ expected_outcome: v === "" ? null : v })
          }
          placeholder="Optional"
        />
      </FieldGroup>

      {Editor ? (
        <Editor
          nodeType={node.node_type}
          onUpdate={onUpdate}
          projectPath={projectPath}
        />
      ) : (
        <p className="text-xs text-[var(--text-muted)]">Unknown node type</p>
      )}
    </div>
  );
}
