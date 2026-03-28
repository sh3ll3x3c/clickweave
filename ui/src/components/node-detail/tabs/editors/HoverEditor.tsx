import { FieldGroup, NumberField, SelectField, TextField } from "../../fields";
import { APP_KIND_LABELS, type NodeEditorProps, usesCdp } from "./types";
import { useNodeTypeUpdater } from "./useNodeTypeUpdater";

const TARGET_TYPES = ["Text", "Coordinates", "WindowControl"] as const;
const TARGET_LABELS: Record<string, string> = {
  Text: "Text",
  Coordinates: "Coordinates",
  WindowControl: "Window Control",
};

const WINDOW_CONTROL_ACTIONS = ["Close", "Minimize", "Maximize", "Zoom"] as const;

export function HoverEditor({ nodeType, onUpdate, appKind }: NodeEditorProps) {
  const nt = nodeType;
  if (nt.type !== "Hover") return null;

  const updateType = useNodeTypeUpdater(nt, onUpdate);

  const isCdp = appKind && usesCdp(appKind);
  const targetType = nt.target?.type ?? "Text";

  return (
    <FieldGroup title="Hover">
      <SelectField
        label="Target Type"
        value={targetType}
        options={[...TARGET_TYPES]}
        labels={TARGET_LABELS}
        onChange={(v) => {
          if (v === "Text") updateType({ target: { type: "Text" as const, text: "" } });
          else if (v === "Coordinates") updateType({ target: { type: "Coordinates" as const, x: 0, y: 0 } });
          else if (v === "WindowControl") updateType({ target: { type: "WindowControl" as const, action: "Close" } });
        }}
      />
      {targetType === "Text" && (
        <TextField
          label="Target Text"
          value={nt.target?.type === "Text" ? nt.target.text : ""}
          onChange={(v) => updateType({ target: v ? { type: "Text" as const, text: v } : null })}
          placeholder="Text to find and hover"
        />
      )}
      {targetType === "Coordinates" && (
        <>
          <NumberField
            label="X"
            value={nt.target?.type === "Coordinates" ? nt.target.x : 0}
            onChange={(v) => updateType({ target: { type: "Coordinates" as const, x: v ?? 0, y: nt.target?.type === "Coordinates" ? nt.target.y : 0 } })}
          />
          <NumberField
            label="Y"
            value={nt.target?.type === "Coordinates" ? nt.target.y : 0}
            onChange={(v) => updateType({ target: { type: "Coordinates" as const, x: nt.target?.type === "Coordinates" ? nt.target.x : 0, y: v ?? 0 } })}
          />
        </>
      )}
      {targetType === "WindowControl" && (
        <SelectField
          label="Action"
          value={nt.target?.type === "WindowControl" ? nt.target.action : "Close"}
          options={[...WINDOW_CONTROL_ACTIONS]}
          onChange={(v) => updateType({ target: { type: "WindowControl" as const, action: v as typeof WINDOW_CONTROL_ACTIONS[number] } })}
        />
      )}
      <NumberField
        label="Dwell (ms)"
        value={nt.dwell_ms}
        min={0}
        max={10000}
        onChange={(v) => updateType({ dwell_ms: v ?? 500 })}
      />
      {isCdp && (
        <div>
          <label className="mb-1 block text-xs text-[var(--text-secondary)]">
            Automation
          </label>
          <span className="block px-2.5 py-1.5 text-xs text-[var(--accent-coral)]">
            {APP_KIND_LABELS[appKind]}
          </span>
        </div>
      )}
    </FieldGroup>
  );
}
