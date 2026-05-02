import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useEscapeKey } from "./useEscapeKey";
import { useStore } from "../store/useAppStore";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

function Harness() {
  useEscapeKey();
  return null;
}

function pressEscape() {
  window.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    }),
  );
}

describe("useEscapeKey overview visibility", () => {
  beforeEach(() => {
    useStore.setState({
      currentView: "overview",
      verdictModalOpen: false,
      showSettings: false,
      selectedNode: null,
      hasCanvasSelection: false,
      assistantSurface: null,
      walkthroughStatus: "Idle",
      walkthroughPanelOpen: false,
      logsDrawerOpen: false,
    });
  });

  it("does not let hidden canvas selection consume Escape on Overview", () => {
    useStore.setState({
      currentView: "overview",
      selectedNode: "node-1",
      logsDrawerOpen: true,
    });
    render(<Harness />);

    pressEscape();

    expect(useStore.getState().selectedNode).toBe("node-1");
    expect(useStore.getState().logsDrawerOpen).toBe(false);
  });

  it("does not let a hidden drawer surface consume Escape on Overview", () => {
    useStore.setState({
      currentView: "overview",
      assistantSurface: "drawer",
      logsDrawerOpen: true,
    });
    render(<Harness />);

    pressEscape();

    expect(useStore.getState().assistantSurface).toBe("drawer");
    expect(useStore.getState().logsDrawerOpen).toBe(false);
  });

  it("still clears visible canvas selection before Logs on Canvas", () => {
    useStore.setState({
      currentView: "canvas",
      selectedNode: "node-1",
      logsDrawerOpen: true,
    });
    render(<Harness />);

    pressEscape();

    expect(useStore.getState().selectedNode).toBeNull();
    expect(useStore.getState().logsDrawerOpen).toBe(true);
  });
});
