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
        hasCanvasSelection,
        walkthroughStatus,
        walkthroughPanelOpen,
        cancelWalkthrough,
        discardDraft,
        setWalkthroughPanelOpen,
        assistantSurface,
        logsDrawerOpen,
        setShowSettings,
        selectNode,
        clearCanvasSelection,
        setAssistantSurface,
        toggleLogsDrawer,
      } = useStore.getState();

      const walkthroughActive = isWalkthroughActive(walkthroughStatus);

      if (verdictModalOpen) {
        closeVerdictModal();
      } else if (showSettings) {
        setShowSettings(false);
      } else if (hasCanvasSelection) {
        // Canvas-only selections (groups, or 2+ nodes) are not represented
        // by `selectedNode`, so prefer this branch before the single-node
        // one. `clearCanvasSelection` also resets `selectedNode` to null.
        clearCanvasSelection();
      } else if (selectedNode !== null) {
        selectNode(null);
      } else if (assistantSurface === "drawer") {
        // Only the Canvas drawer is closable via Esc; the Overview embedded
        // card has no Esc-close affordance (it's always live on Overview).
        // Closing here also reveals any walkthrough review hidden behind it.
        setAssistantSurface(null);
      } else if (walkthroughActive && walkthroughPanelOpen) {
        // Close the panel first; a second Escape will discard.
        setWalkthroughPanelOpen(false);
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
