import { useState } from "react";

interface IntentEmptyStateProps {
  onGenerate: (intent: string) => void;
  onSkip: () => void;
  loading: boolean;
}

export function IntentEmptyState({ onGenerate, onSkip, loading }: IntentEmptyStateProps) {
  const [intent, setIntent] = useState("");

  const handleSubmit = () => {
    if (intent.trim()) {
      onGenerate(intent.trim());
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey && intent.trim()) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <div className="flex flex-1 items-center justify-center bg-[var(--bg-dark)]">
      <div className="flex w-[480px] flex-col items-center gap-6">
        <div className="text-center">
          <h2 className="text-lg font-medium text-[var(--text-primary)]">
            What should this workflow do?
          </h2>
          <p className="mt-1 text-xs text-[var(--text-muted)]">
            Describe the steps and we'll generate a workflow for you.
          </p>
        </div>

        <div className="w-full">
          <textarea
            value={intent}
            onChange={(e) => setIntent(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="e.g. Open Safari, navigate to example.com, take a screenshot, find the login button and click it"
            rows={4}
            autoFocus
            disabled={loading}
            className="w-full rounded-lg border border-[var(--border)] bg-[var(--bg-input)] px-4 py-3 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-coral)]"
          />
        </div>

        <div className="flex items-center gap-3">
          <button
            onClick={handleSubmit}
            disabled={loading || !intent.trim()}
            className="rounded-lg bg-[var(--accent-coral)] px-5 py-2 text-sm font-medium text-white hover:opacity-90 disabled:opacity-50"
          >
            {loading ? "Generating..." : "Generate Workflow"}
          </button>
          <button
            onClick={onSkip}
            className="rounded-lg px-4 py-2 text-xs text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
          >
            Build manually
          </button>
        </div>
      </div>
    </div>
  );
}
