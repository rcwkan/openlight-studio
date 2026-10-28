import { listen } from "@tauri-apps/api/event";
import { Fragment, ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { Log } from "./Agent";
import { api, Develop as Dev, getPath, HistoryEntry, Mask, MaskComponent, Photo, Rendered, setPath, Settings, srgbToOklch, thumbUrl } from "./api";
import { Btn, ColorWheel, CurveEditor, Histogram, Panel, Seg, Slider } from "./controls";

type Props = {
  photos: Photo[];
  active: Photo | null;
  selected: number[];
  setActive: (id: number) => void;
  defaults: Dev;
  settings: Settings | null;
  setSettings: (s: Settings) => void;
  onExport: () => void;
  logs: Log[];
  clearLogs?: () => void;
};

type Tool = "none" | "crop" | "swatch" | "maskcolor" | "select";
type Path = (string | number)[];

const BANDS: [string, string][] = [
  ["Red", "#e5484d"],
  ["Orange", "#f76808"],
  ["Yellow", "#f5d90a"],
  ["Green", "#46a758"],
  ["Aqua", "#12a594"],
  ["Blue", "#3e63dd"],
  ["Purple", "#8e4ec6"],
  ["Magenta", "#d6409f"],
];

const COMPONENT: MaskComponent = {
  op: "add",
  type: "linear_gradient",
  invert: false,
  feather: 0,
  edge_shift: 0,
  start: [0.5, 0.0],
  end: [0.5, 0.5],
  center: [0.5, 0.5],
  radius: [0.3, 0.3],
  range: [0.5, 1.0],
  color: [0.5, 0.5, 0.5],
  tolerance: 50,
  points: [],
};
const MASK_TYPES: [string, string][] = [
  ["linear_gradient", "Linear gradient"],
  ["radial_gradient", "Radial gradient"],
  ["luminance_range", "Luminance range"],
  ["color_range", "Color range"],
];
/** Polygons only come from the AI bar, so they are labelled but not offered in the add lists. */
const TYPE_LABEL: Record<string, string> = { ...Object.fromEntries(MASK_TYPES), polygon: "Object (AI outline)" };
const ASPECTS: [string, number | null][] = [
  ["Free", null],
  ["Original", 0],
  ["1:1", 1],
  ["4:5", 4 / 5],
  ["3:2", 3 / 2],
  ["16:9", 16 / 9],
];

/** Settings copied between photos (crop, removals and provenance stay per photo). */
let clipboard: Dev | null = null;
const paste = (target: Dev, src: Dev): Dev => ({ ...structuredClone(src), crop: target.crop, retouch: target.retouch, ai_provenance_log: target.ai_provenance_log });
const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
const srgbEotf = (v: number) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

export function Develop(p: Props) {
  const id = p.active?.id ?? null;
  const [dev, setDev] = useState<Dev | null>(null);
  const [hist, setHist] = useState<Uint32Array[] | null>(null);
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [before, setBefore] = useState(false);
  const [tool, setTool] = useState<Tool>("none");
  const [aspect, setAspect] = useState<number | null>(null);
  const [maskSel, setMaskSel] = useState<number | null>(null);
  const [compSel, setCompSel] = useState(0);
  const [swatchSel, setSwatchSel] = useState<number | null>(null);
  const [overlay, setOverlay] = useState(false);
  /** AI Select: where the click being traced was (normalised) */
  const [picking, setPicking] = useState<{ x: number; y: number } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [imgSize, setImgSize] = useState({ w: 3, h: 2 });
  const [box, setBox] = useState({ w: 0, h: 0 });
  const canvas = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const boxRef = useRef<HTMLDivElement>(null);

  // --- edit state, debounced commits (one history step per gesture) ---------------------
  const devRef = useRef(dev);
  devRef.current = dev;
  const idRef = useRef(id);
  idRef.current = id;
  const pending = useRef<{ id: number; d: Dev; label: string; timer: number } | null>(null);

  const refreshHistory = useCallback(async (pid: number) => setHistory(await api.history(pid)), []);
  const flush = useCallback(async () => {
    const pd = pending.current;
    if (!pd) return;
    clearTimeout(pd.timer);
    pending.current = null;
    await api.save(pd.id, pd.d, pd.label);
    if (pd.id === idRef.current) refreshHistory(pd.id);
  }, [refreshHistory]);
  const change = useCallback(
    (d: Dev, label: string) => {
      if (idRef.current == null) return;
      setDev(d);
      if (pending.current) clearTimeout(pending.current.timer);
      pending.current = { id: idRef.current, d, label, timer: window.setTimeout(flush, 450) };
    },
    [flush],
  );

  useEffect(() => {
    if (id == null) return;
    let live = true;
    setDev(null);
    setLoading(true);
    setMaskSel(null);
    setSwatchSel(null);
    setTool("none");
    flush().then(() => api.getDevelop(id).then((d) => live && setDev(d)));
    refreshHistory(id);
    return () => {
      live = false;
    };
  }, [id, flush, refreshHistory]);
  useEffect(() => () => void flush(), [flush]);

  // edits from MCP agents / AI providers land here
  useEffect(() => {
    const un = listen<{ id: number }>("photo-changed", async (e) => {
      if (e.payload.id !== idRef.current || pending.current) return;
      const d = await api.getDevelop(e.payload.id);
      if (JSON.stringify(d) !== JSON.stringify(devRef.current)) setDev(d);
      refreshHistory(e.payload.id);
    });
    return () => void un.then((f) => f());
  }, [refreshHistory]);

  // --- render loop: latest request wins, one render in flight --------------------------
  const job = useRef<{ id: number; d: Dev; skip: boolean; overlay: number | null; full: boolean } | null>(null);
  const busy = useRef(false);
  const draw = (r: Rendered) => {
    const c = canvas.current;
    if (!c) return;
    if (c.width !== r.image.width || c.height !== r.image.height) {
      c.width = r.image.width;
      c.height = r.image.height;
      setImgSize({ w: r.image.width, h: r.image.height });
    }
    c.getContext("2d")!.putImageData(r.image, 0, 0);
    setHist(r.hist);
  };
  const pump = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    while (job.current) {
      const j = job.current;
      job.current = null;
      try {
        draw(await api.render(j.id, j.d, j.skip, j.overlay, j.full));
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    }
    busy.current = false;
    setLoading(false);
  }, []);

  const mask = dev && maskSel != null ? dev.masks[maskSel] : null;
  const handlesOn = !!mask && mask.components.some((c) => c.type === "linear_gradient" || c.type === "radial_gradient");
  const skipCrop = tool === "crop" || handlesOn;
  // AI Select shows the uncropped frame, so a click is already in mask coordinates
  const fullFrame = tool === "select";
  useEffect(() => {
    if (id == null || !dev) return;
    job.current = { id, d: before ? p.defaults : dev, skip: skipCrop && !before, overlay: overlay && !before ? maskSel : null, full: fullFrame };
    pump();
  }, [id, dev, before, skipCrop, fullFrame, overlay, maskSel, p.defaults, pump]);

  useEffect(() => {
    const ro = new ResizeObserver(([e]) => setBox({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(boxRef.current!);
    return () => ro.disconnect();
  }, []);
  const scale = Math.min((box.w - 24) / imgSize.w, (box.h - 24) / imgSize.h, 1.5);
  const disp = { w: Math.max(1, imgSize.w * scale), h: Math.max(1, imgSize.h * scale) };

  // --- history, undo, copy/paste, sync -------------------------------------------------
  const undo = async (dir: number) => {
    if (id == null) return;
    await flush();
    const d = await api.undoRedo(id, dir);
    if (d) setDev(d);
    refreshHistory(id);
  };
  // AI bar: a new mask from the run (e.g. "select the castle") is selected with its overlay on
  const aiRun = async (prompt: string, provider: string) => {
    if (id == null) throw "No photo";
    await flush();
    const masks = devRef.current?.masks.length ?? 0;
    const text = await api.aiRun(id, prompt, provider);
    const d = await api.getDevelop(id);
    if (idRef.current === id) {
      setDev(d);
      refreshHistory(id);
      if (d.masks.length > masks) (setMaskSel(d.masks.length - 1), setCompSel(0), setOverlay(true));
    }
    return text;
  };
  // AI Select: the clicked object joins the selected mask (⌥ subtracts it) or starts a new one
  const aiSelect = async (x: number, y: number, subtract: boolean) => {
    if (id == null || !p.settings || picking) return;
    await flush();
    setPicking({ x, y });
    setError(null);
    try {
      const r = await api.aiSelect(id, x, y, p.settings.active_provider, maskSel, subtract);
      if (idRef.current === id) (setDev(r.develop), setMaskSel(r.mask), setCompSel(0), setOverlay(true), refreshHistory(id));
    } catch (e) {
      setError(`AI Select: ${e}`);
    } finally {
      setPicking(null);
    }
  };

  const others = p.selected.filter((s) => s !== id);
  const sync = async () => {
    if (!dev) return;
    await flush();
    for (const o of others) await api.save(o, paste(await api.getDevelop(o), dev), "Sync settings");
  };

  const keys = useRef<(e: KeyboardEvent) => void>(() => {});
  keys.current = (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "z") (e.preventDefault(), undo(e.shiftKey ? 1 : -1));
    else if (mod && e.shiftKey && e.key.toLowerCase() === "c" && dev) (e.preventDefault(), (clipboard = structuredClone(dev)));
    else if (mod && e.shiftKey && e.key.toLowerCase() === "v" && dev && clipboard) (e.preventDefault(), change(paste(dev, clipboard), "Paste settings"));
    else if (mod) return;
    else if (e.key === "\\") setBefore((b) => !b);
    else if (e.key.toLowerCase() === "r") setTool((t) => (t === "crop" ? "none" : "crop"));
    else if (e.key.toLowerCase() === "s") setTool((t) => (t === "select" ? "none" : "select"));
    else if (e.key.toLowerCase() === "o" && maskSel != null) setOverlay((o) => !o);
    else if (e.key === "Escape") (setTool("none"), setMaskSel(null));
  };
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (!["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName)) keys.current(e);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);
  useEffect(() => {
    document.getElementById(`fs-${id}`)?.scrollIntoView({ inline: "center", block: "nearest" });
  }, [id]);

  const cropStart = useRef<Dev["crop"] | null>(null);
  const handleStart = useRef<MaskComponent | null>(null);
  const curveKey = useRef<"point_luma" | "point_red" | "point_green" | "point_blue">("point_luma");
  const [, force] = useState(0);
  const [mixerTab, setMixerTab] = useState<"hue" | "saturation" | "luminance">("hue");

  if (!p.active) return <div className="flex flex-1 items-center justify-center text-neutral-500">Select a photo in the Library first.</div>;

  // --- helpers bound to the current state -----------------------------------------------
  // While a mask is selected every panel tunes inside it: the mask's own settings (a delta
  // that starts at the defaults), applied on top of the photo.
  const T = mask ? (mask.tuning ?? p.defaults) : dev;
  const edit = (t: Dev, label: string) => {
    if (dev && mask && maskSel != null) (setOverlay(false), change(setPath(dev, ["masks", maskSel, "tuning"], t), `${mask.name}: ${label}`));
    else change(t, label);
  };
  const slider =
    (root: Dev | null, write: (d: Dev, label: string) => void) =>
    (label: string, path: Path, min: number, max: number, extra: Partial<Parameters<typeof Slider>[0]> = {}) =>
      root && (
        <Slider
          key={path.join(".")}
          label={label}
          value={getPath(root, path) as number}
          def={(getPath(p.defaults, path) as number | undefined) ?? 0}
          min={min}
          max={max}
          onChange={(v) => write(setPath(root, path, v), label)}
          {...extra}
        />
      );
  const S = slider(T, edit);
  const reset = (key: keyof Dev, label: string) => (
    <button className="text-[10px] font-normal normal-case text-neutral-500 hover:text-neutral-200" onClick={() => T && edit({ ...T, [key]: structuredClone(p.defaults[key]) }, `Reset ${label}`)}>
      reset
    </button>
  );

  const sample = (e: React.MouseEvent) => {
    const c = canvas.current!;
    const r = c.getBoundingClientRect();
    const x = Math.floor(((e.clientX - r.left) / r.width) * c.width);
    const y = Math.floor(((e.clientY - r.top) / r.height) * c.height);
    const [R, G, B] = c.getContext("2d")!.getImageData(x, y, 1, 1).data;
    return [R, G, B];
  };
  const onCanvasClick = (e: React.MouseEvent) => {
    if (!dev || !T) return;
    if (tool === "select") {
      const r = canvas.current!.getBoundingClientRect();
      aiSelect((e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height, e.altKey);
    } else if (tool === "swatch") {
      const [h, C, L] = srgbToOklch(...(sample(e) as [number, number, number]));
      const sw = { id: `swatch_${Date.now().toString(36)}`, src_hsl: [Math.round(h * 10) / 10, Math.round(C * 2500) / 10, Math.round(L * 1000) / 10], shift_hsl: [0, 0, 0], range_hsl: [50, 50, 50], falloff: 50 };
      edit({ ...T, point_color: [...T.point_color, sw].slice(0, 8) }, "Add color swatch");
      setSwatchSel(Math.min(T.point_color.length, 7));
      setTool("none");
    } else if (tool === "maskcolor" && maskSel != null) {
      const rgb = sample(e).map((v) => Math.round((v / 255) * 1000) / 1000);
      change(setPath(dev, ["masks", maskSel, "components", compSel, "color"], rgb), "Mask color");
      setTool("none");
    }
  };

  // drag helper for overlay handles: reports normalised deltas
  const dragger = (onMove: (dx: number, dy: number) => void) => (e: React.PointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    const r = wrap.current!.getBoundingClientRect();
    const [sx, sy] = [e.clientX, e.clientY];
    const move = (ev: PointerEvent) => onMove((ev.clientX - sx) / r.width, (ev.clientY - sy) / r.height);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", () => window.removeEventListener("pointermove", move), { once: true });
  };

  const ratio = (a: number | null) => (a === 0 ? imgSize.w / imgSize.h : a);
  const applyAspect = (a: number | null) => {
    setAspect(a);
    const r = ratio(a);
    if (!dev || r == null) return;
    const W = imgSize.w / imgSize.h;
    const [w, h] = W > r ? [r / W, 1] : [1, W / r];
    change({ ...dev, crop: { ...dev.crop, x: (1 - w) / 2, y: (1 - h) / 2, w, h } }, "Crop aspect");
  };
  const cropDrag = (mode: string) =>
    dragger((dx, dy) => {
      const base = devRef.current!;
      const c0 = cropStart.current!;
      let { x, y, w, h } = c0;
      if (mode === "move") {
        x = clamp(x + dx, 0, 1 - w);
        y = clamp(y + dy, 0, 1 - h);
      } else {
        if (mode.includes("w")) {
          const nx = clamp(x + dx, 0, x + w - 0.05);
          w += x - nx;
          x = nx;
        }
        if (mode.includes("e")) w = clamp(w + dx, 0.05, 1 - x);
        if (mode.includes("n")) {
          const ny = clamp(y + dy, 0, y + h - 0.05);
          h += y - ny;
          y = ny;
        }
        if (mode.includes("s")) h = clamp(h + dy, 0.05, 1 - y);
        const r = ratio(aspect);
        if (r != null) {
          const W = imgSize.w / imgSize.h;
          let nh = (w * W) / r;
          if (nh > 1 - (mode.includes("n") ? 0 : y)) nh = 1 - (mode.includes("n") ? 0 : y);
          w = (nh * r) / W;
          if (mode.includes("w")) x = c0.x + c0.w - w;
          y = mode.includes("n") ? c0.y + c0.h - nh : y;
          h = nh;
          x = clamp(x, 0, 1 - w);
          y = clamp(y, 0, 1 - h);
        }
      }
      change({ ...base, crop: { ...base.crop, x, y, w, h } }, "Crop");
    });

  const compDrag = (ci: number, key: "start" | "end" | "center" | "radius", axis?: 0 | 1) =>
    dragger((dx, dy) => {
      const base = devRef.current!;
      const c = handleStart.current!;
      let v: number[];
      if (key === "radius") v = axis === 0 ? [Math.max(0.01, c.radius[0] + dx), c.radius[1]] : [c.radius[0], Math.max(0.01, c.radius[1] + dy)];
      else v = [clamp(c[key][0] + dx, -0.5, 1.5), clamp(c[key][1] + dy, -0.5, 1.5)];
      change(setPath(base, ["masks", maskSel!, "components", ci, key], v), "Move mask");
    });
  const grab = (ci: number, fn: (e: React.PointerEvent) => void) => (e: React.PointerEvent) => {
    handleStart.current = structuredClone(devRef.current!.masks[maskSel!].components[ci]);
    fn(e);
  };

  const pct = (v: number) => `${v * 100}%`;
  const dot = "absolute h-3.5 w-3.5 -translate-x-1/2 -translate-y-1/2 cursor-move rounded-full border-2 border-white shadow";

  // --- auto tone (histogram heuristic, offline) -----------------------------------------
  const autoTone = () => {
    if (!dev || !hist) return;
    const l = hist[3];
    const n = l.reduce((a, b) => a + b, 0) || 1;
    let acc = 0;
    const pctl = (q: number) => {
      acc = 0;
      for (let i = 0; i < 256; i++) if ((acc += l[i]) >= q * n) return i;
      return 255;
    };
    const mid = Math.max(1, pctl(0.5));
    const ev = clamp(Math.log2(srgbEotf(118 / 255) / srgbEotf(mid / 255)), -2, 2);
    const t = dev.basic_tone;
    const p99 = pctl(0.995);
    const p1 = pctl(0.005);
    change(
      {
        ...dev,
        basic_tone: {
          ...t,
          exposure: Math.round((t.exposure + ev * 0.8) * 100) / 100,
          whites: clamp(Math.round(t.whites + (p99 < 245 ? (245 - p99) / 2 : 0)), -100, 100),
          blacks: clamp(Math.round(t.blacks - (p1 > 6 ? (p1 - 6) / 1.5 : 0)), -100, 100),
          highlights: clamp(t.highlights + (p99 >= 254 ? -30 : 0), -100, 100),
        },
      },
      "Auto tone",
    );
  };

  const swatch = T && swatchSel != null ? T.point_color[swatchSel] : null;

  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-56 shrink-0 flex-col overflow-y-auto border-r border-neutral-800 bg-neutral-900">
        <Panel title="History" right={<span className="text-[10px] font-normal normal-case text-neutral-500">⌘Z / ⇧⌘Z</span>}>
          <div className="max-h-80 space-y-px overflow-y-auto">
            {history.map((h) => (
              <div
                key={h.id}
                onClick={async () => id != null && (await flush(), setDev(await api.gotoHistory(id, h.id)), refreshHistory(id))}
                className={`flex cursor-pointer justify-between gap-2 rounded px-2 py-1 text-[11px] ${h.current ? "bg-neutral-700 text-white" : "text-neutral-400 hover:bg-neutral-800"}`}
              >
                <span className={`truncate ${h.label.startsWith("AI") ? "text-violet-300" : ""}`}>{h.label}</span>
                <span className="shrink-0 text-neutral-500">{h.created_at.slice(11, 16)}</span>
              </div>
            ))}
          </div>
        </Panel>
        <Panel title="Settings">
          <div className="grid grid-cols-2 gap-1">
            <Btn title="⇧⌘C" onClick={() => dev && (clipboard = structuredClone(dev))}>
              Copy
            </Btn>
            <Btn title="⇧⌘V" disabled={!clipboard} onClick={() => dev && clipboard && change(paste(dev, clipboard), "Paste settings")}>
              Paste
            </Btn>
            <Btn disabled={!others.length} onClick={sync} title="Apply these settings (except crop) to the other selected photos">
              Sync to {others.length}
            </Btn>
            <Btn onClick={() => dev && change({ ...structuredClone(p.defaults), ai_provenance_log: dev.ai_provenance_log }, "Reset all")}>Reset all</Btn>
          </div>
        </Panel>
        {dev && dev.ai_provenance_log.length > 0 && (
          <Panel title="AI provenance" open={false}>
            <div className="space-y-2 text-[10px]">
              {dev.ai_provenance_log
                .slice()
                .reverse()
                .map((e, i) => (
                  <div key={i} className="rounded bg-neutral-800 p-2">
                    <div className="text-violet-300">{e.provider}</div>
                    <div className="text-neutral-300">{e.action}</div>
                    <div className="text-neutral-500">{e.timestamp}</div>
                  </div>
                ))}
            </div>
          </Panel>
        )}
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-center gap-2 border-b border-neutral-800 bg-neutral-900 px-3 py-1.5 text-[11px]">
          <span className="truncate text-neutral-300">
            {p.active.filename}
            {p.active.copy_name ? ` · ${p.active.copy_name}` : ""}
          </span>
          <span className="text-neutral-500">{[p.active.camera, p.active.iso && `ISO ${p.active.iso}`, p.active.shutter && `${p.active.shutter}s`, p.active.aperture && `f/${p.active.aperture}`].filter(Boolean).join(" · ")}</span>
          <span className="ml-auto flex gap-1">
            {tool === "swatch" || tool === "maskcolor" ? <span className="px-2 py-1 text-amber-300">Click the photo to sample a colour (Esc cancels)</span> : null}
            {tool === "select" ? (
              <span className="px-2 py-1 text-violet-300">{picking ? "✦ Finding the object…" : mask ? "Click to add an object · ⌥-click to subtract · Esc when done" : "Click an object to select it"}</span>
            ) : null}
            <Btn active={tool === "select"} onClick={() => setTool(tool === "select" ? "none" : "select")} title="AI Select (S): click objects to select them with the AI; then every panel tunes only the selection">
              ✦ Select
            </Btn>
            <Btn active={before} onClick={() => setBefore(!before)} title="Before / after (\)">
              Before
            </Btn>
            <Btn active={tool === "crop"} onClick={() => setTool(tool === "crop" ? "none" : "crop")} title="Crop & straighten (R)">
              Crop
            </Btn>
            <Btn onClick={p.onExport}>Export…</Btn>
          </span>
        </div>

        <div ref={boxRef} className="checker relative flex min-h-0 flex-1 items-center justify-center overflow-hidden">
          <div ref={wrap} className="relative overflow-hidden shadow-2xl" style={{ width: disp.w, height: disp.h }}>
            <canvas
              ref={canvas}
              onClick={onCanvasClick}
              className={`block h-full w-full ${picking ? "cursor-wait" : tool === "swatch" || tool === "maskcolor" || tool === "select" ? "cursor-crosshair" : ""}`}
            />
            {picking && (
              <span
                className="pointer-events-none absolute h-10 w-10 -translate-x-1/2 -translate-y-1/2 animate-ping rounded-full border-2 border-violet-300"
                style={{ left: pct(picking.x), top: pct(picking.y) }}
              />
            )}
            {tool === "crop" && dev && !before && (
              <div
                className="absolute cursor-move border border-white/80"
                style={{ left: pct(dev.crop.x), top: pct(dev.crop.y), width: pct(dev.crop.w), height: pct(dev.crop.h), boxShadow: "0 0 0 9999px rgba(0,0,0,.55)" }}
                onPointerDown={(e) => ((cropStart.current = { ...dev.crop }), cropDrag("move")(e))}
              >
                <div className="pointer-events-none absolute inset-0" style={{ backgroundImage: "linear-gradient(90deg,transparent 33%,rgba(255,255,255,.25) 33%,rgba(255,255,255,.25) 33.4%,transparent 33.4%,transparent 66.4%,rgba(255,255,255,.25) 66.4%,rgba(255,255,255,.25) 66.8%,transparent 66.8%),linear-gradient(transparent 33%,rgba(255,255,255,.25) 33%,rgba(255,255,255,.25) 33.4%,transparent 33.4%,transparent 66.4%,rgba(255,255,255,.25) 66.4%,rgba(255,255,255,.25) 66.8%,transparent 66.8%)" }} />
                {["nw", "ne", "sw", "se"].map((m) => (
                  <div
                    key={m}
                    className="absolute h-3 w-3 border-2 border-white bg-neutral-900"
                    style={{ left: m.includes("w") ? -6 : undefined, right: m.includes("e") ? -6 : undefined, top: m.includes("n") ? -6 : undefined, bottom: m.includes("s") ? -6 : undefined, cursor: `${m}-resize` }}
                    onPointerDown={(e) => ((cropStart.current = { ...dev.crop }), cropDrag(m)(e))}
                  />
                ))}
              </div>
            )}
            {mask && handlesOn && !before && tool !== "crop" && (
              <svg className="pointer-events-none absolute inset-0 h-full w-full overflow-visible" style={{ filter: "drop-shadow(0 0 1px #000)" }}>
                {mask.components.map((c, ci) =>
                  c.type === "linear_gradient" ? (
                    <line key={ci} x1={pct(c.start[0])} y1={pct(c.start[1])} x2={pct(c.end[0])} y2={pct(c.end[1])} stroke="white" strokeDasharray="4 3" />
                  ) : c.type === "radial_gradient" ? (
                    <ellipse key={ci} cx={pct(c.center[0])} cy={pct(c.center[1])} rx={pct(c.radius[0])} ry={pct(c.radius[1])} fill="none" stroke="white" strokeDasharray="4 3" />
                  ) : null,
                )}
              </svg>
            )}
            {mask &&
              handlesOn &&
              !before &&
              tool !== "crop" &&
              mask.components.map((c, ci) =>
                c.type === "linear_gradient" ? (
                  <Fragment key={ci}>
                    <div className={`${dot} bg-sky-500/80`} title="Full effect" style={{ left: pct(c.start[0]), top: pct(c.start[1]) }} onPointerDown={grab(ci, compDrag(ci, "start"))} />
                    <div className={`${dot} bg-neutral-900/80`} title="No effect" style={{ left: pct(c.end[0]), top: pct(c.end[1]) }} onPointerDown={grab(ci, compDrag(ci, "end"))} />
                  </Fragment>
                ) : c.type === "radial_gradient" ? (
                  <Fragment key={ci}>
                    <div className={`${dot} bg-sky-500/80`} style={{ left: pct(c.center[0]), top: pct(c.center[1]) }} onPointerDown={grab(ci, compDrag(ci, "center"))} />
                    <div className={`${dot} bg-neutral-900/80`} style={{ left: pct(c.center[0] + c.radius[0]), top: pct(c.center[1]) }} onPointerDown={grab(ci, compDrag(ci, "radius", 0))} />
                    <div className={`${dot} bg-neutral-900/80`} style={{ left: pct(c.center[0]), top: pct(c.center[1] + c.radius[1]) }} onPointerDown={grab(ci, compDrag(ci, "radius", 1))} />
                  </Fragment>
                ) : null,
              )}
          </div>
          {(loading || error) && (
            <div className="absolute inset-x-0 top-3 mx-auto w-fit rounded bg-neutral-900/90 px-3 py-1.5 text-[11px] text-neutral-300">{error ? <span className="text-red-400">{error}</span> : "Loading…"}</div>
          )}
          {before && <div className="absolute left-3 top-3 rounded bg-neutral-900/80 px-2 py-1 text-[11px]">Before</div>}
        </div>

        <AiBar settings={p.settings} setSettings={p.setSettings} run={aiRun} logs={p.logs} clearLogs={p.clearLogs} />

        <div className="flex h-[84px] shrink-0 gap-1 overflow-x-auto border-t border-neutral-800 bg-neutral-900 p-1.5">
          {p.photos.map((ph) => (
            <img
              id={`fs-${ph.id}`}
              key={ph.id}
              src={thumbUrl(ph)}
              loading="lazy"
              draggable={false}
              onClick={() => p.setActive(ph.id)}
              className={`h-full shrink-0 cursor-pointer rounded-sm object-contain ${ph.id === id ? "ring-2 ring-sky-500" : p.selected.includes(ph.id) ? "opacity-90 ring-1 ring-neutral-400" : "opacity-60 hover:opacity-100"}`}
              alt=""
            />
          ))}
        </div>
      </main>

      <aside className="w-[310px] shrink-0 overflow-y-auto border-l border-neutral-800 bg-neutral-900">
        <div className="border-b border-neutral-800 p-3">
          <Histogram hist={hist} />
        </div>
        {mask && (
          <div className="sticky top-0 z-10 flex items-center gap-1.5 border-b border-violet-800/60 bg-violet-950/95 px-3 py-2 text-[11px]">
            <span className="min-w-0 flex-1 truncate text-violet-200" title="Every panel below changes only this selection">
              Tuning <b className="text-white">{mask.name}</b> only
            </span>
            <Btn active={overlay} onClick={() => setOverlay(!overlay)} title="Show the selection (O)">
              Show
            </Btn>
            <Btn disabled={!mask.tuning} onClick={() => edit(structuredClone(p.defaults), "Reset")} title="Undo all tuning of this selection">
              Reset
            </Btn>
            <Btn onClick={() => (setMaskSel(null), setTool("none"))} title="Back to the whole photo (Esc)">
              Done
            </Btn>
          </div>
        )}
        {dev && T && (
          <>
            <Panel
              title="Basic"
              right={
                <span className="flex gap-2">
                  {!mask && (
                    <button className="text-[10px] font-normal normal-case text-sky-400 hover:text-sky-300" onClick={autoTone}>
                      auto
                    </button>
                  )}
                  {reset("basic_tone", "tone")}
                </span>
              }
            >
              <div className="mb-2">
                <Seg options={[["Color", "Color"], ["Monochrome", "B&W"]]} value={T.profile.toLowerCase() === "monochrome" ? "Monochrome" : "Color"} onChange={(v) => edit({ ...T, profile: v }, "Profile")} />
              </div>
              {S("Temp", ["white_balance", "temperature_k"], 2000, 50000, { log: true, track: "track-temp" })}
              {S("Tint", ["white_balance", "tint"], -150, 150, { track: "track-tint" })}
              <div className="my-1.5 border-t border-neutral-800" />
              {S("Exposure", ["basic_tone", "exposure"], -5, 5, { step: 0.01, digits: 2 })}
              {S("Contrast", ["basic_tone", "contrast"], -100, 100)}
              {S("Highlights", ["basic_tone", "highlights"], -100, 100)}
              {S("Shadows", ["basic_tone", "shadows"], -100, 100)}
              {S("Whites", ["basic_tone", "whites"], -100, 100)}
              {S("Blacks", ["basic_tone", "blacks"], -100, 100)}
              <div className="my-1.5 border-t border-neutral-800" />
              {S("Texture", ["presence", "texture"], -100, 100)}
              {S("Clarity", ["presence", "clarity"], -100, 100)}
              {S("Dehaze", ["presence", "dehaze"], -100, 100)}
              {S("Vibrance", ["presence", "vibrance"], -100, 100)}
              {S("Saturation", ["presence", "saturation"], -100, 100)}
            </Panel>

            <Panel title="Tone curve" open={false} right={reset("tone_curve", "curve")}>
              {S("Highlights", ["tone_curve", "parametric", "highlights"], -100, 100)}
              {S("Lights", ["tone_curve", "parametric", "lights"], -100, 100)}
              {S("Darks", ["tone_curve", "parametric", "darks"], -100, 100)}
              {S("Shadows", ["tone_curve", "parametric", "shadows"], -100, 100)}
              <div className="my-2">
                <Seg
                  options={[["point_luma", "RGB"], ["point_red", "Red"], ["point_green", "Green"], ["point_blue", "Blue"]]}
                  value={curveKey.current}
                  onChange={(v) => ((curveKey.current = v), force((n) => n + 1))}
                />
              </div>
              <CurveEditor
                points={T.tone_curve[curveKey.current]}
                color={{ point_luma: "#ddd", point_red: "#f55", point_green: "#5d5", point_blue: "#58f" }[curveKey.current]}
                hist={hist?.[{ point_luma: 3, point_red: 0, point_green: 1, point_blue: 2 }[curveKey.current]]}
                onChange={(pts) => edit(setPath(T, ["tone_curve", curveKey.current], pts), "Point curve")}
              />
              <p className="mt-1 text-[10px] text-neutral-500">Click to add · drag to move · double-click to delete</p>
            </Panel>

            <Panel title="Color mixer" open={false} right={reset("color_mixer", "mixer")}>
              <div className="mb-2">
                <Seg options={[["hue", "Hue"], ["saturation", "Saturation"], ["luminance", "Luminance"]]} value={mixerTab} onChange={setMixerTab} />
              </div>
              {BANDS.map(([name, col], i) => (
                <div key={name} className="flex items-center gap-1">
                  <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: col }} />
                  <div className="flex-1">{S(name, ["color_mixer", mixerTab, i], -100, 100)}</div>
                </div>
              ))}
            </Panel>

            <Panel title="Point color" open={false} right={reset("point_color", "point color")}>
              <div className="mb-2 flex flex-wrap items-center gap-1.5">
                <Btn active={tool === "swatch"} onClick={() => setTool(tool === "swatch" ? "none" : "swatch")} disabled={T.point_color.length >= 8}>
                  ⌖ Pick colour
                </Btn>
                {T.point_color.map((s, i) => (
                  <button
                    key={s.id || i}
                    onClick={() => setSwatchSel(i)}
                    className={`h-5 w-5 rounded ${swatchSel === i ? "ring-2 ring-white" : ""}`}
                    style={{ background: `oklch(${s.src_hsl[2]}% ${s.src_hsl[1] / 250} ${s.src_hsl[0]})` }}
                    title={`hue ${s.src_hsl[0]}°`}
                  />
                ))}
              </div>
              {swatch && swatchSel != null && (
                <>
                  {S("Hue shift", ["point_color", swatchSel, "shift_hsl", 0], -100, 100)}
                  {S("Sat shift", ["point_color", swatchSel, "shift_hsl", 1], -100, 100)}
                  {S("Lum shift", ["point_color", swatchSel, "shift_hsl", 2], -100, 100)}
                  {S("Hue range", ["point_color", swatchSel, "range_hsl", 0], 0, 100, { def: 50 })}
                  {S("Sat range", ["point_color", swatchSel, "range_hsl", 1], 0, 100, { def: 50 })}
                  {S("Lum range", ["point_color", swatchSel, "range_hsl", 2], 0, 100, { def: 50 })}
                  {S("Falloff", ["point_color", swatchSel, "falloff"], 0, 100, { def: 50 })}
                  <Btn className="mt-1" onClick={() => (edit({ ...T, point_color: T.point_color.filter((_, k) => k !== swatchSel) }, "Delete swatch"), setSwatchSel(null))}>
                    Delete swatch
                  </Btn>
                </>
              )}
            </Panel>

            <Panel title="Color grading" open={false} right={reset("color_grading_3way", "grading")}>
              <div className="grid grid-cols-3 gap-2">
                {(["shadows", "midtones", "highlights"] as const).map((k) => (
                  <ColorWheel key={k} size={84} label={k[0].toUpperCase() + k.slice(1)} wheel={T.color_grading_3way[k]} onChange={(w) => edit(setPath(T, ["color_grading_3way", k], w), `Grade ${k}`)} />
                ))}
              </div>
              <div className="mx-auto mt-2 w-28">
                <ColorWheel label="Global" size={84} wheel={T.color_grading_3way.global} onChange={(w) => edit(setPath(T, ["color_grading_3way", "global"], w), "Grade global")} />
              </div>
              {S("Blending", ["color_grading_3way", "blending"], 0, 100)}
              {S("Balance", ["color_grading_3way", "balance"], -100, 100)}
            </Panel>

            <Panel title="Detail" open={false} right={reset("detail", "detail")}>
              {S("Sharpening", ["detail", "sharpening"], 0, 150)}
              {S("Radius", ["detail", "sharpen_radius"], 0.5, 3, { step: 0.1, digits: 1 })}
              {S("Masking", ["detail", "sharpen_masking"], 0, 100)}
              {S("Color NR", ["detail", "color_noise_reduction"], 0, 100)}
              <p className="mt-1 text-[10px] text-neutral-500">Preview is a 1600px proxy; sharpening is exact on export.</p>
            </Panel>

            {/* whole-photo only: hidden while a selection is tuned */}
            {!mask && (
              <>
                <Panel title="Effects" open={false} right={reset("effects", "effects")}>
                  {S("Vignette", ["effects", "vignette"], -100, 100)}
                  {S("Midpoint", ["effects", "vignette_midpoint"], 0, 100)}
                  {S("Feather", ["effects", "vignette_feather"], 0, 100)}
                </Panel>

                <Panel title="Crop & straighten" open={tool === "crop"} right={reset("crop", "crop")}>
                  <div className="mb-2 flex flex-wrap gap-1">
                    {ASPECTS.map(([label, a]) => (
                      <Btn key={label} active={aspect === a} onClick={() => (setTool("crop"), applyAspect(a))}>
                        {label}
                      </Btn>
                    ))}
                  </div>
                  {S("Angle", ["crop", "angle"], -45, 45, { step: 0.1, digits: 1 })}
                  <Btn className="mt-1" active={tool === "crop"} onClick={() => setTool(tool === "crop" ? "none" : "crop")}>
                    {tool === "crop" ? "Done (R)" : "Edit crop (R)"}
                  </Btn>
                </Panel>

                <Panel title="Remove" open={false} right={reset("retouch", "removals")}>
                  {dev.retouch.length === 0 && <p className="text-[10px] text-neutral-500">Ask the AI bar, e.g. “remove the people on the sand”.</p>}
                  <div className="space-y-1">
                    {dev.retouch.map((r, i) => (
                      <div key={i} className="flex items-center gap-2 rounded bg-neutral-800 px-2 py-1 text-[11px]">
                        <span className="flex-1 truncate" title={r.name}>
                          {r.name}
                        </span>
                        <button className="text-neutral-500 hover:text-white" title="Undo this removal" onClick={() => change({ ...dev, retouch: dev.retouch.filter((_, k) => k !== i) }, "Delete removal")}>
                          ✕
                        </button>
                      </div>
                    ))}
                  </div>
                </Panel>
              </>
            )}

            <Panel title="Masks" open={false}>
              <MasksPanel
                dev={dev}
                change={change}
                sel={maskSel}
                setSel={(i) => (setMaskSel(i), setCompSel(0))}
                compSel={compSel}
                setCompSel={setCompSel}
                overlay={overlay}
                setOverlay={setOverlay}
                pickColor={(ci) => (setCompSel(ci), setTool("maskcolor"))}
                S={slider(dev, change)}
              />
            </Panel>
          </>
        )}
      </aside>
    </div>
  );
}

function MasksPanel({
  dev,
  change,
  sel,
  setSel,
  compSel,
  setCompSel,
  overlay,
  setOverlay,
  pickColor,
  S,
}: {
  dev: Dev;
  change: (d: Dev, label: string) => void;
  sel: number | null;
  setSel: (i: number | null) => void;
  compSel: number;
  setCompSel: (i: number) => void;
  overlay: boolean;
  setOverlay: (b: boolean) => void;
  pickColor: (ci: number) => void;
  S: (label: string, path: Path, min: number, max: number, extra?: Partial<Parameters<typeof Slider>[0]>) => ReactNode;
}) {
  const [newType, setNewType] = useState("linear_gradient");
  const [newOp, setNewOp] = useState("subtract");
  const add = (type: string) => {
    const m: Mask = {
      mask_id: `mask_${Date.now().toString(36)}`,
      name: `${MASK_TYPES.find((t) => t[0] === type)![1]} ${dev.masks.length + 1}`,
      enabled: true,
      components: [{ ...COMPONENT, type }],
      adjustments: { exposure: 0, contrast: 0, highlights: 0, shadows: 0, whites: 0, blacks: 0, temperature_shift: 0, tint_shift: 0, saturation: 0 },
    };
    change({ ...dev, masks: [...dev.masks, m] }, "Add mask");
    setSel(dev.masks.length);
  };
  const m = sel != null ? dev.masks[sel] : null;
  const field = "rounded bg-neutral-800 px-1 py-0.5 text-[11px]";
  return (
    <div className="space-y-2 text-[11px]">
      <div className="grid grid-cols-2 gap-1">
        {MASK_TYPES.map(([t, label]) => (
          <Btn key={t} onClick={() => add(t)}>
            + {label}
          </Btn>
        ))}
      </div>
      {dev.masks.map((mk, i) => (
        <div key={mk.mask_id || i} onClick={() => setSel(sel === i ? null : i)} className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1 ${sel === i ? "bg-neutral-700" : "bg-neutral-800 hover:bg-neutral-700/60"}`}>
          <input type="checkbox" checked={mk.enabled} onClick={(e) => e.stopPropagation()} onChange={(e) => change(setPath(dev, ["masks", i, "enabled"], e.target.checked), "Toggle mask")} />
          <span className="flex-1 truncate">{mk.name}</span>
          <span className="text-neutral-500">{mk.components.length}</span>
        </div>
      ))}
      {m && sel != null && (
        <div className="space-y-2 rounded border border-neutral-700 p-2">
          <div className="flex items-center gap-1">
            <input className={`${field} min-w-0 flex-1`} value={m.name} onChange={(e) => change(setPath(dev, ["masks", sel, "name"], e.target.value), "Rename mask")} />
            <Btn active={overlay} onClick={() => setOverlay(!overlay)} title="Show mask overlay (O)">
              Overlay
            </Btn>
            <Btn onClick={() => (change({ ...dev, masks: dev.masks.filter((_, k) => k !== sel) }, "Delete mask"), setSel(null))}>✕</Btn>
          </div>
          {m.components.map((c, ci) => (
            <div key={ci} onClick={() => setCompSel(ci)} className={`space-y-1 rounded p-1.5 ${compSel === ci ? "bg-neutral-800" : ""}`}>
              <div className="flex items-center gap-1">
                {ci > 0 ? (
                  <select className={field} value={c.op} onChange={(e) => change(setPath(dev, ["masks", sel, "components", ci, "op"], e.target.value), "Mask op")}>
                    <option value="add">Add</option>
                    <option value="subtract">Subtract</option>
                    <option value="intersect">Intersect</option>
                  </select>
                ) : null}
                <span className="flex-1 truncate text-neutral-300">{TYPE_LABEL[c.type] ?? c.type}</span>
                <label className="flex items-center gap-1 text-neutral-400">
                  <input type="checkbox" checked={c.invert} onChange={(e) => change(setPath(dev, ["masks", sel, "components", ci, "invert"], e.target.checked), "Invert mask")} />
                  Invert
                </label>
                {m.components.length > 1 && (
                  <button className="text-neutral-500 hover:text-white" onClick={() => change(setPath(dev, ["masks", sel, "components"], m.components.filter((_, k) => k !== ci)), "Remove component")}>
                    ✕
                  </button>
                )}
              </div>
              {(c.type === "linear_gradient" || c.type === "radial_gradient") && <p className="text-[10px] text-neutral-500">Drag the handles on the photo.</p>}
              {c.type === "polygon" && <p className="text-[10px] text-neutral-500">Traced by the AI; the edge snaps to the photo. Overlay (O) shows it.</p>}
              {c.type === "luminance_range" && (
                <>
                  {S("Low", ["masks", sel, "components", ci, "range", 0], 0, 1, { step: 0.01, digits: 2, def: 0.5 })}
                  {S("High", ["masks", sel, "components", ci, "range", 1], 0, 1, { step: 0.01, digits: 2, def: 1 })}
                </>
              )}
              {c.type === "color_range" && (
                <>
                  <div className="flex items-center gap-2">
                    <span className="h-4 w-4 rounded" style={{ background: `rgb(${c.color.map((v) => v * 255).join(",")})` }} />
                    <Btn onClick={() => pickColor(ci)}>⌖ Pick colour</Btn>
                  </div>
                  {S("Tolerance", ["masks", sel, "components", ci, "tolerance"], 0, 100, { def: 50 })}
                </>
              )}
              {S("Feather", ["masks", sel, "components", ci, "feather"], -100, 100)}
              {S("Edge shift", ["masks", sel, "components", ci, "edge_shift"], -100, 100)}
            </div>
          ))}
          <div className="flex gap-1">
            <select className={field} value={newOp} onChange={(e) => setNewOp(e.target.value)}>
              <option value="add">Add</option>
              <option value="subtract">Subtract</option>
              <option value="intersect">Intersect</option>
            </select>
            <select className={`${field} min-w-0 flex-1`} value={newType} onChange={(e) => setNewType(e.target.value)}>
              {MASK_TYPES.map(([t, l]) => (
                <option key={t} value={t}>
                  {l}
                </option>
              ))}
            </select>
            <Btn onClick={() => change(setPath(dev, ["masks", sel, "components"], [...m.components, { ...COMPONENT, type: newType, op: newOp }]), "Add component")}>+</Btn>
          </div>
          {/* older quick adjustments (e.g. set by the AI bar); the Develop panels tune the selected mask fully */}
          {Object.values(m.adjustments).some((v) => v !== 0) && (
            <div className="border-t border-neutral-700 pt-1">
              <p className="mb-1 text-[10px] text-neutral-500">Quick adjustments (set by the AI)</p>
              {S("Temp", ["masks", sel, "adjustments", "temperature_shift"], -100, 100, { track: "track-temp" })}
              {S("Tint", ["masks", sel, "adjustments", "tint_shift"], -100, 100, { track: "track-tint" })}
              {S("Exposure", ["masks", sel, "adjustments", "exposure"], -5, 5, { step: 0.01, digits: 2 })}
              {S("Contrast", ["masks", sel, "adjustments", "contrast"], -100, 100)}
              {S("Highlights", ["masks", sel, "adjustments", "highlights"], -100, 100)}
              {S("Shadows", ["masks", sel, "adjustments", "shadows"], -100, 100)}
              {S("Whites", ["masks", sel, "adjustments", "whites"], -100, 100)}
              {S("Blacks", ["masks", sel, "adjustments", "blacks"], -100, 100)}
              {S("Saturation", ["masks", sel, "adjustments", "saturation"], -100, 100)}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function AiBar({
  settings,
  setSettings,
  run: runAi,
  logs,
  clearLogs,
}: {
  settings: Settings | null;
  setSettings: (s: Settings) => void;
  run: (prompt: string, provider: string) => Promise<string>;
  logs: Log[];
  clearLogs?: () => void;
}) {
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [startTime, setStartTime] = useState<number | null>(null);
  const [elapsed, setElapsed] = useState(0);
  const [runStartIdx, setRunStartIdx] = useState<number | null>(null);
  const [showDetails, setShowDetails] = useState(false);
  const [logFilter, setLogFilter] = useState<"run" | "all">("run");
  const [copied, setCopied] = useState(false);
  const userClosedDetails = useRef(false);
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!busy || startTime == null) return;
    const interval = setInterval(() => {
      setElapsed((Date.now() - startTime) / 1000);
    }, 100);
    return () => clearInterval(interval);
  }, [busy, startTime]);

  useEffect(() => {
    if (showDetails) {
      logEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [logs, showDetails]);

  if (!settings) return null;
  const provider = settings.active_provider;

  const runLogs = runStartIdx != null ? logs.slice(runStartIdx) : [];
  const displayLogs = logFilter === "run" && runStartIdx != null ? runLogs : logs;
  const latestLog = runLogs.length > 0 ? runLogs[runLogs.length - 1] : logs.length > 0 ? logs[logs.length - 1] : null;

  const copyLogs = () => {
    const text = displayLogs.map((l) => `[${l.t}] [${l.source}] ${l.message}`).join("\n");
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const run = async () => {
    if (!prompt.trim() || busy) return;
    const now = Date.now();
    setStartTime(now);
    setElapsed(0);
    setRunStartIdx(logs.length);
    setBusy(true);
    setResult(null);
    if (!userClosedDetails.current) {
      setShowDetails(true);
    }
    try {
      const text = await runAi(prompt, provider);
      setResult({ ok: true, text });
    } catch (e) {
      setResult({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
      setElapsed((Date.now() - now) / 1000);
    }
  };

  const getSourceBadge = (source: string) => {
    if (source === "mcp") return "bg-sky-950/70 text-sky-400 border border-sky-800/60";
    if (source.includes("antigravity") || source.includes("claude")) return "bg-violet-950/70 text-violet-300 border border-violet-800/60";
    if (source.includes("gemini") || source.includes("openai") || source.includes("anthropic")) return "bg-emerald-950/70 text-emerald-300 border border-emerald-800/60";
    return "bg-neutral-800 text-neutral-300 border border-neutral-700";
  };

  return (
    <div className="border-t border-neutral-800 bg-neutral-900 px-3 py-2 text-[11px]">
      <div className="flex items-center gap-2">
        <span className="font-semibold text-violet-300">✦ AI</span>
        <select
          className="max-w-44 rounded bg-neutral-800 px-1 py-1"
          value={provider}
          onChange={(e) => {
            const s = { ...settings, active_provider: e.target.value };
            setSettings(s);
            api.saveSettings(s);
          }}
          disabled={busy}
        >
          {settings.providers.map((pr) => (
            <option key={pr.id} value={pr.id}>
              {pr.name}
            </option>
          ))}
        </select>
        <input
          className="min-w-0 flex-1 rounded bg-neutral-800 px-2 py-1 outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-violet-500"
          placeholder="Describe an edit, e.g. “select the castle and warm it up”, “remove the people on the sand”, “recover the blown sky”"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && run()}
          disabled={busy}
        />
        <Btn active onClick={run} disabled={busy || !prompt.trim()}>
          {busy ? `Working… ${elapsed.toFixed(1)}s` : "Run"}
        </Btn>
      </div>

      {busy && (
        <div className="relative mt-2 h-1 w-full overflow-hidden rounded bg-neutral-800">
          <div className="animate-indeterminate absolute inset-y-0 w-1/3 rounded bg-gradient-to-r from-violet-500 via-sky-400 to-violet-500" />
        </div>
      )}

      <div className="mt-2 flex items-center justify-between gap-2 text-[11px]">
        <div className="flex min-w-0 items-center gap-2 overflow-hidden">
          {busy ? (
            <>
              <span className="relative flex h-2 w-2 shrink-0">
                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-violet-400 opacity-75" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-violet-500" />
              </span>
              <span className="shrink-0 font-medium text-violet-300">Working ({elapsed.toFixed(1)}s):</span>
              {latestLog ? (
                <span className="truncate font-mono text-[10px] text-neutral-300">
                  <span className={latestLog.source === "mcp" ? "font-semibold text-sky-400" : "font-semibold text-violet-300"}>
                    [{latestLog.source}]
                  </span>{" "}
                  {latestLog.message}
                </span>
              ) : (
                <span className="italic text-neutral-500">Initializing agent...</span>
              )}
            </>
          ) : result ? (
            <span className="flex items-center gap-1.5 text-neutral-400">
              <span className={`h-1.5 w-1.5 rounded-full ${result.ok ? "bg-emerald-400" : "bg-red-400"}`} />
              {result.ok ? `Finished in ${elapsed.toFixed(1)}s` : `Failed after ${elapsed.toFixed(1)}s`}
            </span>
          ) : latestLog ? (
            <span className="truncate font-mono text-[10px] text-neutral-500">
              Last activity:{" "}
              <span className={latestLog.source === "mcp" ? "text-sky-400/80" : "text-violet-400/80"}>[{latestLog.source}]</span>{" "}
              {latestLog.message}
            </span>
          ) : (
            <span className="text-neutral-500">Ready</span>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <button
            type="button"
            onClick={() => {
              const next = !showDetails;
              userClosedDetails.current = !next;
              setShowDetails(next);
            }}
            className="flex items-center gap-1.5 rounded px-2 py-0.5 text-[10px] font-medium text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200"
          >
            <span>{showDetails ? "Hide details" : "Show details"}</span>
            {(runLogs.length > 0 || logs.length > 0) && (
              <span className="rounded-full bg-neutral-800 px-1.5 text-[9px] text-neutral-300">
                {runLogs.length > 0 ? runLogs.length : logs.length}
              </span>
            )}
            <span className="text-[8px]">{showDetails ? "▲" : "▼"}</span>
          </button>
        </div>
      </div>

      {showDetails && (
        <div className="mt-2 flex flex-col rounded border border-neutral-800 bg-neutral-950 p-2 text-[11px]">
          <div className="mb-1.5 flex items-center justify-between border-b border-neutral-800/80 pb-1.5 text-[10px] text-neutral-400">
            <div className="flex items-center gap-2">
              <span className="font-semibold text-neutral-300">Activity Log</span>
              {runStartIdx != null && (
                <div className="flex rounded bg-neutral-900 p-0.5 text-[9px]">
                  <button
                    type="button"
                    className={`rounded px-1.5 py-0.5 ${logFilter === "run" ? "bg-neutral-700 font-medium text-white" : "text-neutral-400 hover:text-neutral-200"}`}
                    onClick={() => setLogFilter("run")}
                  >
                    This Run ({runLogs.length})
                  </button>
                  <button
                    type="button"
                    className={`rounded px-1.5 py-0.5 ${logFilter === "all" ? "bg-neutral-700 font-medium text-white" : "text-neutral-400 hover:text-neutral-200"}`}
                    onClick={() => setLogFilter("all")}
                  >
                    All ({logs.length})
                  </button>
                </div>
              )}
            </div>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={copyLogs}
                className="rounded px-1.5 py-0.5 hover:bg-neutral-800 hover:text-white"
              >
                {copied ? "Copied!" : "Copy"}
              </button>
              {clearLogs && (
                <button
                  type="button"
                  onClick={clearLogs}
                  className="rounded px-1.5 py-0.5 hover:bg-neutral-800 hover:text-white"
                >
                  Clear
                </button>
              )}
            </div>
          </div>
          <div className="max-h-48 min-h-24 select-text space-y-1 overflow-y-auto font-mono text-[10px]">
            {displayLogs.length === 0 ? (
              <div className="py-2 text-center text-neutral-600">No activity logged yet.</div>
            ) : (
              displayLogs.map((l, i) => (
                <div key={i} className="whitespace-pre-wrap break-all leading-tight">
                  <span className="text-neutral-600">{l.t}</span>{" "}
                  <span className={`inline-block rounded px-1 py-0.2 text-[9px] font-medium ${getSourceBadge(l.source)}`}>
                    {l.source}
                  </span>{" "}
                  <span className="text-neutral-300">{l.message}</span>
                </div>
              ))
            )}
            <div ref={logEndRef} />
          </div>
        </div>
      )}

      {result && (
        <div
          className={`mt-2 max-h-32 overflow-y-auto whitespace-pre-wrap rounded border p-2.5 font-sans text-[11px] leading-relaxed ${
            result.ok ? "border-neutral-700 bg-neutral-800/90 text-neutral-200" : "border-red-900/60 bg-red-950/20 text-red-300"
          }`}
        >
          <div className="flex items-start justify-between gap-2">
            <div className="flex-1 select-text">
              <span className="mr-1.5 font-semibold text-violet-300">{result.ok ? "AI summary:" : "Error:"}</span>
              {result.text}
            </div>
            <button className="shrink-0 p-0.5 text-neutral-500 hover:text-white" onClick={() => setResult(null)}>
              ✕
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
