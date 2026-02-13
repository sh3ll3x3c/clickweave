import { useEffect, useState } from "react";
import { commands } from "../../../bindings";
import type { NodeRun } from "../../../bindings";

export function useNodeRuns(
  projectPath: string | null,
  workflowId: string,
  nodeId: string,
): NodeRun[] {
  const [runs, setRuns] = useState<NodeRun[]>([]);

  useEffect(() => {
    commands
      .listRuns({
        project_path: projectPath,
        workflow_id: workflowId,
        node_id: nodeId,
      })
      .then((result) => {
        if (result.status === "ok") {
          setRuns([...result.data].reverse());
        }
      });
  }, [projectPath, workflowId, nodeId]);

  return runs;
}
