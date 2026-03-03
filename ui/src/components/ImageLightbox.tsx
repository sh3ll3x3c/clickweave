import { useCallback, useEffect } from "react";
import { createPortal } from "react-dom";

export interface LightboxImage {
  src: string;
  filename: string;
  crosshair?: { xPercent: number; yPercent: number };
}

export function ImageLightbox({
  images,
  index,
  onClose,
  onNavigate,
}: {
  images: LightboxImage[];
  index: number;
  onClose: () => void;
  onNavigate: (index: number) => void;
}) {
  const current = images[index];
  const hasMultiple = images.length > 1;

  const goPrev = useCallback(() => {
    onNavigate((index - 1 + images.length) % images.length);
  }, [index, images.length, onNavigate]);

  const goNext = useCallback(() => {
    onNavigate((index + 1) % images.length);
  }, [index, images.length, onNavigate]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopImmediatePropagation();
        onClose();
      } else if (e.key === "ArrowLeft" && hasMultiple) goPrev();
      else if (e.key === "ArrowRight" && hasMultiple) goNext();
    };
    // Capture phase so this fires before the global useEscapeKey handler
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [onClose, goPrev, goNext, hasMultiple]);

  if (!current) return null;

  return createPortal(
    <div
      className="fixed inset-0 z-[9999] flex items-center justify-center bg-black/80"
      onClick={onClose}
    >
      <button
        type="button"
        aria-label="Close lightbox"
        onClick={(e) => { e.stopPropagation(); onClose(); }}
        className="absolute top-4 right-4 flex h-8 w-8 items-center justify-center rounded-full bg-white/10 text-white/70 hover:bg-white/20 hover:text-white transition-colors cursor-pointer"
      >
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <path d="M4 4l8 8M12 4l-8 8" />
        </svg>
      </button>

      {hasMultiple && (
        <button
          type="button"
          aria-label="Previous image"
          onClick={(e) => { e.stopPropagation(); goPrev(); }}
          className="absolute left-4 flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-white/70 hover:bg-white/20 hover:text-white transition-colors cursor-pointer"
        >
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 4l-6 6 6 6" />
          </svg>
        </button>
      )}

      <div
        className="flex flex-col items-center gap-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="relative">
          <img
            src={current.src}
            alt={current.filename}
            className="max-h-[85vh] max-w-[90vw] rounded object-contain"
            draggable={false}
          />
          {current.crosshair && (
            <CrosshairOverlay xPercent={current.crosshair.xPercent} yPercent={current.crosshair.yPercent} />
          )}
        </div>
        <span className="text-xs text-white/60">
          {current.filename}
          {hasMultiple && (
            <span className="ml-2 text-white/40">
              {index + 1} / {images.length}
            </span>
          )}
        </span>
      </div>

      {hasMultiple && (
        <button
          type="button"
          aria-label="Next image"
          onClick={(e) => { e.stopPropagation(); goNext(); }}
          className="absolute right-4 flex h-10 w-10 items-center justify-center rounded-full bg-white/10 text-white/70 hover:bg-white/20 hover:text-white transition-colors cursor-pointer"
        >
          <svg width="20" height="20" viewBox="0 0 20 20" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M8 4l6 6-6 6" />
          </svg>
        </button>
      )}
    </div>,
    document.body,
  );
}

export function CrosshairOverlay({ xPercent, yPercent }: { xPercent: number; yPercent: number }) {
  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      {/* Horizontal line */}
      <div
        className="absolute left-0 right-0 h-px"
        style={{ top: `${yPercent}%`, backgroundColor: "var(--accent-coral)", opacity: 0.6 }}
      />
      {/* Vertical line */}
      <div
        className="absolute top-0 bottom-0 w-px"
        style={{ left: `${xPercent}%`, backgroundColor: "var(--accent-coral)", opacity: 0.6 }}
      />
      {/* Center dot */}
      <div
        className="absolute h-2 w-2 rounded-full"
        style={{
          left: `${xPercent}%`,
          top: `${yPercent}%`,
          transform: "translate(-50%, -50%)",
          backgroundColor: "var(--accent-coral)",
          opacity: 0.9,
          boxShadow: "0 0 0 2px rgba(0,0,0,0.4)",
        }}
      />
    </div>
  );
}
