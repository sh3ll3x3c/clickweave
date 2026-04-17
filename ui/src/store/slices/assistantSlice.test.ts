import { describe, it, expect, vi, beforeEach } from "vitest";

// `invoke` is pulled in transitively by the composed store; mock before import.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { useStore } from "../useAppStore";

describe("assistantSlice.pushAssistantMessage", () => {
  beforeEach(() => {
    useStore.setState({ messages: [] });
  });

  it("appends a user message with role/content/timestamp", () => {
    useStore.getState().pushAssistantMessage("user", "hello");
    const msgs = useStore.getState().messages;
    expect(msgs).toHaveLength(1);
    expect(msgs[0].role).toBe("user");
    expect(msgs[0].content).toBe("hello");
    expect(typeof msgs[0].timestamp).toBe("string");
    expect(msgs[0].timestamp.length).toBeGreaterThan(0);
  });

  it("appends assistant messages in order", () => {
    useStore.getState().pushAssistantMessage("user", "first");
    useStore.getState().pushAssistantMessage("assistant", "second");
    const msgs = useStore.getState().messages;
    expect(msgs.map((m) => m.content)).toEqual(["first", "second"]);
    expect(msgs.map((m) => m.role)).toEqual(["user", "assistant"]);
  });

  it("trims whitespace and ignores empty content", () => {
    useStore.getState().pushAssistantMessage("user", "  padded  ");
    useStore.getState().pushAssistantMessage("user", "   ");
    useStore.getState().pushAssistantMessage("user", "");
    const msgs = useStore.getState().messages;
    expect(msgs).toHaveLength(1);
    expect(msgs[0].content).toBe("padded");
  });
});

describe("AssistantMessage extensions", () => {
  beforeEach(() => {
    useStore.setState({ messages: [] });
  });

  it("accepts the system role via pushSystemAnnotation", () => {
    useStore
      .getState()
      .pushSystemAnnotation('Deleted 3 nodes from "Send test"');
    const msgs = useStore.getState().messages;
    expect(msgs).toHaveLength(1);
    expect(msgs[0].role).toBe("system");
    expect(msgs[0].runId).toBeUndefined();
  });

  it("tags user messages with the provided runId", () => {
    useStore
      .getState()
      .pushAssistantMessage("user", "hello", "11111111-1111-1111-1111-111111111111");
    const msg = useStore.getState().messages[0];
    expect(msg.runId).toBe("11111111-1111-1111-1111-111111111111");
  });

  it("clearConversation empties the messages array", () => {
    useStore.getState().pushAssistantMessage("user", "a");
    useStore.getState().pushAssistantMessage("assistant", "b");
    useStore.getState().clearConversation();
    expect(useStore.getState().messages).toEqual([]);
  });

  it("setMessages replaces the full array", () => {
    useStore.getState().pushAssistantMessage("user", "keep me?");
    useStore.getState().setMessages([
      { role: "user", content: "hydrated", timestamp: "t1", runId: "r1" },
    ]);
    const msgs = useStore.getState().messages;
    expect(msgs).toHaveLength(1);
    expect(msgs[0].content).toBe("hydrated");
    expect(msgs[0].runId).toBe("r1");
  });

  it("mapMessagesByRunIds updates only matching messages", () => {
    useStore.getState().pushAssistantMessage("user", "goal", "r1");
    useStore.getState().pushAssistantMessage("assistant", "summary", "r1");
    useStore.getState().pushAssistantMessage("user", "other", "r2");
    useStore.getState().mapMessagesByRunIds(new Set(["r1"]), (m) =>
      m.role === "assistant" ? { ...m, content: "(partially deleted)" } : m,
    );
    const assistants = useStore
      .getState()
      .messages.filter((m) => m.role === "assistant");
    expect(assistants[0].content).toBe("(partially deleted)");
    const otherUser = useStore
      .getState()
      .messages.find((m) => m.runId === "r2");
    expect(otherUser?.content).toBe("other");
  });

  it("dropTurnsByRunIds removes user/assistant messages but keeps system annotations", () => {
    useStore.getState().pushAssistantMessage("user", "goal", "r1");
    useStore.getState().pushAssistantMessage("assistant", "summary", "r1");
    useStore.getState().pushSystemAnnotation("kept note");
    useStore.getState().pushAssistantMessage("user", "other", "r2");
    useStore.getState().dropTurnsByRunIds(new Set(["r1"]));
    const msgs = useStore.getState().messages;
    expect(msgs.some((m) => m.runId === "r1")).toBe(false);
    expect(msgs.some((m) => m.role === "system")).toBe(true);
    expect(msgs.some((m) => m.runId === "r2")).toBe(true);
  });
});
