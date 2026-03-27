import { BaseEdge, EdgeLabelRenderer, type EdgeProps, getSmoothStepPath } from "@xyflow/react";
import { typeColor } from "../utils/typeColors";

export function DataEdge({
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  data,
}: EdgeProps) {
  const [edgePath, labelX, labelY] = getSmoothStepPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  });
  const d = data as Record<string, unknown> | undefined;
  const fieldType = (d?.fieldType as string) ?? "Any";
  const fieldName = d?.fieldName as string | undefined;
  const color = typeColor(fieldType);

  return (
    <>
      <BaseEdge
        path={edgePath}
        style={{
          stroke: color,
          strokeWidth: 1.5,
          strokeDasharray: "4 2",
          pointerEvents: "none",
          opacity: 0.6,
        }}
      />
      {fieldName && (
        <EdgeLabelRenderer>
          <span
            className="nodrag nopan pointer-events-none absolute text-[8px] font-mono"
            style={{
              transform: `translate(-50%, -50%) translate(${labelX}px,${labelY}px)`,
              color,
              opacity: 0.8,
            }}
          >
            {fieldName}
          </span>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
