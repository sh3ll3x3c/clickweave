import type { Check, CheckType, JsonValue, OnCheckFail, Node } from "../../../bindings";

/** The param key and label for each check type. */
const CHECK_PARAM_CONFIG: Record<CheckType, { key: string; label: string }> = {
  TextPresent: { key: "text", label: "Text" },
  TextAbsent: { key: "text", label: "Text" },
  TemplateFound: { key: "template", label: "Template path" },
  WindowTitleMatches: { key: "title", label: "Window title" },
};

function getParamValue(params: JsonValue, key: string): string {
  if (params && typeof params === "object" && !Array.isArray(params)) {
    const val = (params as Record<string, JsonValue>)[key];
    return typeof val === "string" ? val : "";
  }
  return "";
}

function setParamValue(params: JsonValue, key: string, value: string): JsonValue {
  const obj = params && typeof params === "object" && !Array.isArray(params)
    ? { ...(params as Record<string, JsonValue>) }
    : {};
  obj[key] = value;
  return obj;
}

export function ChecksTab({
  node,
  onUpdate,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
}) {
  const checks = node.checks;

  const updateCheck = (index: number, patch: Partial<Check>) => {
    const updated = checks.map((c, i) =>
      i === index ? { ...c, ...patch } : c,
    );
    onUpdate({ checks: updated });
  };

  const addCheck = (checkType: CheckType) => {
    const newCheck: Check = {
      name: `Check ${checks.length + 1}`,
      check_type: checkType,
      params: {},
      on_fail: "FailNode",
    };
    onUpdate({ checks: [...checks, newCheck] });
  };

  const removeCheck = (index: number) => {
    onUpdate({ checks: checks.filter((_, i) => i !== index) });
  };

  return (
    <div className="space-y-4">
      {checks.map((check, i) => {
        const paramConfig = CHECK_PARAM_CONFIG[check.check_type];
        return (
          <div
            key={i}
            className="rounded border border-[var(--border)] bg-[var(--bg-input)] p-3 space-y-2"
          >
            {/* Header: name input + delete */}
            <div className="flex items-center gap-2">
              <input
                type="text"
                value={check.name}
                onChange={(e) => updateCheck(i, { name: e.target.value })}
                className="flex-1 rounded bg-[var(--bg-dark)] px-2 py-1 text-xs text-[var(--text-primary)] border border-[var(--border)] focus:border-[var(--accent-blue)] focus:outline-none"
              />
              <button
                onClick={() => removeCheck(i)}
                className="text-xs text-red-400 hover:text-red-300 shrink-0"
              >
                Delete
              </button>
            </div>

            {/* Type label */}
            <div className="text-[10px] text-[var(--text-muted)]">
              Type: {check.check_type}
            </div>

            {/* Param input */}
            {paramConfig && (
              <label className="flex flex-col gap-1">
                <span className="text-[10px] text-[var(--text-muted)]">{paramConfig.label}</span>
                <input
                  type="text"
                  value={getParamValue(check.params, paramConfig.key)}
                  onChange={(e) =>
                    updateCheck(i, {
                      params: setParamValue(check.params, paramConfig.key, e.target.value),
                    })
                  }
                  placeholder={paramConfig.label}
                  className="rounded bg-[var(--bg-dark)] px-2 py-1 text-xs text-[var(--text-primary)] border border-[var(--border)] focus:border-[var(--accent-blue)] focus:outline-none"
                />
              </label>
            )}

            {/* On fail selector */}
            <label className="flex items-center gap-2">
              <span className="text-[10px] text-[var(--text-muted)]">On fail:</span>
              <select
                value={check.on_fail}
                onChange={(e) =>
                  updateCheck(i, { on_fail: e.target.value as OnCheckFail })
                }
                className="rounded bg-[var(--bg-dark)] px-2 py-1 text-xs text-[var(--text-primary)] border border-[var(--border)] focus:outline-none"
              >
                <option value="FailNode">FailNode</option>
                <option value="WarnOnly">WarnOnly</option>
              </select>
            </label>
          </div>
        );
      })}

      <div>
        <h4 className="mb-2 text-xs font-semibold text-[var(--text-muted)]">
          Add Check
        </h4>
        <div className="flex flex-wrap gap-1">
          {(
            [
              "TextPresent",
              "TextAbsent",
              "TemplateFound",
              "WindowTitleMatches",
            ] as CheckType[]
          ).map((ct) => (
            <button
              key={ct}
              onClick={() => addCheck(ct)}
              className="rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            >
              + {ct}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
