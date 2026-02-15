import type { Node, NodeType } from "../../../../bindings";

export interface NodeEditorProps {
  nodeType: NodeType;
  onUpdate: (u: Partial<Node>) => void;
  projectPath: string | null;
}

/** Convert an empty string to null, for optional string fields. */
export function optionalString(v: string): string | null {
  return v === "" ? null : v;
}
