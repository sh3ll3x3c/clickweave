import { useEffect } from "react";
import { useStore } from "../store/useAppStore";
import { isWalkthroughActive } from "../store/slices/walkthroughSlice";

/**
 * Global Escape key handler that closes panels in priority order:
 * Verdict modal → Settings modal → Node detail → Assistant panel → Logs drawer
 *
 * Reads state at event time via getState() so the listener is registered
 * once and always sees fresh values.
 */
export function useEscapeKey() {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;

      const {
        verdictModalOpen,
        closeVerdictModal,
        showSettings,
        selectedNode,
        walkthroughStatus,
        cancelWalkthrough,
        discardDraft,
        assistantOpen,
        logsDrawerOpen,
        setShowSettings,
        selectNode,
        setAssistantOpen,
        toggleLogsDrawer,
      } = useStore.getState();

      const walkthroughActive = isWalkthroughActive(walkthroughStatus);

      if (verdictModalOpen) {
        closeVerdictModal();
      } else if (showSettings) {
        setShowSettings(false);
      } else if (selectedNode !== null) {
        selectNode(null);
      } else if (assistantOpen) {
        // Close assistant first — if walkthrough review is behind it, this reveals it.
        setAssistantOpen(false);
      } else if (walkthroughActive) {
        if (walkthroughStatus === "Recording" || walkthroughStatus === "Paused") {
          cancelWalkthrough();
        } else {
          discardDraft();
        }
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
