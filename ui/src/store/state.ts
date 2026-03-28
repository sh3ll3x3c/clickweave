import type { Workflow } from "../bindings";

export type DetailTab = "setup" | "trace" | "runs";

export interface EndpointConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

export const DEFAULT_ENDPOINT: EndpointConfig = {
  baseUrl: "http://localhost:1234/v1",
  apiKey: "",
  model: "local",
};

export const DEFAULT_VLM_ENABLED = false;

export function makeDefaultWorkflow(): Workflow {
  return {
    id: crypto.randomUUID(),
    name: "New Workflow",
    nodes: [],
    edges: [],
    groups: [],
  };
}
