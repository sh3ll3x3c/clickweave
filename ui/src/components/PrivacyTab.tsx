import { SettingRow, Toggle } from "./settings/SettingRow";

const inputClass =
  "bg-[var(--bg-input)] text-[var(--text-primary)] border border-[var(--border)] rounded-md px-2.5 py-1 text-[11px]";

interface PrivacyTabProps {
  traceRetentionDays: number;
  storeTraces: boolean;
  onTraceRetentionDaysChange: (days: number) => void;
  onStoreTracesChange: (enabled: boolean) => void;
}

export function PrivacyTab({
  traceRetentionDays,
  storeTraces,
  onTraceRetentionDaysChange,
  onStoreTracesChange,
}: PrivacyTabProps) {
  return (
    <div className="space-y-4 p-4">
      <SettingRow
        title="Store run traces"
        description="Persist agent and workflow run traces to disk. When off, runs execute entirely in memory and nothing is written under the runs directory for this session."
        control={
          <Toggle checked={storeTraces} onChange={onStoreTracesChange} />
        }
      />

      <SettingRow
        title="Trace retention (days)"
        description="Delete run traces older than this many days at app startup. 0 disables cleanup and keeps all traces indefinitely."
        control={
          <input
            type="number"
            min={0}
            max={3650}
            value={traceRetentionDays}
            // The settings slice clamps again on persist, so raw number
            // entry here is safe — passing an out-of-range value round-trips
            // through the clamp before it hits state.
            onChange={(e) => onTraceRetentionDaysChange(Number(e.target.value))}
            aria-label="Trace retention in days"
            className={`${inputClass} w-20 text-center`}
          />
        }
      />
    </div>
  );
}
