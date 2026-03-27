import { typeColor } from "../../utils/typeColors";

interface InputsSectionProps {
  nodeType: Record<string, unknown>;
}

const REF_FIELD_LABELS: Record<string, string> = {
  target_ref: "Target coordinates",
  text_ref: "Text value",
  value_ref: "Value",
  url_ref: "URL",
  from_ref: "Start coordinates",
  to_ref: "End coordinates",
  prompt_ref: "Prompt data",
};

export function InputsSection({ nodeType }: InputsSectionProps) {
  // Extract any _ref fields that are set
  const inner = Object.values(nodeType)[0] as Record<string, unknown> | undefined;
  if (!inner) return null;

  const refs = Object.entries(inner)
    .filter(([key, val]) => key.endsWith("_ref") && val != null)
    .map(([key, val]) => {
      const ref = val as { node: string; field: string };
      return { key, label: REF_FIELD_LABELS[key] || key, ref };
    });

  if (refs.length === 0) return null;

  return (
    <div className="mt-3">
      <h4 className="text-xs font-medium text-[var(--text-muted)] mb-1.5">Inputs</h4>
      <div className="space-y-1">
        {refs.map(({ key, label, ref }) => (
          <div key={key} className="flex items-center gap-2 text-xs">
            <span
              className="w-2 h-2 rounded-full flex-shrink-0"
              style={{ backgroundColor: typeColor("Object") }}
            />
            <span className="text-[var(--text-muted)]">{label}:</span>
            <span className="font-mono text-[var(--text-primary)]">
              {ref.node}.{ref.field}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
