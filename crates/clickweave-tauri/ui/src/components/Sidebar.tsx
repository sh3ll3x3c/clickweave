interface SidebarProps {
  collapsed: boolean;
  onToggle: () => void;
}

const navItems = [
  { icon: "home", label: "Home" },
  { icon: "grid", label: "Templates" },
  { icon: "variable", label: "Variables" },
  { icon: "play", label: "Executions" },
  { icon: "help", label: "Help" },
];

const icons: Record<string, string> = {
  home: "H",
  grid: "T",
  variable: "V",
  play: "E",
  help: "?",
};

export function Sidebar({ collapsed, onToggle }: SidebarProps) {
  return (
    <div
      className={`flex flex-col border-r border-[var(--border)] bg-[var(--bg-panel)] transition-all duration-200 ${
        collapsed ? "w-12" : "w-48"
      }`}
    >
      {/* Logo / Toggle */}
      <button
        onClick={onToggle}
        className="flex h-12 items-center gap-2 border-b border-[var(--border)] px-3 hover:bg-[var(--bg-hover)]"
      >
        <span className="text-lg font-bold text-[var(--accent-coral)]">C</span>
        {!collapsed && (
          <span className="text-sm font-semibold text-[var(--text-primary)]">
            Clickweave
          </span>
        )}
      </button>

      {/* Nav items */}
      <nav className="flex flex-1 flex-col gap-0.5 p-1.5">
        {navItems.map((item) => (
          <button
            key={item.icon}
            className="flex items-center gap-2 rounded px-2.5 py-2 text-sm text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
          >
            <span className="inline-flex h-5 w-5 items-center justify-center text-xs font-bold">
              {icons[item.icon]}
            </span>
            {!collapsed && <span>{item.label}</span>}
          </button>
        ))}
      </nav>
    </div>
  );
}
