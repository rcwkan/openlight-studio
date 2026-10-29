import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type Photo = {
  id: number;
  path: string;
  folder: string;
  filename: string;
  master_id: number | null;
  copy_name: string | null;
  width: number;
  height: number;
  captured_at: string | null;
  camera: string | null;
  lens: string | null;
  iso: number | null;
  aperture: number | null;
  shutter: string | null;
  focal_length: number | null;
  rating: number;
  flag: number;
  color_label: string;
  keywords: string;
  edited: boolean;
  thumb_v: number;
};

export type Filter = {
  folder?: string | null;
  text?: string | null;
  min_rating?: number | null;
  flag?: string | null;
  color_label?: string | null;
  edited?: boolean | null;
  /** Loaded pages of an Immich album (album id, or "favorites") */
  immich_album?: string | null;
  /** Sidebar row being shown; UI only, the backend ignores it */
  view?: string | null;
};

export type Culling = { rating?: number; flag?: string; color_label?: string; keywords?: string };

export type Wheel = { hue: number; sat: number; lum: number };
export type MaskComponent = {
  op: string;
  type: string;
  /** tag of an AI region, e.g. "castle" */
  name: string;
  invert: boolean;
  feather: number;
  edge_shift: number;
  start: number[];
  end: number[];
  center: number[];
  radius: number[];
  range: number[];
  color: number[];
  tolerance: number;
  /** polygon: AI outline [x, y] in the full frame */
  points: number[][];
};
/** Object removal: the outline is refilled from the area at `source` (normalised offset). */
export type Heal = { name: string; points: number[][]; source: number[] };
export type LocalAdjust = {
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  temperature_shift: number;
  tint_shift: number;
  saturation: number;
};
/** `tuning`: develop settings applied on top of the photo inside the mask only (absent = none). */
export type Mask = { mask_id: string; name: string; enabled: boolean; components: MaskComponent[]; adjustments: LocalAdjust; tuning?: Develop | null };
export type Swatch = { id: string; src_hsl: number[]; shift_hsl: number[]; range_hsl: number[]; falloff: number };
export type Level = { black: number; gamma: number; white: number; out_black: number; out_white: number };
/** Selective Color families, in Photoshop's order */
export const FAMILIES = ["reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks"] as const;

export type Develop = {
  profile: string;
  white_balance: { temperature_k: number; tint: number };
  basic_tone: { exposure: number; contrast: number; highlights: number; shadows: number; whites: number; blacks: number };
  presence: { texture: number; clarity: number; dehaze: number; vibrance: number; saturation: number };
  tone_curve: {
    parametric: { shadows: number; darks: number; lights: number; highlights: number; splits: number[] };
    point_luma: number[][];
    point_red: number[][];
    point_green: number[][];
    point_blue: number[][];
  };
  color_mixer: { hue: number[]; saturation: number[]; luminance: number[] };
  point_color: Swatch[];
  color_grading_3way: { shadows: Wheel; midtones: Wheel; highlights: Wheel; global: Wheel; blending: number; balance: number };
  calibration: { shadows_tint: number; red_hue: number; red_saturation: number; green_hue: number; green_saturation: number; blue_hue: number; blue_saturation: number };
  detail: { sharpening: number; sharpen_radius: number; sharpen_masking: number; color_noise_reduction: number; luminance_noise_reduction: number; luminance_detail: number };
  effects: { vignette: number; vignette_midpoint: number; vignette_feather: number };
  grain: { amount: number; size: number; roughness: number };
  lens: {
    distortion: number;
    ca_red_cyan: number;
    ca_blue_yellow: number;
    defringe_purple: number;
    defringe_purple_hue: number[];
    defringe_green: number;
    defringe_green_hue: number[];
    vignetting: number;
    vignetting_midpoint: number;
  };
  /** upright: the Upright mode that set vertical/horizontal/rotate; guides: [[x, y], [x, y]] in the full frame */
  transform: {
    upright: string;
    guides: number[][][];
    vertical: number;
    horizontal: number;
    rotate: number;
    aspect: number;
    scale: number;
    offset_x: number;
    offset_y: number;
    constrain_crop: boolean;
  };
  /** Pro (Photoshop adjustments). Levels in 0-255 display values; selective color [c, m, y, k]; mixer rows [r, g, b, constant] % */
  levels: Record<"rgb" | "red" | "green" | "blue", Level>;
  hue_saturation: { hue: number; saturation: number; lightness: number; colorize: boolean; colorize_hue: number; colorize_saturation: number };
  selective_color: { absolute: boolean } & Record<(typeof FAMILIES)[number], number[]>;
  channel_mixer: { red: number[]; green: number[]; blue: number[]; monochrome: boolean; gray: number[] };
  /** colours are sRGB 0-1 */
  gradient_map: { amount: number; shadows: number[]; highlights: number[] };
  filters: { blur: number; invert: boolean; posterize: number; threshold: number };
  /** angle 0/90/180/270 clockwise, then the mirrors; the crop is in the rotated frame */
  image_rotation: { angle: number; flip_h: boolean; flip_v: boolean };
  /** in the straightened, rotated frame */
  crop: { x: number; y: number; w: number; h: number; angle: number };
  retouch: Heal[];
  masks: Mask[];
  ai_provenance_log: { timestamp?: string; provider?: string; action?: string }[];
};

export type Provider = {
  id: string;
  kind: string;
  name: string;
  command: string;
  endpoint: string;
  model: string;
  has_key: boolean;
  timeout_s: number;
  max_passes: number;
};
export type Settings = {
  mcp_enabled: boolean;
  mcp_port: number;
  mcp_token: string;
  providers: Provider[];
  active_provider: string;
  immich_url: string;
  google_client_id: string;
};
export type Source = "immich" | "google";
export type ImmichAlbum = { id: string; name: string; count: number };
export type RemoteImport = { folder: string; imported: number; skipped: number; failed: number; error: string | null };
export type AppInfo = { data_dir: string; thumbs_dir: string; exe: string | null; mcp_url: string; mcp_status: string };
export type HistoryEntry = { id: number; label: string; created_at: string; current: boolean };
export type Rendered = { hist: Uint32Array[]; image: ImageData };
/** Preview overlays: soft-proof gamut (sRGB | DisplayP3 | AdobeRGB), clipping, Visualize Spots threshold (null = off). */
export type Proof = { gamut: string | null; clipping: boolean; spots: number | null };
/** upload: send the renders to Immich / Google Photos instead of keeping them in `folder` */
export type ExportReq = { ids: number[]; folder: string; format: string; color_space: string; quality: number; max_edge: number | null; upload?: Source | null };
export type CleanUp = "thumbs" | "originals" | "immich_albums" | "missing" | "history" | "compact" | "all";
type Named = { name: string; count: number }[];
export type ImmichAlbumStorage = { name: string; path: string; bytes: number; photos: number };
export type OtherDetail = { name: string; bytes: number };
export type CatalogDetails = { db: number; wal: number; shm: number; free: number };

/** Sizes in bytes. `folders` are local folders; an offline one is not there at all (e.g. an unplugged drive). */
export type StorageReport = {
  data_dir: string;
  total: number;
  catalog: number;
  catalog_details?: CatalogDetails;
  thumbs: number;
  originals: number;
  immich_albums?: ImmichAlbumStorage[];
  immich_albums_bytes?: number;
  google: number;
  other?: number;
  other_details?: OtherDetail[];
  summary: { total_photos: number; virtual_copies: number; edited: number; picks: number; rejects: number; rated_3_plus: number; cameras: Named; lenses: Named };
  immich_photos: number;
  history_steps: number;
  ai: { photos: number; edits: number };
  folders: { folder: string; photos: number; bytes: number; missing: number; online: boolean }[];
  clean: Record<CleanUp, { count: number; bytes: number }>;
};

function parseRender(buf: ArrayBuffer): Rendered {
  const dv = new DataView(buf);
  const w = dv.getUint32(0, true);
  const h = dv.getUint32(4, true);
  const hist = [0, 1, 2, 3].map((i) => new Uint32Array(buf.slice(8 + i * 1024, 8 + (i + 1) * 1024)));
  return { hist, image: new ImageData(new Uint8ClampedArray(buf, 8 + 4096, w * h * 4), w, h) };
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  query: (filter: Filter) => invoke<Photo[]>("query_photos", { filter }),
  photo: async (id: number) => (await invoke<Photo[]>("query_photos", { filter: { ids: [id] } }))[0],
  folders: () => invoke<[string, number][]>("list_folders"),
  importFolder: (path: string) => invoke<number>("import_folder", { path }),
  setSelection: (ids: number[]) => invoke("set_selection", { ids }),
  update: (ids: number[], update: Culling) => invoke<number>("update_photos", { ids, update }),
  virtualCopy: (id: number) => invoke<number>("virtual_copy", { id }),
  remove: (ids: number[]) => invoke("remove_photos", { ids }),
  getDevelop: (id: number) => invoke<Develop>("get_develop", { id }),
  defaultDevelop: () => invoke<Develop>("default_develop"),
  /** fullFrame: no crop or straighten, so view coordinates are mask coordinates */
  render: async (id: number, develop: Develop, skipCrop: boolean, overlayMask: number | null, fullFrame = false, proof: Proof | null = null) =>
    parseRender(await invoke<ArrayBuffer>("render_preview", { id, develop, skipCrop, overlayMask, fullFrame, proof })),
  /** Upright mode: off | auto | level | vertical | full | guided (uses develop.transform.guides) */
  upright: (id: number, develop: Develop, mode: string) => invoke<Develop["transform"]>("upright", { id, develop, mode }),
  /** Measured lateral chromatic aberration: [ca_red_cyan, ca_blue_yellow] */
  autoCa: (id: number) => invoke<[number, number]>("auto_ca", { id }),
  /** Heal a circle at (x, y) of the full frame; radius is a fraction of the long edge. Commits one history step. */
  healSpot: (id: number, x: number, y: number, radius: number) => invoke<Develop>("heal_spot", { id, x, y, radius }),
  /** Auto fine tune to a profile (natural | neutral | vivid | portrait | landscape | bw | film | moody); not saved */
  autoDevelop: (id: number, develop: Develop, profile: string) => invoke<Develop>("auto_develop", { id, develop, profile }),
  autoWhiteBalance: (id: number) => invoke<Develop["white_balance"]>("auto_white_balance", { id }),
  /** under develop's white balance */
  autoTone: (id: number, develop: Develop) => invoke<Develop["basic_tone"]>("auto_tone", { id, develop }),
  /** Heal away the AI-selected objects of selection `mask` and drop it. Commits one history step. */
  removeSelection: (id: number, mask: number) => invoke<Develop>("remove_selection", { id, mask }),
  save: (id: number, develop: Develop, label: string) => invoke<Develop>("save_develop", { id, develop, label }),
  history: (id: number) => invoke<HistoryEntry[]>("get_history", { id }),
  gotoHistory: (id: number, hid: number) => invoke<Develop>("goto_history", { id, hid }),
  undoRedo: (id: number, dir: number) => invoke<Develop | null>("undo_redo", { id, dir }),
  export: (req: ExportReq) => invoke<string[]>("export_photos", { req }),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke("save_settings", { settings }),
  regenerateToken: () => invoke<string>("regenerate_token"),
  setKey: (id: string, key: string) => invoke("set_provider_key", { id, key }),
  aiRun: (id: number, prompt: string, provider: string) => invoke<string>("ai_run", { id, prompt, provider }),
  /** AI selection: each clicked object at (x, y) of the full frame starts its own selection (or subtracts from `mask` on ⌥-click) */
  aiSelect: (id: number, x: number, y: number, provider: string, mask: number | null, subtract: boolean) =>
    invoke<{ develop: Develop; mask: number }>("ai_select", { id, x, y, provider, mask, subtract }),
  immichConnect: (url: string, key: string) => invoke<Settings>("immich_connect", { url, key }),
  immichAlbums: () => invoke<ImmichAlbum[]>("immich_albums"),
  /** Load one page of an album (null = favorites) into the catalog; page 1 starts it over */
  immichOpen: (album: string | null, page: number) => invoke<{ next: number | null }>("immich_open", { album, page }),
  googleConnect: (clientId: string, clientSecret: string) => invoke<Settings>("google_connect", { clientId, clientSecret }),
  googleImport: () => invoke<RemoteImport>("google_import"),
  remoteDisconnect: (source: Source) => invoke<Settings>("remote_disconnect", { source }),
  storage: () => invoke<StorageReport>("storage_report"),
  /** Returns how many files, photos or history steps went */
  cleanUp: (what: CleanUp) => invoke<number>("clean_up", { what }),
};

let thumbsDir = "";
export function setThumbsDir(dir: string) {
  thumbsDir = dir + (dir.includes("\\") ? "\\" : "/");
}
export const thumbUrl = (p: Photo) => `${convertFileSrc(`${thumbsDir}${p.id}.jpg`)}?v=${p.thumb_v}`;

export const FLAGS = ["reject", "unflagged", "pick"] as const;
export const flagName = (f: number) => FLAGS[f + 1];
export const LABELS = ["red", "yellow", "green", "blue", "purple"] as const;
export const LABEL_COLORS: Record<string, string> = {
  red: "#e5484d",
  yellow: "#f5d90a",
  green: "#46a758",
  blue: "#3e63dd",
  purple: "#8e4ec6",
};

/** Immutable set of a nested value, e.g. setPath(d, ["basic_tone", "exposure"], 1). */
export function setPath<T>(obj: T, path: (string | number)[], v: unknown): T {
  const copy = structuredClone(obj);
  let o = copy as Record<string | number, unknown>;
  for (const k of path.slice(0, -1)) o = o[k] as Record<string | number, unknown>;
  o[path[path.length - 1]] = v;
  return copy;
}
export function getPath(obj: unknown, path: (string | number)[]): unknown {
  return path.reduce<unknown>((o, k) => (o == null ? undefined : (o as Record<string | number, unknown>)[k]), obj);
}

/** sRGB 8-bit -> Oklch [hue°, chroma, L] (same maths as the Rust pipeline). */
export function srgbToOklch(r: number, g: number, b: number): [number, number, number] {
  const lin = [r, g, b].map((v) => {
    v /= 255;
    return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  const l = Math.cbrt(0.4122214708 * lin[0] + 0.5363325363 * lin[1] + 0.0514459929 * lin[2]);
  const m = Math.cbrt(0.2119034982 * lin[0] + 0.6806995451 * lin[1] + 0.1073969566 * lin[2]);
  const s = Math.cbrt(0.0883024619 * lin[0] + 0.2817188376 * lin[1] + 0.6299787005 * lin[2]);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  return [((Math.atan2(B, A) * 180) / Math.PI + 360) % 360, Math.hypot(A, B), L];
}

/** Monotone cubic through curve points (0–255), same as the Rust Spline. */
export function spline(pts: number[][]): (x: number) => number {
  const p = [...pts].sort((a, b) => a[0] - b[0]);
  const n = p.length;
  if (n < 2) return (x) => x;
  const x = p.map((q) => q[0]);
  const y = p.map((q) => q[1]);
  const d = x.slice(0, -1).map((_, k) => (y[k + 1] - y[k]) / Math.max(1e-6, x[k + 1] - x[k]));
  const m = x.map((_, k) => (k === 0 ? d[0] : k === n - 1 ? d[n - 2] : d[k - 1] * d[k] <= 0 ? 0 : (d[k - 1] + d[k]) / 2));
  for (let k = 0; k < n - 1; k++) {
    if (d[k] === 0) {
      m[k] = m[k + 1] = 0;
      continue;
    }
    const a = m[k] / d[k];
    const b = m[k + 1] / d[k];
    const s = a * a + b * b;
    if (s > 9) {
      const t = 3 / Math.sqrt(s);
      m[k] = t * a * d[k];
      m[k + 1] = t * b * d[k];
    }
  }
  return (v) => {
    if (v <= x[0]) return y[0];
    if (v >= x[n - 1]) return y[n - 1];
    let k = 0;
    while (x[k + 1] <= v) k++;
    const h = x[k + 1] - x[k];
    const t = (v - x[k]) / h;
    const t2 = t * t;
    const t3 = t2 * t;
    return (2 * t3 - 3 * t2 + 1) * y[k] + (t3 - 2 * t2 + t) * h * m[k] + (-2 * t3 + 3 * t2) * y[k + 1] + (t3 - t2) * h * m[k + 1];
  };
}
