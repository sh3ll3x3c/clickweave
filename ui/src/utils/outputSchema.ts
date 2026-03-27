export interface OutputFieldInfo {
  name: string;
  type: string;
  description: string;
}

export const OUTPUT_SCHEMAS: Record<string, OutputFieldInfo[]> = {
  FindText: [
    { name: "found", type: "Bool", description: "Whether any matches were found" },
    { name: "count", type: "Number", description: "Number of matches found" },
    { name: "text", type: "String", description: "Text of the first match" },
    { name: "coordinates", type: "Object", description: "Coordinates of the first match" },
  ],
  FindImage: [
    { name: "found", type: "Bool", description: "Whether any matches were found" },
    { name: "count", type: "Number", description: "Number of matches found" },
    { name: "coordinates", type: "Object", description: "Coordinates of the first match" },
    { name: "confidence", type: "Number", description: "Confidence score" },
  ],
  FindApp: [
    { name: "found", type: "Bool", description: "Whether the app is running" },
    { name: "name", type: "String", description: "App name" },
    { name: "pid", type: "Number", description: "Process ID" },
  ],
  TakeScreenshot: [{ name: "result", type: "String", description: "Screenshot data" }],
  CdpWait: [{ name: "found", type: "Bool", description: "Whether text appeared" }],
  AiStep: [{ name: "result", type: "String", description: "LLM response text" }],
  McpToolCall: [{ name: "result", type: "Any", description: "Raw tool result" }],
  AppDebugKitOp: [{ name: "result", type: "Any", description: "Raw tool result" }],
};

/** Get output schema fields for a node type name (static, without verification). */
export function getOutputSchema(nodeTypeName: string): OutputFieldInfo[] {
  return OUTPUT_SCHEMAS[nodeTypeName] ?? [];
}

const VERIFICATION_FIELDS: OutputFieldInfo[] = [
  { name: "verified", type: "Bool", description: "Whether the action had the intended effect" },
  { name: "verification_reasoning", type: "String", description: "Explanation of the verification result" },
];

/** Get full output schema for a node type, including verification fields when
 *  both verification_method and verification_assertion are set. */
export function getFullOutputSchema(nodeType: Record<string, unknown>): OutputFieldInfo[] {
  const typeName = (nodeType as { type?: string }).type ?? "";
  const base = OUTPUT_SCHEMAS[typeName] ?? [];
  if (nodeType.verification_method && nodeType.verification_assertion) {
    return [...base, ...VERIFICATION_FIELDS];
  }
  return base;
}

/** Get the node type name from a NodeType tagged union object. */
export function nodeTypeName(nodeType: Record<string, unknown>): string {
  return (nodeType as { type?: string }).type ?? "";
}

export interface ExtractedRef {
  key: string;
  ref: { node: string; field: string };
}

/** Extract all OutputRef fields from a NodeType's inner params.
 *  NodeType uses internally-tagged serde: the variant name is in the `type` field
 *  and params are spread as sibling keys. */
export function extractOutputRefs(nodeType: Record<string, unknown>): ExtractedRef[] {
  return Object.entries(nodeType)
    .filter(([key, val]) => key.endsWith("_ref") && val != null)
    .map(([key, val]) => ({ key, ref: val as { node: string; field: string } }));
}

/** Input schemas: which params accept variable refs and what types they accept. */
export const INPUT_SCHEMAS: Record<string, { param: string; acceptedTypes: string[] }[]> = {
  Click: [{ param: "target_ref", acceptedTypes: ["Object"] }],
  Hover: [{ param: "target_ref", acceptedTypes: ["Object"] }],
  Drag: [
    { param: "from_ref", acceptedTypes: ["Object"] },
    { param: "to_ref", acceptedTypes: ["Object"] },
  ],
  TypeText: [{ param: "text_ref", acceptedTypes: ["String", "Number", "Bool"] }],
  FocusWindow: [{ param: "value_ref", acceptedTypes: ["String", "Number"] }],
  CdpFill: [{ param: "value_ref", acceptedTypes: ["String", "Number", "Bool"] }],
  CdpType: [{ param: "text_ref", acceptedTypes: ["String", "Number", "Bool"] }],
  CdpNavigate: [{ param: "url_ref", acceptedTypes: ["String"] }],
  CdpNewPage: [{ param: "url_ref", acceptedTypes: ["String"] }],
  AiStep: [{ param: "prompt_ref", acceptedTypes: ["String", "Number", "Bool"] }],
};

/** Check if a source field type is compatible with a target input param. */
export function isTypeCompatible(sourceFieldType: string, targetNodeType: string, targetInputKey: string): boolean {
  const inputs = INPUT_SCHEMAS[targetNodeType];
  if (!inputs) return false;
  const input = inputs.find((i) => i.param === targetInputKey);
  if (!input) return false;
  return input.acceptedTypes.includes(sourceFieldType);
}

/** Map NodeType variant name to auto_id base string (mirrors Rust auto_id_base). */
const AUTO_ID_BASE: Record<string, string> = {
  FindText: "find_text",
  FindImage: "find_image",
  FindApp: "find_app",
  TakeScreenshot: "take_screenshot",
  Click: "click",
  Hover: "hover",
  Drag: "drag",
  TypeText: "type_text",
  PressKey: "press_key",
  Scroll: "scroll",
  FocusWindow: "focus_window",
  LaunchApp: "launch_app",
  QuitApp: "quit_app",
  CdpClick: "cdp_click",
  CdpHover: "cdp_hover",
  CdpFill: "cdp_fill",
  CdpType: "cdp_type",
  CdpPressKey: "cdp_press_key",
  CdpNavigate: "cdp_navigate",
  CdpNewPage: "cdp_new_page",
  CdpClosePage: "cdp_close_page",
  CdpSelectPage: "cdp_select_page",
  CdpWait: "cdp_wait",
  CdpHandleDialog: "cdp_handle_dialog",
  AiStep: "ai_step",
  If: "if",
  Switch: "switch",
  Loop: "loop",
  EndLoop: "end_loop",
  McpToolCall: "mcp_tool_call",
  AppDebugKitOp: "app_debug_kit_op",
};

/** Reverse map: auto_id base -> NodeType variant name. */
const BASE_TO_NODE_TYPE: Record<string, string> = Object.fromEntries(
  Object.entries(AUTO_ID_BASE).map(([k, v]) => [v, k]),
);

/** Look up the output field type for a given auto_id and field name.
 *  e.g. fieldTypeFromAutoId("find_text_1", "coordinates") -> "Object" */
export function fieldTypeFromAutoId(autoId: string, field: string): string {
  const base = autoId.replace(/_\d+$/, "");
  const nodeType = BASE_TO_NODE_TYPE[base];
  if (!nodeType) return "Any";
  const schema = OUTPUT_SCHEMAS[nodeType];
  return schema?.find((f) => f.name === field)?.type ?? "Any";
}

/** Generate an auto_id for a new node using the workflow's counter map.
 *  Returns [auto_id, updated_counter_value]. The counter map is monotonic —
 *  deleted nodes don't release their IDs, preventing OutputRef retargeting. */
export function generateAutoId(
  nodeTypeName: string,
  counters: Record<string, number>,
): { autoId: string; base: string; counter: number } {
  const base = AUTO_ID_BASE[nodeTypeName] ?? nodeTypeName.toLowerCase().replace(/\s+/g, "_");
  const counter = (counters[base] ?? 0) + 1;
  return { autoId: `${base}_${counter}`, base, counter };
}
