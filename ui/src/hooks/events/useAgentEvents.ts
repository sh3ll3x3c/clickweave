import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useStore } from "../../store/useAppStore";

interface AgentStepPayload {
  summary: string;
  tool_name: string;
  step_number: number;
}

interface AgentPlanPayload {
  horizon: string[];
}

interface AgentErrorPayload {
  message: string;
}

/**
 * Subscribe to agent backend events:
 * agent://step, agent://plan, agent://complete, agent://error.
 *
 * Dispatches into the Zustand AgentSlice via `getState()`.
 */
export function useAgentEvents() {
  useEffect(() => {
    const unlisteners: (() => void)[] = [];
    let cancelled = false;

    const sub = (p: Promise<() => void>) =>
      p
        .then((u) => {
          if (cancelled) {
            u();
            return;
          }
          unlisteners.push(u);
        })
        .catch((err) => {
          console.error("Failed to subscribe to agent event:", err);
          useStore
            .getState()
            .pushLog(`Critical: agent event listener failed: ${err}`);
        });

    sub(
      listen<AgentStepPayload>("agent://step", (e) => {
        useStore.getState().addAgentStep({
          summary: e.payload.summary,
          toolName: e.payload.tool_name,
          toolArgs: null,
          toolResult: e.payload.summary,
          pageTransitioned: false,
        });
        useStore
          .getState()
          .pushLog(
            `Agent step ${e.payload.step_number}: ${e.payload.tool_name}`,
          );
      }),
    );

    sub(
      listen<AgentPlanPayload>("agent://plan", (e) => {
        useStore.getState().setAgentPlanHorizon(e.payload.horizon);
      }),
    );

    sub(
      listen("agent://complete", () => {
        useStore.getState().setAgentStatus("complete");
        useStore.getState().pushLog("Agent completed");
      }),
    );

    sub(
      listen<AgentErrorPayload>("agent://error", (e) => {
        useStore.getState().setAgentError(e.payload.message);
        useStore.getState().setAgentStatus("error");
        useStore
          .getState()
          .pushLog(`Agent error: ${e.payload.message}`);
      }),
    );

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, []);
}
