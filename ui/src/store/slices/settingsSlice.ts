import type { StateCreator } from "zustand";
import type { EndpointConfig } from "../state";
import { DEFAULT_ENDPOINT, DEFAULT_MCP_COMMAND, DEFAULT_VLM_ENABLED } from "../state";
import { loadSettings, saveSetting } from "../settings";
import type { PersistedSettings } from "../settings";
import type { StoreState } from "./types";

export interface SettingsSlice {
  plannerConfig: EndpointConfig;
  agentConfig: EndpointConfig;
  vlmConfig: EndpointConfig;
  vlmEnabled: boolean;
  mcpCommand: string;
  _settingsLoaded: boolean;

  loadSettingsFromDisk: () => void;
  setPlannerConfig: (config: EndpointConfig) => void;
  setAgentConfig: (config: EndpointConfig) => void;
  setVlmConfig: (config: EndpointConfig) => void;
  setVlmEnabled: (enabled: boolean) => void;
  setMcpCommand: (cmd: string) => void;
}

function persistSetting<K extends keyof PersistedSettings>(
  key: K,
  value: PersistedSettings[K],
  set: (partial: Partial<StoreState>) => void,
) {
  set({ [key]: value } as Partial<StoreState>);
  saveSetting(key, value).catch((e) =>
    console.error(`Failed to save setting "${key}":`, e),
  );
}

export const createSettingsSlice: StateCreator<StoreState, [], [], SettingsSlice> = (set, get) => ({
  plannerConfig: DEFAULT_ENDPOINT,
  agentConfig: DEFAULT_ENDPOINT,
  vlmConfig: DEFAULT_ENDPOINT,
  vlmEnabled: DEFAULT_VLM_ENABLED,
  mcpCommand: DEFAULT_MCP_COMMAND,
  _settingsLoaded: false,

  loadSettingsFromDisk: () => {
    if (get()._settingsLoaded) return;
    set({ _settingsLoaded: true });
    loadSettings()
      .then((s) => {
        set({
          plannerConfig: s.plannerConfig,
          agentConfig: s.agentConfig,
          vlmConfig: s.vlmConfig,
          vlmEnabled: s.vlmEnabled,
          mcpCommand: s.mcpCommand,
        });
      })
      .catch((e) => console.error("Failed to load settings:", e));
  },

  setPlannerConfig: (config) => persistSetting("plannerConfig", config, set),
  setAgentConfig: (config) => persistSetting("agentConfig", config, set),
  setVlmConfig: (config) => persistSetting("vlmConfig", config, set),
  setVlmEnabled: (enabled) => persistSetting("vlmEnabled", enabled, set),
  setMcpCommand: (cmd) => persistSetting("mcpCommand", cmd, set),
});
