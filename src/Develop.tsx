import { listen } from "@tauri-apps/api/event";
import { Fragment, ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { AdvancedTab } from "./Advanced";
import { Log } from "./Agent";
import { AutoTab } from "./Auto";
import { ProTab } from "./Pro";
import { api, Develop as Dev, getPath, HistoryEntry, Mask, MaskComponent, Photo, Proof, Rendered, setPath, Settings, srgbToOklch, thumbUrl } from "./api";
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

type Tool = "none" | "crop" | "swatch" | "maskcolor" | "select" | "guides" | "spot";
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
  name: "",
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

/** Ensure each AI-selected polygon object is its own selection in `masks` so it can be tuned independently. */
function splitAiObjects(d: Dev): Dev | null {
  let changed = false;
  const masks: Mask[] = [];
  for (const m of d.masks) {
    const comps = m.components.map((c) => ({ ...c }));
    if (comps[0] && comps[0].type === "polygon" && comps[0].op === "subtract" && !m.name.startsWith("all but ")) {
      comps[0].op = "add";
      changed = true;
    }
    const keep: MaskComponent[] = [];
    const extra: MaskComponent[] = [];
    comps.forEach((c, ci) => {
      if (ci > 0 && c.type === "polygon" && c.op === "add") {
        extra.push({ ...c, op: "add" });
        changed = true;
      } else {
        keep.push(c);
      }
    });
    masks.push({ ...m, components: keep });
    extra.forEach((c, k) => {
      masks.push({
        mask_id: `${m.mask_id}_${k + 1}`,
        name: c.name || `Object ${masks.length + 1}`,
        enabled: m.enabled,
        components: [c],
        adjustments: { exposure: 0, contrast: 0, highlights: 0, shadows: 0, whites: 0, blacks: 0, temperature_shift: 0, tint_shift: 0, saturation: 0 },
        tuning: null,
      });
    });
  }
  return changed ? { ...d, masks } : null;
}

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
  const [rtab, setRtab] = useState<"adjust" | "selections" | "advanced" | "pro" | "auto">("adjust");
  /** a prompt for the AI bar to run (the Auto tab's ✦ buttons) */
  const [ask, setAsk] = useState<{ text: string; n: number } | null>(null);
  /** AI Select / spot heal: where the click being worked on was (normalised) */
  const [picking, setPicking] = useState<{ x: number; y: number } | null>(null);
  const [proof, setProof] = useState<Proof>({ gamut: null, clipping: false, spots: null });
  /** spot heal radius, % of the long edge */
  const [spotSize, setSpotSize] = useState(1.5);
  /** Guided Upright line being drawn */
  const [draft, setDraft] = useState<number[][] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [imgSize, setImgSize] = useState({ w: 3, h: 2 });
  const [box, setBox] = useState({ w: 0, h: 0 });
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [spaceHeld, setSpaceHeld] = useState(false);
  const [peekingBefore, setPeekingBefore] = useState(false);
  const panStart = useRef<{ x: number; y: number; px: number; py: number; id: number; moved: boolean } | null>(null);
  const suppressClick = useRef(false);
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
    setZoom(1);
    setPan({ x: 0, y: 0 });
    flush().then(() => api.getDevelop(id).then((d) => live && setDev(d)));
    refreshHistory(id);
    return () => {
      live = false;
    };
  }, [id, flush, refreshHistory]);
  useEffect(() => () => void flush(), [flush]);

  useEffect(() => {
    if (!dev) return;
    const split = splitAiObjects(dev);
    if (split) change(split, "Split AI objects");
  }, [dev, change]);

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
  const job = useRef<{ id: number; d: Dev; skip: boolean; overlay: number | null; full: boolean; proof: Proof | null } | null>(null);
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
        draw(await api.render(j.id, j.d, j.skip, j.overlay, j.full, j.proof));
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
  const skipCrop = tool === "crop";
  // AI Select, gradient handles, guides and spot healing show the frame before any geometry
  // (distortion, transform, straighten, crop), so the photo's coordinates are mask coordinates
  const fullFrame = tool === "select" || tool === "guides" || tool === "spot" || handlesOn;
  // Visualize Spots only while healing; gamut and clipping warnings in every tab
  const spots = tool === "spot" ? proof.spots : null;
  const showBefore = before || peekingBefore;
  useEffect(() => {
    if (id == null || !dev) return;
    const view = showBefore ? null : { gamut: proof.gamut, clipping: proof.clipping, spots };
    job.current = { id, d: showBefore ? p.defaults : dev, skip: skipCrop && !showBefore, overlay: overlay && !showBefore ? maskSel : null, full: fullFrame, proof: view };
    pump();
  }, [id, dev, showBefore, skipCrop, fullFrame, overlay, maskSel, proof.gamut, proof.clipping, spots, p.defaults, pump]);

  useEffect(() => {
    const ro = new ResizeObserver(([e]) => setBox({ w: e.contentRect.width, h: e.contentRect.height }));
    ro.observe(boxRef.current!);
    return () => ro.disconnect();
  }, []);
  const fitScale = Math.min((box.w - 24) / imgSize.w, (box.h - 24) / imgSize.h, 1.5);
  const fillRatio = fitScale > 0 ? Math.max(box.w / (imgSize.w * fitScale), box.h / (imgSize.h * fitScale), 1) : 1;
  const scale = fitScale * zoom;
  const disp = { w: Math.max(1, imgSize.w * scale), h: Math.max(1, imgSize.h * scale) };

  const clampPan = useCallback(
    (px: number, py: number, z = zoom, bw = box.w, bh = box.h) => {
      const fs = Math.min((bw - 24) / imgSize.w, (bh - 24) / imgSize.h, 1.5);
      const w = Math.max(1, imgSize.w * fs * z);
      const h = Math.max(1, imgSize.h * fs * z);
      const maxX = Math.max((bw - 48) * 0.45, (w - bw) / 2 + 48);
      const maxY = Math.max((bh - 48) * 0.45, (h - bh) / 2 + 48);
      return { x: clamp(px, -maxX, maxX), y: clamp(py, -maxY, maxY) };
    },
    [zoom, box.w, box.h, imgSize.w, imgSize.h],
  );

  useEffect(() => {
    setPan((prev) => (prev.x === 0 && prev.y === 0 ? prev : clampPan(prev.x, prev.y, zoom)));
  }, [box.w, box.h, imgSize.w, imgSize.h, zoom, clampPan]);

  const zoomTo = useCallback(
    (target: number, anchor?: { clientX: number; clientY: number }) => {
      const z1 = Math.round(clamp(target, 0.5, 4) * 100) / 100;
      setZoom((z0) => {
        const r = boxRef.current?.getBoundingClientRect();
        const cx = anchor && r ? anchor.clientX - (r.left + r.width / 2) : 0;
        const cy = anchor && r ? anchor.clientY - (r.top + r.height / 2) : 0;
        const ratio = z1 / z0;
        setPan((p0) => (z1 === 1 && !anchor ? { x: 0, y: 0 } : clampPan(cx - (cx - p0.x) * ratio, cy - (cy - p0.y) * ratio, z1)));
        return z1;
      });
    },
    [clampPan],
  );

  const resetView = useCallback(() => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
  }, []);

  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      if (e.shiftKey && !e.ctrlKey && !e.metaKey) {
        setPan((p0) => clampPan(p0.x - e.deltaX - e.deltaY, p0.y));
        return;
      }
      const factor = Math.exp(-e.deltaY * (e.ctrlKey || e.metaKey ? 0.01 : 0.0025));
      zoomTo(zoom * factor, { clientX: e.clientX, clientY: e.clientY });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoom, zoomTo, clampPan]);

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
  // AI Select: each clicked object becomes its own selection (⌥-click subtracts from the active one)
  const aiSelect = async (x: number, y: number, subtract: boolean) => {
    if (id == null || !p.settings || picking) return;
    await flush();
    setPicking({ x, y });
    setError(null);
    try {
      const r = await api.aiSelect(id, x, y, p.settings.active_provider, subtract ? maskSel : null, subtract);
      if (idRef.current === id) (setDev(r.develop), setMaskSel(r.mask), setCompSel(0), setOverlay(true), refreshHistory(id));
    } catch (e) {
      setError(`AI Select: ${e}`);
    } finally {
      setPicking(null);
    }
  };

  // Advanced: Upright and auto CA are measured by the engine; the result lands on the latest state
  const upright = async (mode: string, d: Dev | null = dev) => {
    if (id == null || !d) return;
    setError(null);
    try {
      const t = await api.upright(id, d, mode);
      if (idRef.current === id && devRef.current) change({ ...devRef.current, transform: t }, `Upright: ${mode}`);
    } catch (e) {
      setError(`Upright: ${e}`);
    }
  };
  const autoCa = async () => {
    if (id == null) return;
    setError(null);
    try {
      const [rc, by] = await api.autoCa(id);
      const cur = devRef.current;
      if (idRef.current === id && cur) change({ ...cur, lens: { ...cur.lens, ca_red_cyan: rc, ca_blue_yellow: by } }, "Auto chromatic aberration");
    } catch (e) {
      setError(`Chromatic aberration: ${e}`);
    }
  };
  // Pro: Photoshop's Auto Levels (each channel's black / white point, 0.1% clipped), measured
  // on a render without the current levels so pressing it again does not undo it
  const autoLevels = async () => {
    if (id == null || !dev) return;
    setError(null);
    try {
      const { hist: h } = await api.render(id, { ...dev, levels: p.defaults.levels }, false, null);
      const cur = devRef.current;
      if (idRef.current !== id || !cur) return;
      const at = (c: number, q: number) => {
        const n = h[c].reduce((a, b) => a + b, 0) * q;
        let acc = 0;
        return Math.max(0, h[c].findIndex((v) => (acc += v) > n));
      };
      const lv = structuredClone(p.defaults.levels);
      (["red", "green", "blue"] as const).forEach((c, k) => Object.assign(lv[c], { black: at(k, 0.001), white: at(k, 0.999) }));
      change({ ...cur, levels: lv }, "Auto levels");
    } catch (e) {
      setError(`Auto levels: ${e}`);
    }
  };
  // Spot removal: the engine picks a matching source area and commits the heal
  const healSpot = async (x: number, y: number) => {
    if (id == null || picking) return;
    await flush();
    setPicking({ x, y });
    setError(null);
    try {
      const d = await api.healSpot(id, x, y, spotSize / 100);
      if (idRef.current === id) (setDev(d), refreshHistory(id));
    } catch (e) {
      setError(`Spot removal: ${e}`);
    } finally {
      setPicking(null);
    }
  };
  // Guided Upright: drag a line on the full frame; from the second guide on, solve
  const drawGuide = (e: React.PointerEvent) => {
    const r = wrap.current!.getBoundingClientRect();
    const at = (ev: { clientX: number; clientY: number }) => [clamp((ev.clientX - r.left) / r.width, 0, 1), clamp((ev.clientY - r.top) / r.height, 0, 1)];
    const a = at(e);
    const move = (ev: PointerEvent) => setDraft([a, at(ev)]);
    window.addEventListener("pointermove", move);
    window.addEventListener(
      "pointerup",
      (ev) => {
        window.removeEventListener("pointermove", move);
        setDraft(null);
        const b = at(ev);
        const cur = devRef.current;
        if (!cur || Math.hypot((b[0] - a[0]) * r.width, (b[1] - a[1]) * r.height) < 12) return;
        const d = setPath(cur, ["transform", "guides"], [...cur.transform.guides, [a, b]].slice(-4));
        change(d, "Add guide");
        if (d.transform.guides.length >= 2) upright("guided", d);
      },
      { once: true },
    );
  };
  const switchTab = (t: typeof rtab) => {
    setRtab(t);
    if (t === "advanced" || t === "auto") setMaskSel(null); // these panels edit the whole photo
    if (tool === "guides" || tool === "spot") setTool("none");
  };

  const others = p.selected.filter((s) => s !== id);
  const sync = async () => {
    if (!dev) return;
    await flush();
    for (const o of others) await api.save(o, paste(await api.getDevelop(o), dev), "Sync settings");
  };

  // Image orientation helpers: keep the cropped region anchored when rotating or flipping
  const turn = (dir: 1 | -1) => {
    const cur = devRef.current;
    if (!cur) return;
    const c = cur.crop;
    const r = cur.image_rotation;
    const crop = dir === 1 ? { ...c, x: 1 - c.y - c.h, y: c.x, w: c.h, h: c.w } : { ...c, x: c.y, y: 1 - c.x - c.w, w: c.h, h: c.w };
    change(
      {
        ...cur,
        image_rotation: { ...r, angle: (r.angle + (r.flip_h !== r.flip_v ? -dir : dir) * 90 + 360) % 360 },
        crop,
      },
      dir === 1 ? "Rotate 90° clockwise" : "Rotate 90° counter-clockwise",
    );
  };
  const flip = (axis: "h" | "v") => {
    const cur = devRef.current;
    if (!cur) return;
    const c = cur.crop;
    const r = cur.image_rotation;
    change(
      {
        ...cur,
        image_rotation: axis === "h" ? { ...r, flip_h: !r.flip_h } : { ...r, flip_v: !r.flip_v },
        crop: axis === "h" ? { ...c, x: 1 - c.x - c.w } : { ...c, y: 1 - c.y - c.h },
      },
      axis === "h" ? "Flip horizontal" : "Flip vertical",
    );
  };

  const keys = useRef<(e: KeyboardEvent) => void>(() => {});
  keys.current = (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "z") (e.preventDefault(), undo(e.shiftKey ? 1 : -1));
    else if (mod && e.shiftKey && e.key.toLowerCase() === "c" && dev) (e.preventDefault(), (clipboard = structuredClone(dev)));
    else if (mod && e.shiftKey && e.key.toLowerCase() === "v" && dev && clipboard) (e.preventDefault(), change(paste(dev, clipboard), "Paste settings"));
    else if (mod && (e.key === "=" || e.key === "+")) (e.preventDefault(), zoomTo(zoom * 1.25));
    else if (mod && e.key === "-") (e.preventDefault(), zoomTo(zoom / 1.25));
    else if (mod && e.key === "0") (e.preventDefault(), resetView());
    else if (mod && e.key === "1") (e.preventDefault(), zoom > 1.05 || pan.x !== 0 || pan.y !== 0 ? resetView() : zoomTo(2));
    else if (mod) return;
    else if (e.code === "Space") (e.preventDefault(), setSpaceHeld(true));
    else if (e.key === "=" || e.key === "+") (e.preventDefault(), zoomTo(zoom * 1.25));
    else if (e.key === "-") (e.preventDefault(), zoomTo(zoom / 1.25));
    else if (e.key.toLowerCase() === "f") (e.preventDefault(), resetView());
    else if (e.key.toLowerCase() === "z") (e.preventDefault(), zoom > 1.05 || pan.x !== 0 || pan.y !== 0 ? resetView() : zoomTo(2));
    else if (e.key === "[" || e.key === "]") (e.preventDefault(), turn(e.key === "]" ? 1 : -1));
    else if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key) && (document.activeElement === boxRef.current || zoom > 1.01 || pan.x !== 0 || pan.y !== 0)) {
      e.preventDefault();
      const step = e.shiftKey ? 60 : 24;
      const dx = e.key === "ArrowLeft" ? step : e.key === "ArrowRight" ? -step : 0;
      const dy = e.key === "ArrowUp" ? step : e.key === "ArrowDown" ? -step : 0;
      setPan((p0) => clampPan(p0.x + dx, p0.y + dy));
    } else if (e.key === "\\") setBefore((b) => !b);
    else if (e.key.toLowerCase() === "r") setTool((t) => (t === "crop" ? "none" : "crop"));
    else if (e.key.toLowerCase() === "s") setTool((t) => (t === "select" ? "none" : "select"));
    else if (e.key.toLowerCase() === "o" && maskSel != null) setOverlay((o) => !o);
    else if (e.key.toLowerCase() === "q") (setRtab("advanced"), setMaskSel(null), setTool((t) => (t === "spot" ? "none" : "spot")));
    else if (e.key.toLowerCase() === "j") setProof((v) => ({ ...v, clipping: !v.clipping }));
    else if (e.key === "Escape") (setTool("none"), setMaskSel(null));
  };
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (!["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName)) keys.current(e);
    };
    const up = (e: KeyboardEvent) => {
      if (e.code === "Space") setSpaceHeld(false);
    };
    const blur = () => setSpaceHeld(false);
    window.addEventListener("keydown", h);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", blur);
    return () => {
      window.removeEventListener("keydown", h);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", blur);
    };
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
    const x = clamp(Math.floor(((e.clientX - r.left) / r.width) * c.width), 0, Math.max(0, c.width - 1));
    const y = clamp(Math.floor(((e.clientY - r.top) / r.height) * c.height), 0, Math.max(0, c.height - 1));
    const [R, G, B] = c.getContext("2d")!.getImageData(x, y, 1, 1).data;
    return [R, G, B];
  };
  const onCanvasClick = (e: React.MouseEvent) => {
    if (suppressClick.current || spaceHeld) {
      suppressClick.current = false;
      return;
    }
    if (!dev || !T) return;
    if (tool === "select" || tool === "spot") {
      const r = canvas.current!.getBoundingClientRect();
      const [x, y] = [clamp((e.clientX - r.left) / r.width, 0, 1), clamp((e.clientY - r.top) / r.height, 0, 1)];
      if (tool === "spot") healSpot(x, y);
      else aiSelect(x, y, e.altKey);
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

  // Click-and-drag pan across the photo viewport: only capture the pointer after >4px movement
  // so normal clicks on <canvas> (AI Select, Spot Heal, Swatch) are never redirected to boxRef
  const onBoxPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 && e.button !== 1) return;
    if ((e.target as HTMLElement).closest("button, input, select, textarea, a, [data-no-pan]")) return;
    if ((tool === "guides" || tool === "crop") && !spaceHeld && e.button === 0) return;
    boxRef.current?.focus();
    suppressClick.current = false;
    panStart.current = { id: e.pointerId, x: e.clientX, y: e.clientY, px: pan.x, py: pan.y, moved: false };
  };
  const onBoxPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const st = panStart.current;
    if (!st || st.id !== e.pointerId) return;
    const dx = e.clientX - st.x;
    const dy = e.clientY - st.y;
    if (!st.moved && Math.hypot(dx, dy) <= 4) return;
    if (!st.moved) {
      st.moved = true;
      suppressClick.current = true;
      e.currentTarget.setPointerCapture(e.pointerId);
      setDragging(true);
    }
    setPan(clampPan(st.px + dx, st.py + dy));
  };
  const endBoxPan = (pointerId: number) => {
    if (panStart.current?.id === pointerId) {
      const wasMoved = panStart.current.moved;
      panStart.current = null;
      setDragging(false);
      if (wasMoved) {
        setTimeout(() => {
          suppressClick.current = false;
        }, 0);
      }
    }
  };
  const onBoxPointerUp = (e: React.PointerEvent<HTMLDivElement>) => endBoxPan(e.pointerId);
  const onBoxLostCapture = (e: React.PointerEvent<HTMLDivElement>) => endBoxPan(e.pointerId);
  const onBoxDoubleClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if ((e.target as HTMLElement).closest("button, input, select, textarea, a, [data-no-pan]")) return;
    if (tool !== "none" && !spaceHeld) return;
    if (zoom > 1.05 || pan.x !== 0 || pan.y !== 0) resetView();
    else zoomTo(2, { clientX: e.clientX, clientY: e.clientY });
  };

  const photoIdx = p.photos.findIndex((ph) => ph.id === id);
  const stepPhoto = (dir: -1 | 1) => {
    const next = p.photos[photoIdx + dir];
    if (next) p.setActive(next.id);
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

  // --- auto: what the engine measures on the photo lands on the latest state as one step ---
  const measured = async <R,>(label: string, get: (id: number, cur: Dev) => Promise<R>, put: (cur: Dev, r: R) => Dev) => {
    if (id == null || !devRef.current) return;
    setError(null);
    try {
      const r = await get(id, devRef.current);
      const cur = devRef.current;
      if (idRef.current === id && cur) change(put(cur, r), label);
    } catch (e) {
      setError(`${label}: ${e}`);
    }
  };
  const autoTone = () => measured("Auto tone", api.autoTone, (cur, basic_tone) => ({ ...cur, basic_tone }));
  const autoWb = () => measured("Auto white balance", api.autoWhiteBalance, (cur, white_balance) => ({ ...cur, white_balance }));
  const autoProfile = (profile: string, label: string) => measured(`Auto: ${label}`, (i, cur) => api.autoDevelop(i, cur, profile), (_, d) => d);
  // Auto › remove: the engine heals the selection's objects away and commits the step
  const removeSelection = async (i: number) => {
    if (id == null) return;
    await flush();
    setError(null);
    try {
      const d = await api.removeSelection(id, i);
      if (idRef.current === id) (setDev(d), setMaskSel(null), refreshHistory(id));
    } catch (e) {
      setError(`Remove: ${e}`);
    }
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
              <span className="px-2 py-1 text-violet-300">{picking ? "✦ Finding the object…" : mask ? "Click another object to select it · ⌥-click to subtract · Esc when done" : "Click an object to select it"}</span>
            ) : null}
            {tool === "guides" ? <span className="px-2 py-1 text-amber-300">Drag along lines that should be vertical or horizontal ({dev?.transform.guides.length ?? 0}/4) · Esc when done</span> : null}
            {tool === "spot" ? <span className="px-2 py-1 text-sky-300">{picking ? "Healing…" : "Click a spot to heal it · Esc when done"}</span> : null}
            {(proof.gamut || proof.clipping) && (
              <Btn active onClick={() => setProof({ ...proof, gamut: null, clipping: false })} title="Soft-proofing overlays are on; click to turn them off">
                {[proof.gamut && `Gamut ${proof.gamut}`, proof.clipping && "Clipping"].filter(Boolean).join(" · ")} ✕
              </Btn>
            )}
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

        <div
          ref={boxRef}
          tabIndex={0}
          aria-label="Photo viewport (drag or use arrow keys to pan, scroll to zoom, double-click to toggle zoom)"
          onPointerDown={onBoxPointerDown}
          onPointerMove={onBoxPointerMove}
          onPointerUp={onBoxPointerUp}
          onPointerCancel={onBoxPointerUp}
          onLostPointerCapture={onBoxLostCapture}
          onDoubleClick={onBoxDoubleClick}
          style={{ WebkitUserSelect: "none", userSelect: "none", touchAction: "none" }}
          className={`checker relative flex min-h-0 flex-1 items-center justify-center overflow-hidden outline-none focus-visible:ring-1 focus-visible:ring-sky-500/60 ${
            dragging ? "cursor-grabbing" : spaceHeld || tool === "none" ? "cursor-grab" : ""
          }`}
        >
          <div ref={wrap} className="relative shrink-0 overflow-hidden shadow-2xl" style={{ width: disp.w, height: disp.h, transform: `translate(${pan.x}px, ${pan.y}px)` }}>
            <canvas
              ref={canvas}
              onClick={onCanvasClick}
              onPointerDown={tool === "guides" && !showBefore && !spaceHeld ? drawGuide : undefined}
              className={`block h-full w-full ${
                picking ? "cursor-wait" : dragging ? "cursor-grabbing" : spaceHeld ? "cursor-grab" : tool !== "none" && tool !== "crop" ? "cursor-crosshair" : ""
              }`}
            />
            {tool === "guides" && dev && !showBefore && (
              <svg viewBox="0 0 1 1" preserveAspectRatio="none" className="pointer-events-none absolute inset-0 h-full w-full" style={{ filter: "drop-shadow(0 0 1px #000)" }}>
                {[...dev.transform.guides, ...(draft ? [draft] : [])].map((g, i) => (
                  <line key={i} x1={g[0][0]} y1={g[0][1]} x2={g[1][0]} y2={g[1][1]} stroke={i < dev.transform.guides.length ? "#fbbf24" : "white"} strokeWidth={2} vectorEffect="non-scaling-stroke" />
                ))}
              </svg>
            )}
            {tool === "spot" && dev && !showBefore && (
              <svg viewBox="0 0 1 1" preserveAspectRatio="none" className="pointer-events-none absolute inset-0 h-full w-full" style={{ filter: "drop-shadow(0 0 1px #000)" }}>
                {dev.retouch.map((r, i) => (
                  <g key={i} fill="none">
                    <polygon points={r.points.map((q) => q.join(",")).join(" ")} stroke="white" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
                    <polygon points={r.points.map((q) => `${q[0] + r.source[0]},${q[1] + r.source[1]}`).join(" ")} stroke="#7dd3fc" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />
                  </g>
                ))}
              </svg>
            )}
            {picking && (
              <span
                className="pointer-events-none absolute h-10 w-10 -translate-x-1/2 -translate-y-1/2 animate-ping rounded-full border-2 border-violet-300"
                style={{ left: pct(picking.x), top: pct(picking.y) }}
              />
            )}
            {tool === "crop" && dev && !showBefore && (
              <div
                data-no-pan
                className="absolute cursor-move border border-white/80"
                style={{ left: pct(dev.crop.x), top: pct(dev.crop.y), width: pct(dev.crop.w), height: pct(dev.crop.h), boxShadow: "0 0 0 9999px rgba(0,0,0,.55)" }}
                onPointerDown={(e) => ((cropStart.current = { ...dev.crop }), cropDrag("move")(e))}
              >
                <div className="pointer-events-none absolute inset-0" style={{ backgroundImage: "linear-gradient(90deg,transparent 33%,rgba(255,255,255,.25) 33%,rgba(255,255,255,.25) 33.4%,transparent 33.4%,transparent 66.4%,rgba(255,255,255,.25) 66.4%,rgba(255,255,255,.25) 66.8%,transparent 66.8%),linear-gradient(transparent 33%,rgba(255,255,255,.25) 33%,rgba(255,255,255,.25) 33.4%,transparent 33.4%,transparent 66.4%,rgba(255,255,255,.25) 66.4%,rgba(255,255,255,.25) 66.8%,transparent 66.8%)" }} />
                {["nw", "ne", "sw", "se"].map((m) => (
                  <div
                    key={m}
                    data-no-pan
                    className="absolute h-3 w-3 border-2 border-white bg-neutral-900"
                    style={{ left: m.includes("w") ? -6 : undefined, right: m.includes("e") ? -6 : undefined, top: m.includes("n") ? -6 : undefined, bottom: m.includes("s") ? -6 : undefined, cursor: `${m}-resize` }}
                    onPointerDown={(e) => ((cropStart.current = { ...dev.crop }), cropDrag(m)(e))}
                  />
                ))}
              </div>
            )}
            {mask && handlesOn && !showBefore && tool !== "crop" && (
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
              !showBefore &&
              tool !== "crop" &&
              mask.components.map((c, ci) =>
                c.type === "linear_gradient" ? (
                  <Fragment key={ci}>
                    <div data-no-pan className={`${dot} bg-sky-500/80`} title="Full effect" style={{ left: pct(c.start[0]), top: pct(c.start[1]) }} onPointerDown={grab(ci, compDrag(ci, "start"))} />
                    <div data-no-pan className={`${dot} bg-neutral-900/80`} title="No effect" style={{ left: pct(c.end[0]), top: pct(c.end[1]) }} onPointerDown={grab(ci, compDrag(ci, "end"))} />
                  </Fragment>
                ) : c.type === "radial_gradient" ? (
                  <Fragment key={ci}>
                    <div data-no-pan className={`${dot} bg-sky-500/80`} style={{ left: pct(c.center[0]), top: pct(c.center[1]) }} onPointerDown={grab(ci, compDrag(ci, "center"))} />
                    <div data-no-pan className={`${dot} bg-neutral-900/80`} style={{ left: pct(c.center[0] + c.radius[0]), top: pct(c.center[1]) }} onPointerDown={grab(ci, compDrag(ci, "radius", 0))} />
                    <div data-no-pan className={`${dot} bg-neutral-900/80`} style={{ left: pct(c.center[0]), top: pct(c.center[1] + c.radius[1]) }} onPointerDown={grab(ci, compDrag(ci, "radius", 1))} />
                  </Fragment>
                ) : null,
              )}
          </div>
          {(loading || error) && (
            <div className="absolute inset-x-0 top-3 mx-auto w-fit rounded bg-neutral-900/90 px-3 py-1.5 text-[11px] text-neutral-300">{error ? <span className="text-red-400">{error}</span> : "Loading…"}</div>
          )}
          {showBefore && <div className="absolute left-3 top-3 rounded bg-neutral-900/80 px-2 py-1 text-[11px]">Before</div>}

          {/* Floating viewport HUD: zoom, pan/fit, orientation, photo navigation, and hold-to-compare */}
          <div
            data-no-pan
            onPointerDown={(e) => e.stopPropagation()}
            onDoubleClick={(e) => e.stopPropagation()}
            className="absolute bottom-3 left-1/2 z-10 flex -translate-x-1/2 items-center gap-1 rounded-full border border-neutral-700/80 bg-neutral-900/90 px-2.5 py-1 text-[11px] text-neutral-200 shadow-xl backdrop-blur"
          >
            <button
              className="rounded px-1.5 py-0.5 text-neutral-300 hover:bg-neutral-800 hover:text-white disabled:opacity-30"
              disabled={photoIdx <= 0}
              onClick={() => stepPhoto(-1)}
              title="Previous photo (←)"
            >
              ‹
            </button>
            <button
              className="rounded px-1.5 py-0.5 text-neutral-300 hover:bg-neutral-800 hover:text-white disabled:opacity-30"
              disabled={photoIdx < 0 || photoIdx >= p.photos.length - 1}
              onClick={() => stepPhoto(1)}
              title="Next photo (→)"
            >
              ›
            </button>
            <span className="mx-0.5 h-3.5 w-px bg-neutral-700" />
            <button
              className="rounded px-1.5 py-0.5 text-neutral-300 hover:bg-neutral-800 hover:text-white"
              onClick={() => zoomTo(zoom / 1.25)}
              title="Zoom out (−)"
            >
              −
            </button>
            <input
              type="range"
              min={0.5}
              max={4}
              step={0.05}
              value={Math.min(4, zoom)}
              onChange={(e) => zoomTo(Number(e.target.value))}
              className="w-20"
              title="Zoom level (scroll wheel or pinch on photo)"
            />
            <button
              className="min-w-[42px] rounded px-1 py-0.5 text-center font-mono text-[10px] text-neutral-200 hover:bg-neutral-800"
              onClick={() => (zoom > 1.05 || pan.x !== 0 || pan.y !== 0 ? resetView() : zoomTo(2))}
              title="Click to toggle Fit / 200% zoom (Z or double-click photo)"
            >
              {Math.round(zoom * 100)}%
            </button>
            <button
              className="rounded px-1.5 py-0.5 text-neutral-300 hover:bg-neutral-800 hover:text-white"
              onClick={() => zoomTo(zoom * 1.25)}
              title="Zoom in (+)"
            >
              +
            </button>
            <button
              className={`rounded px-1.5 py-0.5 text-[10px] ${Math.abs(zoom - 1) < 0.03 && pan.x === 0 && pan.y === 0 ? "bg-sky-600 text-white" : "text-neutral-300 hover:bg-neutral-800"}`}
              onClick={resetView}
              title="Fit to view & center (F or ⌘0)"
            >
              Fit
            </button>
            <button
              className={`rounded px-1.5 py-0.5 text-[10px] ${Math.abs(zoom - fillRatio) < 0.05 && fillRatio > 1.02 ? "bg-sky-600 text-white" : "text-neutral-300 hover:bg-neutral-800"}`}
              onClick={() => (setPan({ x: 0, y: 0 }), zoomTo(fillRatio))}
              title="Fill viewport"
            >
              Fill
            </button>
            <button
              className={`rounded px-1.5 py-0.5 text-[10px] ${Math.abs(zoom - 2) < 0.05 ? "bg-sky-600 text-white" : "text-neutral-300 hover:bg-neutral-800"}`}
              onClick={() => zoomTo(2)}
              title="Zoom 200% (Z or ⌘1)"
            >
              200%
            </button>
            {(pan.x !== 0 || pan.y !== 0) && (
              <button
                className="rounded px-1.5 py-0.5 text-[10px] text-sky-300 hover:bg-neutral-800"
                onClick={() => setPan({ x: 0, y: 0 })}
                title="Re-center panned image"
              >
                Center
              </button>
            )}
            <span className="mx-0.5 h-3.5 w-px bg-neutral-700" />
            <button className="rounded px-1.5 py-0.5 text-neutral-300 hover:bg-neutral-800 hover:text-white" onClick={() => turn(-1)} title="Rotate 90° counter-clockwise ([)">
              ⟲
            </button>
            <button className="rounded px-1.5 py-0.5 text-neutral-300 hover:bg-neutral-800 hover:text-white" onClick={() => turn(1)} title="Rotate 90° clockwise (])">
              ⟳
            </button>
            <button
              className={`rounded px-1.5 py-0.5 ${dev?.image_rotation.flip_h ? "bg-sky-600 text-white" : "text-neutral-300 hover:bg-neutral-800 hover:text-white"}`}
              onClick={() => flip("h")}
              title="Flip horizontal"
            >
              ⇋
            </button>
            <button
              className={`rounded px-1.5 py-0.5 ${dev?.image_rotation.flip_v ? "bg-sky-600 text-white" : "text-neutral-300 hover:bg-neutral-800 hover:text-white"}`}
              onClick={() => flip("v")}
              title="Flip vertical"
            >
              ⇵
            </button>
            <span className="mx-0.5 h-3.5 w-px bg-neutral-700" />
            <button
              className={`rounded px-2 py-0.5 text-[10px] ${showBefore ? "bg-amber-600 text-white" : "text-neutral-300 hover:bg-neutral-800 hover:text-white"}`}
              onPointerDown={() => setPeekingBefore(true)}
              onPointerUp={() => setPeekingBefore(false)}
              onPointerLeave={() => setPeekingBefore(false)}
              onPointerCancel={() => setPeekingBefore(false)}
              title="Hold to peek at Original / Before (or press \ to toggle)"
            >
              Hold: Before
            </button>
          </div>
        </div>

        <AiBar settings={p.settings} setSettings={p.setSettings} run={aiRun} logs={p.logs} clearLogs={p.clearLogs} ask={ask} />

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
              Tuning{" "}
              {dev && dev.masks.length > 1 ? (
                <select
                  className="max-w-[110px] rounded bg-violet-900/80 px-1 py-0.5 font-semibold text-white outline-none"
                  value={maskSel ?? 0}
                  onChange={(e) => (setMaskSel(Number(e.target.value)), setCompSel(0))}
                >
                  {dev.masks.map((mk, i) => (
                    <option key={mk.mask_id || i} value={i}>
                      {mk.name}
                    </option>
                  ))}
                </select>
              ) : (
                <b className="text-white">{mask.name}</b>
              )}{" "}
              only
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
        <div className="border-b border-neutral-800 px-3 py-2">
          <Seg
            options={[["adjust", "Adjust"], ["selections", dev?.masks.length ? `Selections (${dev.masks.length})` : "Selections"], ["advanced", "Advanced"], ["pro", "Pro"], ["auto", "Auto"]]}
            value={rtab}
            onChange={switchTab}
          />
        </div>
        {dev && rtab === "advanced" && (
          <AdvancedTab
            dev={dev}
            defaults={p.defaults}
            change={change}
            S={slider(dev, change)}
            tool={tool}
            setTool={setTool}
            upright={(m) => upright(m)}
            autoCa={autoCa}
            proof={proof}
            setProof={setProof}
            spotSize={spotSize}
            setSpotSize={setSpotSize}
          />
        )}
        {dev && T && rtab === "pro" && <ProTab dev={T} edit={edit} S={S} reset={reset} whole={!mask} autoLevels={autoLevels} />}
        {dev && rtab === "auto" && (
          <AutoTab
            dev={dev}
            profile={autoProfile}
            whiteBalance={autoWb}
            tone={autoTone}
            levels={autoLevels}
            ca={autoCa}
            upright={upright}
            remove={removeSelection}
            ask={(text) => setAsk({ text, n: Date.now() })}
          />
        )}
        {dev && rtab === "selections" && (
          <SelectionsTab
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
            selecting={tool === "select"}
            toggleSelecting={() => setTool(tool === "select" ? "none" : "select")}
            onTune={() => switchTab("adjust")}
          />
        )}
        {dev && T && rtab === "adjust" && (
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
                  <div className="mb-2 grid grid-cols-4 gap-1">
                    <Btn onClick={() => turn(-1)} title="Rotate 90° counter-clockwise ([)">
                      ⟲ 90°
                    </Btn>
                    <Btn onClick={() => turn(1)} title="Rotate 90° clockwise (])">
                      ⟳ 90°
                    </Btn>
                    <Btn active={dev.image_rotation.flip_h} onClick={() => flip("h")} title="Flip horizontal">
                      ⇋ Flip H
                    </Btn>
                    <Btn active={dev.image_rotation.flip_v} onClick={() => flip("v")} title="Flip vertical">
                      ⇵ Flip V
                    </Btn>
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
          </>
        )}
      </aside>
    </div>
  );
}

/** Stored selections (masks). Each AI-selected object is its own selection that can be selected
 *  to finetune in Adjust, unselected, toggled on/off or deleted without a new AI run. */
function SelectionsTab({
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
  selecting,
  toggleSelecting,
  onTune,
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
  selecting: boolean;
  toggleSelecting: () => void;
  onTune: () => void;
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
  const remove = (i: number) => {
    change({ ...dev, masks: dev.masks.filter((_, k) => k !== i) }, "Delete selection");
    if (sel === i) setSel(null);
    else if (sel != null && sel > i) setSel(sel - 1);
  };
  const m = sel != null ? dev.masks[sel] : null;
  const field = "rounded bg-neutral-800 px-1 py-0.5 text-[11px]";
  return (
    <div className="space-y-2 p-3 text-[11px]">
      <Btn active={selecting} onClick={toggleSelecting} className="w-full" title="Click objects on the photo to select them with the AI (S)">
        {selecting ? "✦ Click objects on the photo… (S to stop)" : "✦ Select objects with AI"}
      </Btn>
      <p className="text-[10px] text-neutral-500">
        {dev.masks.length
          ? "Each AI-selected object is listed below. Select either one to finetune only it in Adjust (or Unselect when done) — no new AI run; the checkbox turns its effect on/off, and × deletes it."
          : "No selections yet. Each object you select with AI is stored separately here so you can select any of them to finetune later."}
      </p>
      {dev.masks.map((mk, i) => {
        const tuned = !!mk.tuning || Object.values(mk.adjustments).some((v) => v !== 0);
        return (
          <div key={mk.mask_id || i} className={`space-y-1 rounded border p-1.5 ${sel === i ? "border-violet-500/70 bg-neutral-800" : "border-neutral-800"}`}>
            <div className="flex items-center gap-1.5">
              <input
                type="checkbox"
                checked={mk.enabled}
                title="Effect on / off (leave out or include without deleting)"
                onChange={(e) => change(setPath(dev, ["masks", i, "enabled"], e.target.checked), "Toggle selection")}
              />
              <button
                type="button"
                onClick={() => setSel(sel === i ? null : i)}
                className={`min-w-0 flex-1 truncate text-left ${mk.enabled ? "hover:text-white" : "text-neutral-500"}`}
                title={sel === i ? "Back to the whole photo (Esc)" : `Select ${mk.name} to finetune it`}
              >
                {mk.name}
                {tuned && (
                  <span className="ml-1 text-violet-300" title="Has its own tuning">
                    ●
                  </span>
                )}
              </button>
              <Btn active={sel === i} onClick={() => setSel(sel === i ? null : i)} title={sel === i ? "Back to the whole photo (Esc)" : "Tune only this object"}>
                {sel === i ? "Unselect" : "Select"}
              </Btn>
              <button className="px-1 text-neutral-500 hover:text-white" title="Delete selection" onClick={() => remove(i)}>
                ✕
              </button>
            </div>
          </div>
        );
      })}
      {m && sel != null && (
        <div className="space-y-2 rounded border border-neutral-700 p-2">
          <div className="flex items-center gap-1">
            <input className={`${field} min-w-0 flex-1`} value={m.name} onChange={(e) => change(setPath(dev, ["masks", sel, "name"], e.target.value), "Rename selection")} />
            <Btn active={overlay} onClick={() => setOverlay(!overlay)} title="Show the selection (O)">
              Overlay
            </Btn>
            <Btn onClick={onTune} title="Open Adjust panels to finetune this object">
              Adjust →
            </Btn>
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
                <span className="flex-1 truncate text-neutral-300">{c.name || TYPE_LABEL[c.type] || c.type}</span>
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
      <div className="border-t border-neutral-800 pt-2">
        <p className="mb-1 text-[10px] text-neutral-500">Add a drawn mask</p>
        <div className="grid grid-cols-2 gap-1">
          {MASK_TYPES.map(([t, label]) => (
            <Btn key={t} onClick={() => add(t)}>
              + {label}
            </Btn>
          ))}
        </div>
      </div>
    </div>
  );
}

function AiBar({
  settings,
  setSettings,
  run: runAi,
  logs,
  clearLogs,
  ask,
}: {
  settings: Settings | null;
  setSettings: (s: Settings) => void;
  run: (prompt: string, provider: string) => Promise<string>;
  logs: Log[];
  clearLogs?: () => void;
  /** a prompt to run as if typed here (the Auto tab's ✦ buttons); n tells repeats apart */
  ask?: { text: string; n: number } | null;
}) {
  const [prompt, setPrompt] = useState("");
  const runRef = useRef<(text: string) => void>(() => {});
  useEffect(() => {
    if (ask) runRef.current(ask.text);
  }, [ask]);
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

  const run = async (text = prompt) => {
    if (!text.trim() || busy) return;
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
      setResult({ ok: true, text: await runAi(text, provider) });
    } catch (e) {
      setResult({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
      setElapsed((Date.now() - now) / 1000);
    }
  };
  runRef.current = (text) => (setPrompt(text), run(text));

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
        <Btn active onClick={() => run()} disabled={busy || !prompt.trim()}>
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
