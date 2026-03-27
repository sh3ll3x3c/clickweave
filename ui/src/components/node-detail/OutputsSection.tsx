import { typeColor } from "../../utils/typeColors";

// A simple static lookup matching the Rust output_schema() registry
const OUTPUT_SCHEMAS: Record<string, Array<{ name: string; type: string; description: string }>> = {
  FindText: [
    { name: "found", type: "Bool", description: "Whether any matches were found" },
    { name: "count", type: "Number", description: "Number of matches found" },
    { name: "text", type: "String", description: "Text of the first match" },
    { name: "coordinates", type: "Object", description: "Coordinates of the first match" },
  ],
  FindImage: [
    { name: "found", type: "Bool", description: "Whether any matches were found" },
    { name: "count", type: "Number", description: "Number of matches found" },
    { name: "coordinates", type: "Object", description: "Coordinates of the first match" },
    { name: "confidence", type: "Number", description: "Confidence score" },
  ],
  FindApp: [
    { name: "found", type: "Bool", description: "Whether the app is running" },
    { name: "name", type: "String", description: "App name" },
    { name: "pid", type: "Number", description: "Process ID" },
  ],
  TakeScreenshot: [{ name: "result", type: "String", description: "Screenshot data" }],
  CdpWait: [{ name: "found", type: "Bool", description: "Whether text appeared" }],
  AiStep: [{ name: "result", type: "String", description: "LLM response text" }],
  McpToolCall: [{ name: "result", type: "Any", description: "Raw tool result" }],
  AppDebugKitOp: [{ name: "result", type: "Any", description: "Raw tool result" }],
};

interface OutputsSectionProps {
  nodeTypeName: string;
  autoId?: string;
}

export function OutputsSection({ nodeTypeName, autoId }: OutputsSectionProps) {
  const fields = OUTPUT_SCHEMAS[nodeTypeName];
  if (!fields || fields.length === 0) return null;

  return (
    <div className="mt-3">
      <h4 className="text-xs font-medium text-[var(--text-muted)] mb-1.5">Outputs</h4>
      <div className="space-y-1">
        {fields.map((field) => (
          <div key={field.name} className="flex items-center gap-2 text-xs">
            <span
              className="w-2 h-2 rounded-full flex-shrink-0"
              style={{ backgroundColor: typeColor(field.type) }}
            />
            <span className="font-mono text-[var(--text-primary)]">
              {autoId ? `${autoId}.${field.name}` : field.name}
            </span>
            <span className="text-[var(--text-muted)]">{field.type}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
