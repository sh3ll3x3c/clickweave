import { useState, useRef, useCallback } from "react";

/**
 * Reusable hook for horizontal panel resizing (drag left edge).
 * Returns current width and a mousedown handler for the resize handle.
 */
export function useHorizontalResize(initialWidth = 380, min = 280, max = 600) {
  const [width, setWidth] = useState(initialWidth);
  const widthRef = useRef(width);
  widthRef.current = width;

  const handleResizeStart = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startWidth = widthRef.current;
    document.body.style.userSelect = "none";

    const onMove = (e: MouseEvent) => {
      e.preventDefault();
      setWidth(Math.min(max, Math.max(min, startWidth + (startX - e.clientX))));
    };
    const onUp = () => {
      document.body.style.userSelect = "";
      document.removeEventListener("mousemove", onMove);
      document.removeEventListener("mouseup", onUp);
    };

    document.addEventListener("mousemove", onMove);
    document.addEventListener("mouseup", onUp);
  }, [min, max]);

  return { width, handleResizeStart };
}
