import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useStore } from "../../store/useAppStore";

/** Subscribe to executor supervision events. */
export function useSupervisionEvents() {
  useEffect(() => {
    const unlisteners: (() => void)[] = [];
    let cancelled = false;

    const sub = (p: Promise<() => void>) =>
      p.then((u) => {
        if (cancelled) { u(); return; }
        unlisteners.push(u);
      }).catch((err) => {
        console.error("Failed to subscribe to supervision event:", err);
        useStore.getState().pushLog(`Critical: supervision event listener failed: ${err}`);
      });

    sub(listen<{ node_id: string; node_name: string; summary: string }>(
      "executor://supervision_passed",
      (e) => {
        useStore.getState().pushLog(`Verified: ${e.payload.node_name} — ${e.payload.summary}`);
      },
    ));
    sub(listen<{ node_id: string; node_name: string; finding: string; screenshot: string | null }>(
      "executor://supervision_paused",
      (e) => {
        useStore.getState().setSupervisionPause({
          nodeId: e.payload.node_id,
          nodeName: e.payload.node_name,
          finding: e.payload.finding,
          screenshot: e.payload.screenshot ?? null,
        });
      },
    ));

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);
}
