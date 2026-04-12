import { useState } from "react";
import type { NodeTypeInfo, NodeType } from "../bindings";
import { nodeMetadata, defaultNodeMetadata } from "../constants/nodeMetadata";

interface NodePaletteProps {
  nodeTypes: NodeTypeInfo[];
  search: string;
  collapsed: boolean;
  onSearchChange: (s: string) => void;
  onAdd: (nodeType: NodeType) => void;
  onToggle: () => void;
}

interface PaletteSubgroup {
  label: string;
  types: string[];
}

interface PaletteGroup {
  label: string;
  note?: string;
  types?: string[];
  subgroups?: PaletteSubgroup[];
}

const PALETTE_GROUPS: PaletteGroup[] = [
  {
    label: "Native",
    subgroups: [
      { label: "Query", types: ["FindText", "FindImage", "FindApp", "TakeScreenshot"] },
      { label: "Action", types: ["Click", "Hover", "Drag", "TypeText", "PressKey", "Scroll", "FocusWindow", "LaunchApp", "QuitApp"] },
    ],
  },
  {
    label: "CDP (Browser)",
    note: "Requires a Chrome or Electron app focused via FocusWindow or LaunchApp.",
    subgroups: [
      { label: "Query", types: ["CdpWait"] },
      { label: "Action", types: ["CdpClick", "CdpHover", "CdpFill", "CdpType", "CdpPressKey", "CdpNavigate", "CdpNewPage", "CdpClosePage", "CdpSelectPage", "CdpHandleDialog"] },
    ],
  },
  { label: "AI", types: ["AiStep"] },
  { label: "Generic", types: ["McpToolCall", "AppDebugKitOp"] },
];

function NodeItem({
  info,
  role,
  onAdd,
}: {
  info: NodeTypeInfo;
  role?: "Query" | "Action";
  onAdd: (nodeType: NodeType) => void;
}) {
  const meta = nodeMetadata[info.node_type.type] ?? defaultNodeMetadata;
  return (
    <button
      key={info.name}
      onClick={() => onAdd(info.node_type)}
      className="flex items-center gap-2 rounded px-2 py-1.5 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] transition-colors"
    >
      <span
        className="flex h-5 w-5 items-center justify-center rounded text-[8px] font-bold text-white flex-shrink-0"
        style={{ backgroundColor: meta.color }}
      >
        {meta.icon}
      </span>
      <span>{info.name}</span>
      {role && (
        <span className="ml-auto text-[9px] text-[var(--text-muted)] opacity-60 flex-shrink-0">
          {role}
        </span>
      )}
    </button>
  );
}

export function NodePalette({
  nodeTypes,
  search,
  collapsed,
  onSearchChange,
  onAdd,
  onToggle,
}: NodePaletteProps) {
  const searchLower = search.toLowerCase();
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());

  // Build a lookup from node_type.type to NodeTypeInfo
  const typeInfoMap = new Map<string, NodeTypeInfo>();
  for (const nt of nodeTypes) {
    typeInfoMap.set(nt.node_type.type, nt);
  }

  const toggleGroup = (label: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(label)) {
        next.delete(label);
      } else {
        next.add(label);
      }
      return next;
    });
  };

  /** Resolve type names to NodeTypeInfo entries, filtering by search. */
  function resolveTypes(typeNames: string[]): NodeTypeInfo[] {
    return typeNames
      .map((t) => typeInfoMap.get(t))
      .filter((info): info is NodeTypeInfo =>
        info != null &&
        (searchLower === "" ||
          info.name.toLowerCase().includes(searchLower) ||
          info.node_type.type.toLowerCase().includes(searchLower)),
      );
  }

  /** Check if a group has any matching items after search filtering. */
  function groupHasItems(group: PaletteGroup): boolean {
    if (group.types) {
      return resolveTypes(group.types).length > 0;
    }
    if (group.subgroups) {
      return group.subgroups.some((sg) => resolveTypes(sg.types).length > 0);
    }
    return false;
  }

  return (
    <div
      className={`flex flex-col border-r border-[var(--border)] bg-[var(--bg-panel)] transition-all duration-200 ${
        collapsed ? "w-12" : "w-56"
      }`}
    >
      {/* Toggle */}
      <button
        onClick={onToggle}
        className="flex h-10 items-center justify-center border-b border-[var(--border)] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-secondary)]"
        title={collapsed ? "Expand node palette" : "Collapse node palette"}
      >
        <svg
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        >
          <path d="M2 4h12M2 8h12M2 12h12" />
        </svg>
      </button>

      {!collapsed && (
        <>
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
            {PALETTE_GROUPS.filter(groupHasItems).map((group) => {
              const isCollapsed = collapsedGroups.has(group.label);
              return (
                <div key={group.label} className="mb-3">
                  {/* Group header (collapsible) */}
                  <button
                    onClick={() => toggleGroup(group.label)}
                    className="flex w-full items-center gap-1 mb-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
                  >
                    <span className="text-[8px]">{isCollapsed ? "\u25B6" : "\u25BC"}</span>
                    <span>{group.label}</span>
                  </button>

                  {!isCollapsed && (
                    <>
                      {group.note && (
                        <p className="text-[9px] text-[var(--text-muted)] opacity-70 mb-1.5 px-1 leading-tight">
                          {group.note}
                        </p>
                      )}

                      {/* Groups with subgroups (Query / Action) */}
                      {group.subgroups?.map((sg) => {
                        const items = resolveTypes(sg.types);
                        if (items.length === 0) return null;
                        const subgroupTooltip: Record<string, string> = {
                          Query: "Query nodes return data you can use in conditions and variable wiring",
                          Action: "Action nodes perform effects like clicking, typing, or navigating",
                        };
                        const role = sg.label === "Query" || sg.label === "Action" ? sg.label : undefined;
                        return (
                          <div key={sg.label} className="mb-1.5">
                            <h5
                              className="text-[9px] font-medium text-[var(--text-muted)] opacity-70 uppercase tracking-wider px-2 mb-0.5"
                              title={subgroupTooltip[sg.label]}
                            >
                              {sg.label}
                            </h5>
                            <div className="flex flex-col gap-0.5">
                              {items.map((info) => (
                                <NodeItem key={info.name} info={info} role={role} onAdd={onAdd} />
                              ))}
                            </div>
                          </div>
                        );
                      })}

                      {/* Groups with flat types */}
                      {group.types && (
                        <div className="flex flex-col gap-0.5">
                          {resolveTypes(group.types).map((info) => (
                            <NodeItem key={info.name} info={info} onAdd={onAdd} />
                          ))}
                        </div>
                      )}
                    </>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
