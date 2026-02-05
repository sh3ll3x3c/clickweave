import type { LlmConfig } from "../store/useAppStore";

interface SettingsModalProps {
  open: boolean;
  llmConfig: LlmConfig;
  mcpCommand: string;
  onClose: () => void;
  onLlmConfigChange: (config: LlmConfig) => void;
  onMcpCommandChange: (cmd: string) => void;
}

const inputClass =
  "w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]";

export function SettingsModal({
  open,
  llmConfig,
  mcpCommand,
  onClose,
  onLlmConfigChange,
  onMcpCommandChange,
}: SettingsModalProps) {
  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[480px] rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] shadow-xl">
        <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <h2 className="text-sm font-semibold text-[var(--text-primary)]">
            Settings
          </h2>
          <button
            onClick={onClose}
            className="text-[var(--text-muted)] hover:text-[var(--text-primary)]"
          >
            x
          </button>
        </div>

        <div className="space-y-4 p-4">
          <div>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
              LLM Configuration
            </h3>
            <div className="space-y-2">
              <div>
                <label className="mb-1 block text-xs text-[var(--text-secondary)]">
                  Base URL
                </label>
                <input
                  type="text"
                  value={llmConfig.baseUrl}
                  onChange={(e) =>
                    onLlmConfigChange({ ...llmConfig, baseUrl: e.target.value })
                  }
                  className={inputClass}
                />
              </div>
              <div>
                <label className="mb-1 block text-xs text-[var(--text-secondary)]">
                  Model
                </label>
                <input
                  type="text"
                  value={llmConfig.model}
                  onChange={(e) =>
                    onLlmConfigChange({ ...llmConfig, model: e.target.value })
                  }
                  className={inputClass}
                />
              </div>
              <div>
                <label className="mb-1 block text-xs text-[var(--text-secondary)]">
                  API Key
                </label>
                <input
                  type="password"
                  value={llmConfig.apiKey}
                  onChange={(e) =>
                    onLlmConfigChange({ ...llmConfig, apiKey: e.target.value })
                  }
                  placeholder="Optional"
                  className={`${inputClass} placeholder-[var(--text-muted)]`}
                />
              </div>
            </div>
          </div>

          <div>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
              MCP Server
            </h3>
            <div>
              <label className="mb-1 block text-xs text-[var(--text-secondary)]">
                Command
              </label>
              <input
                type="text"
                value={mcpCommand}
                onChange={(e) => onMcpCommandChange(e.target.value)}
                placeholder="npx"
                className={`${inputClass} placeholder-[var(--text-muted)]`}
              />
              <p className="mt-1 text-[10px] text-[var(--text-muted)]">
                Use "npx" for native-devtools-mcp, or a custom command path
              </p>
            </div>
          </div>
        </div>

        <div className="flex justify-end border-t border-[var(--border)] px-4 py-3">
          <button
            onClick={onClose}
            className="rounded bg-[var(--accent-coral)] px-4 py-1.5 text-xs font-medium text-white hover:opacity-90"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
