import { useState, useRef, useEffect, useCallback } from "react";
import type { ConversationSession, WorkflowPatch } from "../bindings";
import { ChatMessage } from "./ChatMessage";

interface AssistantPanelProps {
  open: boolean;
  loading: boolean;
  error: string | null;
  conversation: ConversationSession;
  pendingPatch: WorkflowPatch | null;
  pendingPatchWarnings: string[];
  onSendMessage: (message: string) => void;
  onApplyPatch: () => void;
  onDiscardPatch: () => void;
  onClearConversation: () => void;
  onClose: () => void;
}

export function AssistantPanel({
  open,
  loading,
  error,
  conversation,
  pendingPatch,
  pendingPatchWarnings,
  onSendMessage,
  onApplyPatch,
  onDiscardPatch,
  onClearConversation,
  onClose,
}: AssistantPanelProps) {
  const [input, setInput] = useState("");
  const [width, setWidth] = useState(380);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const widthRef = useRef(width);
  widthRef.current = width;

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = widthRef.current;
    document.body.style.userSelect = "none";

    const onMove = (e: MouseEvent) => {
      e.preventDefault();
      setWidth(Math.min(600, Math.max(280, startWidth + (startX - e.clientX))));
    };
    const onUp = () => {
      document.body.style.userSelect = "";
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }, []);

  // Auto-scroll to bottom when messages change
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [conversation.messages.length, loading]);

  // Focus textarea when panel opens
  useEffect(() => {
    if (open) {
      // Small delay to allow transition
      const timer = setTimeout(() => textareaRef.current?.focus(), 100);
      return () => clearTimeout(timer);
    }
  }, [open]);

  if (!open) return null;

  const handleSend = () => {
    const trimmed = input.trim();
    if (!trimmed || loading) return;
    setInput("");
    onSendMessage(trimmed);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Find the index of the last assistant message
  const lastAssistantIndex = conversation.messages.reduceRight(
    (found, msg, idx) => (found === -1 && msg.role === "assistant" ? idx : found),
    -1,
  );

  const hasMessages = conversation.messages.length > 0;

  return (
    <div className="relative flex h-full flex-col border-l border-[var(--border)] bg-[var(--bg-panel)]" style={{ width, minWidth: width }}>
      {/* Resize handle */}
      <div
        onMouseDown={handleResizeStart}
        className="absolute left-0 top-0 z-10 h-full w-1.5 cursor-col-resize hover:bg-[var(--accent-coral)]/30 active:bg-[var(--accent-coral)]/40"
      />
      {/* Header */}
      <div className="flex items-center justify-between border-b border-[var(--border)] px-4 py-2.5">
        <h2 className="text-sm font-medium text-[var(--text-primary)]">
          Assistant
        </h2>
        <div className="flex items-center gap-1">
          {hasMessages && (
            <button
              onClick={onClearConversation}
              className="rounded px-2 py-1 text-[11px] text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-secondary)]"
              title="Clear conversation"
            >
              Clear
            </button>
          )}
          <button
            onClick={onClose}
            className="rounded px-1.5 py-0.5 text-[var(--text-muted)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            title="Close panel"
          >
            &times;
          </button>
        </div>
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto px-3 py-3">
        {!hasMessages && (
          <div className="flex h-full items-center justify-center">
            <p className="text-center text-xs text-[var(--text-muted)]">
              Ask me to create or modify your workflow.
            </p>
          </div>
        )}

        <div className="space-y-3">
          {conversation.messages.map((entry, idx) => (
            <ChatMessage
              key={`${entry.timestamp}-${idx}`}
              entry={entry}
              isLastAssistant={idx === lastAssistantIndex}
              pendingPatch={pendingPatch}
              pendingPatchWarnings={pendingPatchWarnings}
              onApplyPatch={onApplyPatch}
              onDiscardPatch={onDiscardPatch}
            />
          ))}

          {/* Loading indicator */}
          {loading && (
            <div className="flex justify-start">
              <div className="flex items-center gap-2 rounded-lg bg-[var(--bg-hover)] px-3 py-2">
                <div className="h-3.5 w-3.5 animate-spin rounded-full border-2 border-[var(--accent-coral)] border-t-transparent" />
                <span className="text-xs text-[var(--text-secondary)]">
                  Thinking...
                </span>
              </div>
            </div>
          )}

          <div ref={messagesEndRef} />
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="mx-3 mb-2 rounded border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] text-red-400">
          {error}
        </div>
      )}

      {/* Input */}
      <div className="border-t border-[var(--border)] px-3 py-3">
        <div className="flex gap-2">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Ask about your workflow..."
            rows={2}
            disabled={loading}
            className="flex-1 resize-none rounded-lg border border-[var(--border)] bg-[var(--bg-input)] px-3 py-2 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-muted)] outline-none focus:border-[var(--accent-coral)]"
          />
          <button
            onClick={handleSend}
            disabled={loading || !input.trim()}
            className="self-end rounded-lg bg-[var(--accent-coral)] px-3 py-2 text-xs font-medium text-white hover:opacity-90 disabled:opacity-40"
          >
            Send
          </button>
        </div>
        <p className="mt-1.5 text-[10px] text-[var(--text-muted)]">
          Enter to send, Shift+Enter for new line
        </p>
      </div>
    </div>
  );
}
