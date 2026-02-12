import type { Check, CheckType, Node } from "../../../bindings";

export function ChecksTab({
  node,
  onUpdate,
}: {
  node: Node;
  onUpdate: (u: Partial<Node>) => void;
}) {
  const checks = node.checks;

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
      {checks.map((check, i) => (
        <div
          key={i}
          className="rounded border border-[var(--border)] bg-[var(--bg-input)] p-3"
        >
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-[var(--text-primary)]">
              {check.name} ({check.check_type})
            </span>
            <button
              onClick={() => removeCheck(i)}
              className="text-xs text-red-400 hover:text-red-300"
            >
              Delete
            </button>
          </div>
          <div className="mt-1 text-[10px] text-[var(--text-muted)]">
            On fail: {check.on_fail}
          </div>
        </div>
      ))}

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
