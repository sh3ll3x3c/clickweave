import { Modal } from "./Modal";

interface Props {
  open: boolean;
  agentNodeCount: number;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmClearConversationModal({
  open,
  agentNodeCount,
  onConfirm,
  onCancel,
}: Props) {
  return (
    <Modal
      open={open}
      onClose={onCancel}
      className="w-[420px] max-w-[92vw] rounded-xl border border-[var(--border)] bg-[var(--bg-dark)] p-4 shadow-2xl"
    >
      <div className="space-y-3 text-sm text-[var(--text-primary)]">
        <h3 className="text-base font-semibold">Clear conversation</h3>
        <p>
          Delete {agentNodeCount} agent-built node
          {agentNodeCount === 1 ? "" : "s"}, wipe the agent cache and
          conversational memory, and drop the chat transcript?
        </p>
        <p className="text-[var(--text-secondary)]">
          This includes any nodes you may have edited after the agent
          created them. This action cannot be undone.
        </p>
        <div className="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg border border-[var(--border)] px-3 py-1.5 text-xs font-medium text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded-lg bg-red-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-red-500"
          >
            Clear conversation
          </button>
        </div>
      </div>
    </Modal>
  );
}
