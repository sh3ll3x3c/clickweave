import { describe, it, expect, vi, beforeEach } from "vitest";

// Tauri's `invoke` must be mocked before walkthroughSlice is imported — the
// slice imports command bindings at module init time.
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

// Webview window helpers are only needed for the recording-bar lifecycle in
// other actions — stub them so they don't touch a real Tauri runtime.
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: class {
    static async getByLabel() {
      return null;
    }
  },
}));
vi.mock("@tauri-apps/api/window", () => ({
  currentMonitor: async () => null,
}));

import { useStore } from "../useAppStore";

describe("walkthroughSlice.pushWalkthroughEvent", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    useStore.setState({
      walkthroughStatus: "Idle",
      walkthroughEvents: [],
    });
  });

  it("appends the event while Recording", () => {
    useStore.setState({ walkthroughStatus: "Recording" });
    useStore.getState().pushWalkthroughEvent({ kind: { type: "Clicked" } });
    expect(useStore.getState().walkthroughEvents).toHaveLength(1);
  });

  it("appends the event while Paused", () => {
    useStore.setState({ walkthroughStatus: "Paused" });
    useStore.getState().pushWalkthroughEvent({ kind: { type: "Clicked" } });
    expect(useStore.getState().walkthroughEvents).toHaveLength(1);
  });

  it("freezes the counter once the backend transitions to Processing", () => {
    // Simulate two events captured during Recording, then a transition to
    // Processing followed by late hover/CDP events from the drain phase.
    useStore.setState({ walkthroughStatus: "Recording" });
    useStore.getState().pushWalkthroughEvent({ kind: { type: "Clicked" } });
    useStore.getState().pushWalkthroughEvent({ kind: { type: "KeyPressed" } });

    useStore.setState({ walkthroughStatus: "Processing" });
    useStore.getState().pushWalkthroughEvent({ kind: { type: "HoverDetected" } });
    useStore.getState().pushWalkthroughEvent({ kind: { type: "CdpHoverResolved" } });

    expect(useStore.getState().walkthroughEvents).toHaveLength(2);
  });

  it("drops events received in non-capturing states the backend actually emits", () => {
    // cancel_walkthrough ends at Idle; stop_walkthrough goes through
    // Processing → Review. None of these states should keep appending.
    for (const status of ["Idle", "Processing", "Review"] as const) {
      useStore.setState({ walkthroughStatus: status, walkthroughEvents: [] });
      useStore.getState().pushWalkthroughEvent({ kind: { type: "Clicked" } });
      expect(useStore.getState().walkthroughEvents).toHaveLength(0);
    }
  });
});

describe("walkthroughSlice.cancelWalkthrough", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(null);
  });

  it("flips status out of capture before awaiting the backend", async () => {
    useStore.setState({
      walkthroughStatus: "Recording",
      walkthroughEvents: [{ kind: { type: "Clicked" } }],
    });

    // Hold the backend call open so we can observe the synchronous set().
    let resolveInvoke: (() => void) | undefined;
    invokeMock.mockImplementationOnce(
      () => new Promise<void>((r) => {
        resolveInvoke = () => r();
      }),
    );

    const pending = useStore.getState().cancelWalkthrough();
    // Status must already be non-capturing so late drain events are dropped.
    expect(useStore.getState().walkthroughStatus).not.toBe("Recording");
    expect(useStore.getState().walkthroughStatus).not.toBe("Paused");
    expect(useStore.getState().walkthroughEvents).toHaveLength(0);

    // A drain-phase event arriving while cancel is in flight must not
    // repopulate the cleared array.
    useStore.getState().pushWalkthroughEvent({ kind: { type: "HoverDetected" } });
    expect(useStore.getState().walkthroughEvents).toHaveLength(0);

    resolveInvoke?.();
    await pending;
  });
});
