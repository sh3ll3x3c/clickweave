import { useEffect, useState } from "react";
import { commands } from "../../../bindings";
import type { NodeRun } from "../../../bindings";

export function useNodeRuns(
  projectPath: string | null,
  workflowId: string,
  workflowName: string,
  nodeName: string,
): NodeRun[] {
  const [runs, setRuns] = useState<NodeRun[]>([]);

  useEffect(() => {
    commands
      .listRuns({
        project_path: projectPath,
        workflow_id: workflowId,
        workflow_name: workflowName,
        node_name: nodeName,
      })
      .then((result) => {
        if (result.status === "ok") {
          setRuns([...result.data].reverse());
        }
      });
  }, [projectPath, workflowId, workflowName, nodeName]);

  return runs;
}
