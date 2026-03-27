import { typeColor } from "../../utils/typeColors";
import { getFullOutputSchema, OUTPUT_SCHEMAS } from "../../utils/outputSchema";

interface OutputsSectionProps {
  nodeTypeName: string;
  /** The full node_type object, used to derive verification-aware output schema. */
  nodeType?: Record<string, unknown>;
  autoId?: string;
  /** Map of fieldName -> array of consumer auto_ids that reference this field. */
  consumers?: Record<string, string[]>;
  /** Whether this node is an action node (no outputs schema). */
  isActionNode?: boolean;
  /** Callback when user clicks "Enable Verification" for an action node. */
  onEnableVerification?: () => void;
}

export function OutputsSection({
  nodeTypeName,
  nodeType,
  autoId,
  consumers,
  isActionNode,
  onEnableVerification,
}: OutputsSectionProps) {
  const fields = nodeType ? getFullOutputSchema(nodeType) : (OUTPUT_SCHEMAS[nodeTypeName] ?? []);

  // Action node with no output schema
  if (isActionNode && (!fields || fields.length === 0)) {
    return (
      <div className="mt-3">
        <h4 className="text-xs font-medium text-[var(--text-muted)] mb-1.5">Outputs</h4>
        <p className="text-xs text-[var(--text-muted)] italic">
          No outputs — enable verification to check action effect
        </p>
        {onEnableVerification && (
          <button
            onClick={onEnableVerification}
            className="mt-1.5 text-xs text-[var(--accent-coral)] hover:underline"
          >
            Enable Verification
          </button>
        )}
      </div>
    );
  }

  // Non-action node with no outputs
  if (!fields || fields.length === 0) {
    return (
      <div className="mt-3">
        <h4 className="text-xs font-medium text-[var(--text-muted)] mb-1.5">Outputs</h4>
        <p className="text-xs text-[var(--text-muted)] italic">No outputs</p>
      </div>
    );
  }

  return (
    <div className="mt-3">
      <h4 className="text-xs font-medium text-[var(--text-muted)] mb-1.5">Outputs</h4>
      <div className="space-y-1.5">
        {fields.map((field) => {
          const fieldConsumers = consumers?.[field.name];
          return (
            <div key={field.name}>
              <div className="flex items-center gap-2 text-xs">
                <span
                  className="w-2 h-2 rounded-full flex-shrink-0"
                  style={{ backgroundColor: typeColor(field.type) }}
                />
                <span className="font-mono text-[var(--text-primary)]">
                  {autoId ? `${autoId}.${field.name}` : field.name}
                </span>
                <span className="text-[var(--text-muted)]">{field.type}</span>
                {fieldConsumers && fieldConsumers.length > 0 && (
                  <span className="text-[var(--text-muted)]">
                    {fieldConsumers.map((c) => `→ ${c}`).join(", ")}
                  </span>
                )}
              </div>
              {field.description && (
                <p className="text-[10px] text-[var(--text-muted)] ml-4 mt-0.5">
                  {field.description}
                </p>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
