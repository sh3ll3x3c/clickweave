import { useEffect } from "react";
import { useStore } from "../store/useAppStore";

/**
 * Global Escape key handler that closes panels in priority order:
 * Settings modal → Node detail → Assistant panel → Logs drawer
 *
 * Reads state at event time via getState() so the listener is registered
 * once and always sees fresh values.
 */
export function useEscapeKey() {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;

      const {
        showSettings,
        selectedNode,
        assistantOpen,
        logsDrawerOpen,
        setShowSettings,
        selectNode,
        setAssistantOpen,
        toggleLogsDrawer,
      } = useStore.getState();

      if (showSettings) {
        setShowSettings(false);
      } else if (selectedNode !== null) {
        selectNode(null);
      } else if (assistantOpen) {
        setAssistantOpen(false);
      } else if (logsDrawerOpen) {
        toggleLogsDrawer();
      } else {
        return;
      }

      e.preventDefault();
    };

    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);
}
