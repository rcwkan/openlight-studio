import { useState, useEffect, useRef, useCallback } from "react";
import { Photo, thumbUrl, Culling, LABEL_COLORS } from "./api";
import { Stars } from "./controls";

export interface CompareGroup {
  id?: string | number;
  title?: string;
  kind?: "exact" | "similar" | "manual";
  photos: Photo[];
  keepId?: number;
}

export interface CompareModalProps {
  groups: CompareGroup[];
  initialIndex?: number;
  onClose: () => void;
  onSetKeep?: (groupIndex: number, photoId: number) => void;
  onCull?: (photoId: number, culling: Culling) => void;
  onMoveToBin?: (photoId: number) => void;
}

export function CompareModal({
  groups,
  initialIndex = 0,
  onClose,
  onSetKeep,
  onCull,
  onMoveToBin,
}: CompareModalProps) {
  const [groupIndex, setGroupIndex] = useState(initialIndex);
  const [zoom, setZoom] = useState(false);
  const [pan, setPan] = useState({ x: 0.5, y: 0.5 });
  const [leftIndex, setLeftIndex] = useState(0);
  const [rightIndex, setRightIndex] = useState(1);
  const [localKeeps, setLocalKeeps] = useState<Record<number, number>>({});
  const [notification, setNotification] = useState<string>("");

  const curGroup = groups[groupIndex] || groups[0];
  const curPhotos = curGroup?.photos || [];

  if (!groups || groups.length === 0 || curPhotos.length === 0) {
    return null;
  }

  // Reset candidate indices when group changes
  useEffect(() => {
    setLeftIndex(0);
    setRightIndex(Math.min(1, curPhotos.length - 1));
  }, [groupIndex, curPhotos.length]);

  const pA = curPhotos[leftIndex];
  const pB = curPhotos[rightIndex] || curPhotos[0];
  const currentKeep = localKeeps[groupIndex] ?? curGroup?.keepId ?? pA?.id;

  const showNote = (msg: string) => {
    setNotification(msg);
    setTimeout(() => setNotification(""), 3000);
  };

  const handleSetKeep = useCallback(
    (photoId: number) => {
      setLocalKeeps((prev) => ({ ...prev, [groupIndex]: photoId }));
      if (onSetKeep) {
        onSetKeep(groupIndex, photoId);
      }
      const keptPhoto = curPhotos.find((p) => p.id === photoId);
      showNote(`Kept ${keptPhoto?.filename || photoId}`);
    },
    [groupIndex, curPhotos, onSetKeep]
  );

  const handleCull = (photoId: number, u: Culling) => {
    if (onCull) {
      onCull(photoId, u);
      showNote(`Updated status for photo`);
    }
  };

  const handleBin = (photoId: number) => {
    if (onMoveToBin) {
      onMoveToBin(photoId);
      showNote(`Moved photo to Bin`);
    }
  };

  const handleQuickDecide = (keepSide: "left" | "right", actionOther?: "reject" | "bin") => {
    const keepPhoto = keepSide === "left" ? pA : pB;
    const otherPhoto = keepSide === "left" ? pB : pA;
    if (!keepPhoto) return;

    handleSetKeep(keepPhoto.id);
    if (onCull) {
      onCull(keepPhoto.id, { flag: "pick" });
    }

    if (otherPhoto && otherPhoto.id !== keepPhoto.id) {
      if (actionOther === "bin" && onMoveToBin) {
        onMoveToBin(otherPhoto.id);
      } else if (actionOther === "reject" && onCull) {
        onCull(otherPhoto.id, { flag: "reject" });
      }
    }

    // Auto advance to next group if available
    if (groupIndex < groups.length - 1) {
      setTimeout(() => setGroupIndex((i) => i + 1), 250);
    }
  };

  // Keyboard navigation and shortcuts
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      if (e.key === "Escape") {
        onClose();
      } else if (e.key === "ArrowLeft") {
        setGroupIndex((i) => Math.max(0, i - 1));
      } else if (e.key === "ArrowRight") {
        setGroupIndex((i) => Math.min(groups.length - 1, i + 1));
      } else if (e.key === "1" && pA) {
        handleSetKeep(pA.id);
      } else if (e.key === "2" && pB) {
        handleSetKeep(pB.id);
      } else if (e.key === "z" || e.key === "Z") {
        setZoom((z) => !z);
      }
    };
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  }, [groups.length, pA, pB, handleSetKeep, onClose]);

  // Synchronized panning across both viewports
  const handleMouseMove = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!zoom) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const x = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const y = Math.max(0, Math.min(1, (e.clientY - rect.top) / rect.height));
    setPan({ x, y });
  };

  if (!curGroup || !pA) {
    return null;
  }

  const isAKept = currentKeep === pA.id;
  const isBKept = currentKeep === pB?.id;

  return (
    <div className="fixed inset-0 z-50 flex flex-col bg-neutral-950/95 text-neutral-100 backdrop-blur-md select-none">
      {/* Top Header Bar */}
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-neutral-800 bg-neutral-900/90 px-4">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <span className="flex h-6 w-6 items-center justify-center rounded bg-sky-500/20 text-sky-400 text-xs font-bold">
              ⧉
            </span>
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-200">
              Side-by-Side Compare to Decide
            </span>
          </div>

          <span className="text-neutral-600">|</span>

          {/* Group details badge */}
          <div className="flex items-center gap-2 text-xs">
            <span
              className={`rounded px-2 py-0.5 text-[10px] font-semibold uppercase ${
                curGroup.kind === "exact"
                  ? "bg-amber-500/20 text-amber-300 border border-amber-500/40"
                  : "bg-sky-500/20 text-sky-300 border border-sky-500/40"
              }`}
            >
              {curGroup.kind === "exact" ? "Exact Duplicate" : curGroup.kind === "manual" ? "Selection" : "Look-alike"}
            </span>
            <span className="text-neutral-400 font-mono text-[11px]">
              Group {groupIndex + 1} of {groups.length}
            </span>
          </div>
        </div>

        {/* Action feedback toast */}
        {notification && (
          <span className="rounded bg-emerald-500/20 border border-emerald-500/40 px-3 py-1 text-xs text-emerald-300 animate-pulse font-medium">
            ✓ {notification}
          </span>
        )}

        {/* Navigation & Controls */}
        <div className="flex items-center gap-2">
          {/* Zoom toggle button */}
          <button
            onClick={() => setZoom(!zoom)}
            className={`flex items-center gap-1.5 rounded px-2.5 py-1 text-xs font-medium border transition-colors ${
              zoom
                ? "bg-sky-600 text-white border-sky-500 shadow-xs"
                : "bg-neutral-800 text-neutral-300 border-neutral-700 hover:bg-neutral-700 hover:text-white"
            }`}
            title="Toggle 100% 1:1 Pixel Inspection (Z)"
          >
            <span>🔍</span>
            <span>{zoom ? "1:1 Zoomed (Fit: Z)" : "Fit (1:1: Z)"}</span>
          </button>

          {/* Prev / Next buttons */}
          <div className="flex items-center rounded border border-neutral-700 bg-neutral-800">
            <button
              disabled={groupIndex <= 0}
              onClick={() => setGroupIndex((i) => Math.max(0, i - 1))}
              className="px-2.5 py-1 text-xs text-neutral-300 hover:text-white hover:bg-neutral-700 disabled:opacity-30 disabled:hover:bg-transparent"
              title="Previous group (←)"
            >
              ◀ Prev
            </button>
            <span className="text-neutral-600 text-[11px] px-1">|</span>
            <button
              disabled={groupIndex >= groups.length - 1}
              onClick={() => setGroupIndex((i) => Math.min(groups.length - 1, i + 1))}
              className="px-2.5 py-1 text-xs text-neutral-300 hover:text-white hover:bg-neutral-700 disabled:opacity-30 disabled:hover:bg-transparent"
              title="Next group (→)"
            >
              Next ▶
            </button>
          </div>

          <button
            onClick={onClose}
            className="flex h-7 w-7 items-center justify-center rounded-md bg-neutral-800 text-neutral-400 hover:bg-neutral-700 hover:text-white transition-colors"
            title="Close Compare (Esc)"
          >
            ✕
          </button>
        </div>
      </header>

      {/* Quick Decision Top Strip */}
      <div className="flex flex-wrap items-center justify-between border-b border-neutral-800/80 bg-neutral-900/60 px-4 py-1.5 text-xs">
        <div className="flex items-center gap-2">
          <span className="text-[11px] text-neutral-400">Quick Decision:</span>
          <button
            onClick={() => handleQuickDecide("left", "reject")}
            className="rounded bg-neutral-800 px-2.5 py-1 text-[11px] font-medium text-emerald-400 border border-neutral-700/80 hover:bg-emerald-950/40 hover:border-emerald-600/60"
            title="Keep Left photo and flag Right photo as reject [1]"
          >
            ✓ Keep Left & Reject Right
          </button>
          <button
            onClick={() => handleQuickDecide("right", "reject")}
            className="rounded bg-neutral-800 px-2.5 py-1 text-[11px] font-medium text-emerald-400 border border-neutral-700/80 hover:bg-emerald-950/40 hover:border-emerald-600/60"
            title="Keep Right photo and flag Left photo as reject [2]"
          >
            ✓ Keep Right & Reject Left
          </button>
          <button
            onClick={() => handleQuickDecide("left", "bin")}
            className="rounded bg-neutral-800 px-2.5 py-1 text-[11px] font-medium text-red-400 border border-neutral-700/80 hover:bg-red-950/40 hover:border-red-600/60"
            title="Keep Left photo and move Right photo to Bin"
          >
            🗑️ Keep Left & Trash Right
          </button>
          <button
            onClick={() => handleQuickDecide("right", "bin")}
            className="rounded bg-neutral-800 px-2.5 py-1 text-[11px] font-medium text-red-400 border border-neutral-700/80 hover:bg-red-950/40 hover:border-red-600/60"
            title="Keep Right photo and move Left photo to Bin"
          >
            🗑️ Keep Right & Trash Left
          </button>
        </div>

        <div className="flex items-center gap-2 text-[11px] text-neutral-500">
          <span>Shortcuts:</span>
          <kbd className="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-300 border border-neutral-700">1</kbd> Keep Left
          <kbd className="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-300 border border-neutral-700">2</kbd> Keep Right
          <kbd className="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-300 border border-neutral-700">Z</kbd> Zoom 1:1
          <kbd className="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-300 border border-neutral-700">←/→</kbd> Navigate
        </div>
      </div>

      {/* Main Side-by-Side Viewport */}
      <div className="flex flex-1 min-h-0 overflow-hidden">
        {/* Left Photo Pane */}
        <ComparePane
          photo={pA}
          isKept={isAKept}
          side="left"
          zoom={zoom}
          pan={pan}
          onMouseMove={handleMouseMove}
          onSetKeep={() => handleSetKeep(pA.id)}
          onCull={(u) => handleCull(pA.id, u)}
          onMoveToBin={() => handleBin(pA.id)}
          otherPhoto={pB}
          allPhotos={curPhotos}
          currentIndex={leftIndex}
          onSelectIndex={setLeftIndex}
        />

        <div className="w-[1px] bg-neutral-800 shrink-0 shadow-lg" />

        {/* Right Photo Pane */}
        <ComparePane
          photo={pB}
          isKept={isBKept}
          side="right"
          zoom={zoom}
          pan={pan}
          onMouseMove={handleMouseMove}
          onSetKeep={() => handleSetKeep(pB.id)}
          onCull={(u) => handleCull(pB.id, u)}
          onMoveToBin={() => handleBin(pB.id)}
          otherPhoto={pA}
          allPhotos={curPhotos}
          currentIndex={rightIndex}
          onSelectIndex={setRightIndex}
        />
      </div>
    </div>
  );
}

function ComparePane({
  photo,
  isKept,
  side,
  zoom,
  pan,
  onMouseMove,
  onSetKeep,
  onCull,
  onMoveToBin,
  otherPhoto,
  allPhotos,
  currentIndex,
  onSelectIndex,
}: {
  photo: Photo;
  isKept: boolean;
  side: "left" | "right";
  zoom: boolean;
  pan: { x: number; y: number };
  onMouseMove: (e: React.MouseEvent<HTMLDivElement>) => void;
  onSetKeep: () => void;
  onCull: (u: Culling) => void;
  onMoveToBin: () => void;
  otherPhoto?: Photo;
  allPhotos: Photo[];
  currentIndex: number;
  onSelectIndex: (index: number) => void;
}) {
  const containerRef = useRef<HTMLDivElement>(null);

  if (!photo) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center p-8 text-neutral-500">
        <p>No photo selected for this pane</p>
      </div>
    );
  }

  // EXIF comparison helpers
  const isDiff = (valA: any, valB: any) => valA != null && valB != null && valA !== valB;

  const shutterA = photo.shutter;
  const shutterB = otherPhoto?.shutter;
  const isoA = photo.iso;
  const isoB = otherPhoto?.iso;
  const apertureA = photo.aperture;
  const apertureB = otherPhoto?.aperture;
  const sizeA = photo.width && photo.height ? `${photo.width} × ${photo.height}` : null;
  const sizeB = otherPhoto?.width && otherPhoto?.height ? `${otherPhoto.width} × ${otherPhoto.height}` : null;

  return (
    <div
      className={`flex flex-1 flex-col min-w-0 transition-colors ${
        isKept ? "bg-emerald-950/10" : "bg-neutral-950/60"
      }`}
    >
      {/* Pane Header */}
      <div className="flex items-center justify-between border-b border-neutral-800/80 bg-neutral-900/80 px-4 py-2">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[10px] uppercase font-bold text-neutral-400">
            {side === "left" ? "Candidate A (Left)" : "Candidate B (Right)"}:
          </span>
          <span className="truncate text-xs font-semibold text-neutral-100" title={photo.filename}>
            {photo.filename}
          </span>
          {photo.copy_name && (
            <span className="rounded bg-sky-950/80 text-sky-300 border border-sky-800/80 px-1.5 text-[10px]">
              {photo.copy_name}
            </span>
          )}
          {photo.edited && (
            <span className="rounded bg-neutral-800 text-amber-300 px-1 text-[10px]" title="Edited">
              ±
            </span>
          )}
        </div>

        {/* Candidate Selector (if group has > 2 photos) */}
        {allPhotos.length > 2 && (
          <div className="flex items-center gap-1 text-[11px]">
            <span className="text-neutral-500">Pick:</span>
            <select
              value={currentIndex}
              onChange={(e) => onSelectIndex(+e.target.value)}
              className="rounded bg-neutral-800 border border-neutral-700 px-1 py-0.5 text-xs text-neutral-200 outline-none"
            >
              {allPhotos.map((p, idx) => (
                <option key={p.id} value={idx}>
                  #{idx + 1}: {p.filename}
                </option>
              ))}
            </select>
          </div>
        )}

        {/* Decision Keep Indicator */}
        <div>
          {isKept ? (
            <span className="flex items-center gap-1 rounded-full bg-emerald-500/20 border border-emerald-500/50 px-2.5 py-0.5 text-[11px] font-semibold text-emerald-400 shadow-xs">
              ✓ KEPT AS BEST
            </span>
          ) : (
            <button
              onClick={onSetKeep}
              className="rounded bg-neutral-800 hover:bg-emerald-600 hover:text-white px-2.5 py-0.5 text-[11px] font-medium text-neutral-300 border border-neutral-700 transition-colors"
            >
              Mark as Keep
            </button>
          )}
        </div>
      </div>

      {/* Image Preview Canvas */}
      <div
        ref={containerRef}
        onMouseMove={onMouseMove}
        className={`relative flex flex-1 items-center justify-center overflow-hidden p-3 cursor-${
          zoom ? "crosshair" : "default"
        }`}
      >
        <div className="relative flex h-full w-full items-center justify-center">
          <img
            src={thumbUrl(photo)}
            alt={photo.filename}
            draggable={false}
            style={{
              transformOrigin: `${pan.x * 100}% ${pan.y * 100}%`,
              transform: zoom ? "scale(2.5)" : "none",
              transition: zoom ? "none" : "transform 0.15s ease",
            }}
            className="max-h-full max-w-full object-contain rounded shadow-lg select-none"
          />

          {/* Quick status stamp over the image */}
          <div className="absolute top-2 left-2 flex items-center gap-1.5 rounded-md bg-black/60 backdrop-blur-xs px-2 py-1 text-[10px] text-neutral-300 border border-white/10 shadow-sm">
            {photo.flag === 1 && <span className="text-emerald-400 font-semibold">⚑ Pick</span>}
            {photo.flag === -1 && <span className="text-red-400 font-semibold">✕ Reject</span>}
            {photo.rating > 0 && <span className="text-amber-300 font-bold">★ {photo.rating}</span>}
            {photo.color_label && photo.color_label !== "none" && (
              <span
                className="h-2 w-2 rounded-full shadow-xs"
                style={{ backgroundColor: LABEL_COLORS[photo.color_label] }}
              />
            )}
            <span>{photo.filename}</span>
          </div>
        </div>
      </div>

      {/* Decision Action Toolbar */}
      <div className="flex items-center justify-between border-t border-neutral-800/80 bg-neutral-900/90 px-4 py-2">
        <div className="flex items-center gap-2">
          <button
            onClick={onSetKeep}
            className={`flex items-center gap-1.5 rounded px-3 py-1.5 text-xs font-semibold shadow-xs transition-all ${
              isKept
                ? "bg-emerald-600 text-white ring-2 ring-emerald-500/40"
                : "bg-neutral-800 text-neutral-200 border border-neutral-700 hover:bg-emerald-700 hover:text-white"
            }`}
          >
            <span>{isKept ? "✓ Kept" : "✓ Choose to Keep"}</span>
          </button>

          <div className="flex items-center rounded border border-neutral-700 bg-neutral-800 text-[11px]">
            <button
              onClick={() => onCull({ flag: photo.flag === 1 ? "unflagged" : "pick" })}
              className={`px-2 py-1 transition-colors ${
                photo.flag === 1 ? "bg-emerald-600 text-white" : "text-neutral-300 hover:text-white"
              }`}
              title="Toggle Pick"
            >
              ⚑ Pick
            </button>
            <span className="text-neutral-600">|</span>
            <button
              onClick={() => onCull({ flag: photo.flag === -1 ? "unflagged" : "reject" })}
              className={`px-2 py-1 transition-colors ${
                photo.flag === -1 ? "bg-red-600 text-white" : "text-neutral-300 hover:text-white"
              }`}
              title="Toggle Reject"
            >
              ✕ Reject
            </button>
          </div>

          <div className="flex items-center px-1">
            <Stars value={photo.rating} onChange={(r) => onCull({ rating: r })} />
          </div>
        </div>

        <div>
          <button
            onClick={onMoveToBin}
            className="flex items-center gap-1 rounded bg-red-950/40 border border-red-800/60 px-2.5 py-1 text-xs text-red-300 hover:bg-red-900 hover:text-white transition-colors"
            title="Move this photo to Bin"
          >
            <span>🗑️</span>
            <span>Move to Bin</span>
          </button>
        </div>
      </div>

      {/* EXIF & Metadata Comparative Table */}
      <div className="border-t border-neutral-800/80 bg-neutral-950/80 p-3 text-[11px]">
        <div className="grid grid-cols-2 gap-x-3 gap-y-1 sm:grid-cols-4 font-mono">
          <div className="flex flex-col">
            <span className="text-[10px] text-neutral-500 uppercase">Resolution</span>
            <span
              className={`font-medium ${
                isDiff(sizeA, sizeB) ? "text-amber-300 font-semibold" : "text-neutral-300"
              }`}
            >
              {sizeA || "Unknown"}
            </span>
          </div>

          <div className="flex flex-col">
            <span className="text-[10px] text-neutral-500 uppercase">Shutter</span>
            <span
              className={`font-medium ${
                isDiff(shutterA, shutterB) ? "text-amber-300 font-semibold" : "text-neutral-300"
              }`}
            >
              {shutterA ? `${shutterA} s` : "—"}
            </span>
          </div>

          <div className="flex flex-col">
            <span className="text-[10px] text-neutral-500 uppercase">Aperture</span>
            <span
              className={`font-medium ${
                isDiff(apertureA, apertureB) ? "text-amber-300 font-semibold" : "text-neutral-300"
              }`}
            >
              {apertureA ? `f/${apertureA.toFixed(1)}` : "—"}
            </span>
          </div>

          <div className="flex flex-col">
            <span className="text-[10px] text-neutral-500 uppercase">ISO</span>
            <span
              className={`font-medium ${
                isDiff(isoA, isoB) ? "text-amber-300 font-semibold" : "text-neutral-300"
              }`}
            >
              {isoA ? `ISO ${isoA}` : "—"}
            </span>
          </div>
        </div>

        <div className="mt-2 flex items-center justify-between border-t border-neutral-800/60 pt-1.5 text-[10px] text-neutral-400">
          <span className="truncate" title={photo.folder}>
            📁 {photo.folder}
          </span>
          <span>{photo.captured_at || "No capture time"}</span>
        </div>
      </div>
    </div>
  );
}
