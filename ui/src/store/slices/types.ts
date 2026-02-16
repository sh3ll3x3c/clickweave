import type { AssistantSlice } from "./assistantSlice";
import type { ExecutionSlice } from "./executionSlice";
import type { LogSlice } from "./logSlice";
import type { ProjectSlice } from "./projectSlice";
import type { SettingsSlice } from "./settingsSlice";
import type { UiSlice } from "./uiSlice";
import type { VerdictSlice } from "./verdictSlice";

export type StoreState = AssistantSlice &
  ExecutionSlice &
  LogSlice &
  ProjectSlice &
  SettingsSlice &
  UiSlice &
  VerdictSlice;
