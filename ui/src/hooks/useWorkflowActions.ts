import { useStore } from "../store/useAppStore";
import { useWorkflowMutations } from "../store/useWorkflowMutations";
import type { Workflow } from "../bindings";

export function useWorkflowActions() {
  const nodesLength = useStore((s) => s.workflow.nodes.length);
  const pushHistory = useStore((s) => s.pushHistory);

  const setWorkflow: React.Dispatch<React.SetStateAction<Workflow>> = (action) => {
    if (typeof action === "function") {
      useStore.setState((s) => ({ workflow: action(s.workflow) }));
    } else {
      useStore.setState({ workflow: action });
    }
  };

  const setSelectedNode: React.Dispatch<React.SetStateAction<string | null>> = (action) => {
    if (typeof action === "function") {
      useStore.setState((s) => ({ selectedNode: action(s.selectedNode) }));
    } else {
      useStore.setState({ selectedNode: action });
    }
  };

  return useWorkflowMutations(setWorkflow, setSelectedNode, nodesLength, pushHistory);
}
