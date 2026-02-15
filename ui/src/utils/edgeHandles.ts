import type { EdgeOutput } from "../bindings";

/**
 * Convert an EdgeOutput to its React Flow handle ID string.
 */
export function edgeOutputToHandle(output: EdgeOutput | null): string | undefined {
  if (!output) return undefined;
  return output.type === "SwitchCase" ? `SwitchCase:${output.name}` : output.type;
}

/**
 * Convert a React Flow handle ID string back to an EdgeOutput.
 */
export function handleToEdgeOutput(handle: string): EdgeOutput | null {
  switch (handle) {
    case "IfTrue": return { type: "IfTrue" };
    case "IfFalse": return { type: "IfFalse" };
    case "SwitchDefault": return { type: "SwitchDefault" };
    case "LoopBody": return { type: "LoopBody" };
    case "LoopDone": return { type: "LoopDone" };
    default:
      if (handle.startsWith("SwitchCase:")) {
        return { type: "SwitchCase", name: handle.slice(11) };
      }
      return null;
  }
}

/**
 * Structural equality for EdgeOutput values.
 * Replaces brittle JSON.stringify comparisons.
 */
export function edgeOutputsEqual(a: EdgeOutput | null | undefined, b: EdgeOutput | null | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  if (a.type !== b.type) return false;
  if (a.type === "SwitchCase" && b.type === "SwitchCase") return a.name === b.name;
  return true;
}
