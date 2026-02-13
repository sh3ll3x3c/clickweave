import type { NodeRun } from "../../bindings";

export function runDuration(run: NodeRun): string | null {
  if (!run.ended_at) return null;
  return ((run.ended_at - run.started_at) / 1000).toFixed(1);
}

const eventTypeColors: Record<string, string> = {
  node_started: "bg-blue-500/20 text-blue-400",
  tool_call: "bg-purple-500/20 text-purple-400",
  tool_result: "bg-green-500/20 text-green-400",
  retry: "bg-yellow-500/20 text-yellow-400",
};

export function eventTypeColor(eventType: string): string {
  return eventTypeColors[eventType] ?? "bg-[var(--bg-hover)] text-[var(--text-secondary)]";
}

const payloadFormatters: [string, (v: unknown) => string][] = [
  ["name", (v) => String(v)],
  ["type", (v) => String(v)],
  ["error", (v) => `error: ${v}`],
  ["attempt", (v) => `attempt ${v}`],
  ["text_len", (v) => `${v} chars`],
  ["image_count", (v) => `${v} images`],
];

export function formatEventPayload(payload: unknown): string {
  if (payload == null) return "";
  if (typeof payload === "string") return payload;
  if (typeof payload !== "object") return String(payload);

  const obj = payload as Record<string, unknown>;
  const parts = payloadFormatters
    .filter(([key]) => obj[key])
    .map(([key, fmt]) => fmt(obj[key]));
  return parts.join(" | ") || JSON.stringify(payload);
}

/** Keys to omit from the detail view (shown in summary already or too noisy). */
const detailOmitKeys = new Set(["text_len", "image_count"]);

export function formatEventDetail(payload: unknown): string {
  if (payload == null) return "(empty)";
  if (typeof payload === "string") return payload;
  if (typeof payload !== "object") return String(payload);

  const obj = payload as Record<string, unknown>;
  const filtered: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) {
    if (!detailOmitKeys.has(k)) filtered[k] = v;
  }
  return JSON.stringify(filtered, null, 2);
}
