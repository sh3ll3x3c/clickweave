import type { ReactNode } from "react";

const settingRowClass =
  "flex items-center justify-between gap-3 rounded-lg bg-[var(--bg-dark)] px-3.5 py-2.5";

interface SettingRowProps {
  title: string;
  description: string;
  control: ReactNode;
}

/** Labeled row used by the Permissions and Privacy settings tabs.
 *  Title + description on the left, interactive control on the right. */
export function SettingRow({ title, description, control }: SettingRowProps) {
  return (
    <div className={settingRowClass}>
      <div>
        <div className="text-xs font-semibold text-[var(--text-primary)]">
          {title}
        </div>
        <div className="mt-0.5 text-[10px] text-[var(--text-muted)]">
          {description}
        </div>
      </div>
      {control}
    </div>
  );
}

interface ToggleProps {
  checked: boolean;
  onChange: (next: boolean) => void;
}

/** Pill-style on/off switch used by SettingRow-style tabs. */
export function Toggle({ checked, onChange }: ToggleProps) {
  return (
    <button
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={`relative h-[22px] w-10 flex-shrink-0 rounded-full transition-colors ${
        checked ? "bg-[var(--accent-coral)]" : "bg-[var(--bg-input)]"
      }`}
    >
      <span
        className={`absolute top-[3px] h-4 w-4 rounded-full bg-white transition-[left] ${
          checked ? "left-[21px]" : "left-[3px]"
        }`}
      />
    </button>
  );
}
