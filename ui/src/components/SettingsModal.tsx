import type { EndpointConfig } from "../store/useAppStore";

interface SettingsModalProps {
  open: boolean;
  orchestratorConfig: EndpointConfig;
  vlmConfig: EndpointConfig;
  vlmEnabled: boolean;
  mcpCommand: string;
  onClose: () => void;
  onOrchestratorConfigChange: (config: EndpointConfig) => void;
  onVlmConfigChange: (config: EndpointConfig) => void;
  onVlmEnabledChange: (enabled: boolean) => void;
  onMcpCommandChange: (cmd: string) => void;
}

const inputClass =
  "w-full rounded bg-[var(--bg-input)] px-2.5 py-1.5 text-xs text-[var(--text-primary)] outline-none focus:ring-1 focus:ring-[var(--accent-coral)]";

function EndpointFields({
  config,
  onChange,
}: {
  config: EndpointConfig;
  onChange: (config: EndpointConfig) => void;
}) {
  return (
    <div className="space-y-2">
      <div>
        <label className="mb-1 block text-xs text-[var(--text-secondary)]">
          Base URL
        </label>
        <input
          type="text"
          value={config.baseUrl}
          onChange={(e) => onChange({ ...config, baseUrl: e.target.value })}
          className={inputClass}
        />
      </div>
      <div>
        <label className="mb-1 block text-xs text-[var(--text-secondary)]">
          Model
        </label>
        <input
          type="text"
          value={config.model}
          onChange={(e) => onChange({ ...config, model: e.target.value })}
          className={inputClass}
        />
      </div>
      <div>
        <label className="mb-1 block text-xs text-[var(--text-secondary)]">
          API Key
        </label>
        <input
          type="password"
          value={config.apiKey}
          onChange={(e) => onChange({ ...config, apiKey: e.target.value })}
          placeholder="Optional"
          className={`${inputClass} placeholder-[var(--text-muted)]`}
        />
      </div>
    </div>
  );
}

export function SettingsModal({
  open,
  orchestratorConfig,
  vlmConfig,
  vlmEnabled,
  mcpCommand,
  onClose,
  onOrchestratorConfigChange,
  onVlmConfigChange,
  onVlmEnabledChange,
  onMcpCommandChange,
}: SettingsModalProps) {
  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[480px] max-h-[90vh] overflow-y-auto rounded-lg border border-[var(--border)] bg-[var(--bg-panel)] shadow-xl">
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
              Orchestrator
            </h3>
            <p className="mb-2 text-[10px] text-[var(--text-muted)]">
              Decides tool calls and controls the workflow. Does not receive images.
            </p>
            <EndpointFields
              config={orchestratorConfig}
              onChange={onOrchestratorConfigChange}
            />
          </div>

          <div>
            <div className="mb-2 flex items-center gap-2">
              <h3 className="text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                Vision (VLM)
              </h3>
              <label className="flex items-center gap-1.5 text-xs text-[var(--text-secondary)] cursor-pointer">
                <input
                  type="checkbox"
                  checked={vlmEnabled}
                  onChange={(e) => onVlmEnabledChange(e.target.checked)}
                  className="accent-[var(--accent-coral)]"
                />
                Separate model
              </label>
            </div>
            {vlmEnabled ? (
              <>
                <p className="mb-2 text-[10px] text-[var(--text-muted)]">
                  Analyzes screenshots and images, returns text summaries to the orchestrator.
                </p>
                <EndpointFields
                  config={vlmConfig}
                  onChange={onVlmConfigChange}
                />
              </>
            ) : (
              <p className="text-[10px] text-[var(--text-muted)]">
                Using orchestrator model for vision. Enable to use a separate vision model.
              </p>
            )}
          </div>

          <div>
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wider text-[var(--text-muted)]">
              native-devtools-mcp
            </h3>
            <p className="mb-2 text-[10px] text-[var(--text-muted)]">
              Provides browser automation and screenshot tools for workflow execution.
            </p>
            <div>
              <label className="mb-1 block text-xs text-[var(--text-secondary)]">
                Binary path
              </label>
              <input
                type="text"
                value={mcpCommand === "npx" ? "" : mcpCommand}
                onChange={(e) =>
                  onMcpCommandChange(e.target.value.trim() || "npx")
                }
                placeholder="Default (npx)"
                className={`${inputClass} placeholder-[var(--text-muted)]`}
              />
              <p className="mt-1 text-[10px] text-[var(--text-muted)]">
                Leave empty to use npx, or set a path to a local binary
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
