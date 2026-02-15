import type { SettingsSlice } from "./settingsSlice";
import type { ProjectSlice } from "./projectSlice";
import type { AssistantSlice } from "./assistantSlice";
import type { ExecutionSlice } from "./executionSlice";
import type { LogSlice } from "./logSlice";
import type { UiSlice } from "./uiSlice";

export type StoreState = SettingsSlice &
  ProjectSlice &
  AssistantSlice &
  ExecutionSlice &
  LogSlice &
  UiSlice;
