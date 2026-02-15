import type { Node, NodeType } from "../../../../bindings";

export interface NodeEditorProps {
  nodeType: NodeType;
  onUpdate: (u: Partial<Node>) => void;
  projectPath: string | null;
}
