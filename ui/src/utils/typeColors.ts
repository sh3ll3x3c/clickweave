/** Hex color palette for output field types (for non-CSS contexts like canvas). */
export const TYPE_COLORS: Record<string, string> = {
  Bool:   "#10b981", // green
  Number: "#3b82f6", // blue
  String: "#9ca3af", // light gray
  Array:  "#a855f7", // purple
  Object: "#f59e0b", // orange
  Any:    "#6b7280", // dim gray
};

/** Return a CSS variable reference for the given field type.
 *  Use this in React/HTML style props where CSS custom properties resolve. */
export function typeColor(t: string): string {
  return `var(--type-${t.toLowerCase()})`;
}

/** Return the raw hex color for contexts that cannot resolve CSS variables (e.g. canvas). */
export function typeColorHex(fieldType: string): string {
  return TYPE_COLORS[fieldType] ?? TYPE_COLORS.Any;
}
