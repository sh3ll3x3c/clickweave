import type { NodeTypeInfo, NodeType } from "../bindings";

interface NodePaletteProps {
  nodeTypes: NodeTypeInfo[];
  search: string;
  onSearchChange: (s: string) => void;
  onAdd: (nodeType: NodeType) => void;
}

const categoryColors: Record<string, string> = {
  AI: "var(--node-ai)",
  "Vision / Discovery": "var(--node-vision)",
  Input: "var(--node-input)",
  Window: "var(--node-window)",
  AppDebugKit: "var(--node-debugkit)",
  "Control Flow": "#10b981",
};

export function NodePalette({
  nodeTypes,
  search,
  onSearchChange,
  onAdd,
}: NodePaletteProps) {
  const searchLower = search.toLowerCase();
  const filtered = nodeTypes.filter(
    (nt) =>
      nt.name.toLowerCase().includes(searchLower) ||
      nt.category.toLowerCase().includes(searchLower),
  );

  const grouped = filtered.reduce(
    (acc, nt) => {
      (acc[nt.category] ||= []).push(nt);
      return acc;
    },
    {} as Record<string, NodeTypeInfo[]>,
  );

  return (
    <div className="flex w-56 flex-col border-l border-[var(--border)] bg-[var(--bg-panel)]">
      <div className="border-b border-[var(--border)] px-3 py-2.5">
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
          Add Node
        </h3>
        <input
          type="text"
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder="Search nodes..."
          className="w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] placeholder-[var(--text-muted)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]"
        />
      </div>

      <div className="flex-1 overflow-y-auto p-2">
        {Object.entries(grouped).map(([category, types]) => (
          <div key={category} className="mb-3">
            <h4
              className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider"
              style={{ color: categoryColors[category] || "var(--text-muted)" }}
            >
              {category}
            </h4>
            <div className="flex flex-col gap-0.5">
              {types.map((nt) => (
                <button
                  key={nt.name}
                  onClick={() => onAdd(nt.node_type)}
                  className="flex items-center gap-2 rounded px-2 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] transition-colors"
                >
                  <span>{nt.icon}</span>
                  <span>{nt.name}</span>
                </button>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
