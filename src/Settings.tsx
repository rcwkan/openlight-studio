import { ask } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { Agent, Log } from "./Agent";
import { api, AppInfo, CleanUp, JobProgress, Settings, size, Source, StorageReport } from "./api";
import { Btn } from "./controls";
import { Duplicate, Locations } from "./Storage";
import { applyTheme, getStoredTheme, isSystemDark, Theme, THEMES } from "./theme";

const card = "rounded-lg border border-neutral-800 bg-neutral-900 p-4";
const heading = "mb-1 text-[11px] font-semibold uppercase tracking-wider text-neutral-400";

const num = (n: number) => n.toLocaleString();
const plural = (n: number, word: string) => `${num(n)} ${word}${n === 1 ? "" : "s"}`;
const short = (folder: string) => folder.split(/[\\/]/).slice(-2).join("/");

export type SettingsTab = "appearance" | "locations" | "duplicate" | "usage" | "agent";
const TABS: [SettingsTab, string][] = [
  ["appearance", "Appearance"],
  ["locations", "Locations"],
  ["duplicate", "Duplicate"],
  ["usage", "Usage & clean up"],
  ["agent", "AI & MCP"],
];

/** Settings: Appearance & theme selection, storage locations, duplicate detection, library clean-up, and AI & MCP configuration. */
export function SettingsView(p: {
  tab: SettingsTab;
  setTab: (t: SettingsTab) => void;
  settings: Settings | null;
  setSettings: (s: Settings) => void;
  selected: number[];
  jobs: Record<string, JobProgress>;
  onConnect: (s: Source) => void;
  onAuto: () => void;
  info?: AppInfo | null;
  logs?: Log[];
  clearLogs?: () => void;
}) {
  const reload = () => void api.settings().then(p.setSettings);
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <nav className="flex shrink-0 items-center gap-1 border-b border-neutral-800 bg-neutral-900 px-4 py-1.5 text-[12px]">
        <span className="mr-3 font-semibold text-neutral-300">Settings</span>
        {TABS.map(([t, label]) => (
          <button key={t} onClick={() => p.setTab(t)} className={`rounded px-3 py-1 ${p.tab === t ? "bg-neutral-700 text-white" : "text-neutral-400 hover:text-neutral-200"}`}>
            {label}
          </button>
        ))}
      </nav>
      {p.settings && p.tab === "appearance" && <Appearance settings={p.settings} setSettings={p.setSettings} />}
      {p.settings && p.tab === "locations" && <Locations settings={p.settings} setSettings={p.setSettings} onConnect={p.onConnect} onAuto={p.onAuto} />}
      {p.settings && p.tab === "duplicate" && <Duplicate settings={p.settings} selected={p.selected} jobs={p.jobs} />}
      {p.tab === "usage" && <Usage onSettingsChanged={reload} />}
      {p.tab === "agent" && (
        <Agent
          info={p.info ?? null}
          settings={p.settings}
          setSettings={p.setSettings}
          logs={p.logs ?? []}
          clearLogs={p.clearLogs ?? (() => { })}
        />
      )}
    </div>
  );
}

/** Appearance settings: theme selection (9 distinct palettes including Studio Dark, Light, Cool Slate, Warm Darkroom, Nordic Frost, Midnight Violet, Forest Sage, OLED Obsidian, and System OS sync),
 *  with category filtering, dynamic accent glowing cards, and live previews. */
function Appearance({ settings, setSettings }: { settings: Settings; setSettings: (s: Settings) => void }) {
  const currentTheme: Theme = (settings.theme as Theme) || getStoredTheme();
  const [activeTheme, setActiveTheme] = useState<Theme>(currentTheme);
  const [previewExposure, setPreviewExposure] = useState<number>(0.35);
  const [categoryFilter, setCategoryFilter] = useState<string>("All");
  const sysDark = isSystemDark();

  const handleSelect = (t: Theme) => {
    setActiveTheme(t);
    applyTheme(t);
    const updated: Settings = { ...settings, theme: t };
    setSettings(updated);
    api.saveSettings(updated);
  };

  const categories = ["All", "Vibrant", "Neutral", "Warm", "Light"];
  const filteredThemes = THEMES.filter((th) => {
    if (categoryFilter === "All") return true;
    if (categoryFilter === "Neutral" && (th.category === "Neutral" || th.id === "system")) return true;
    return th.category === categoryFilter;
  });

  const activeThemeMeta = THEMES.find((t) => t.id === activeTheme) || THEMES[0];

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mx-auto max-w-5xl space-y-6">
        <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <h2 className="text-base font-semibold text-neutral-100 flex items-center gap-2">
              <span>Appearance & Color Themes</span>
              <span className="rounded-full bg-neutral-800 px-2 py-0.5 text-[10px] font-medium text-neutral-400">
                {THEMES.length} Themes
              </span>
            </h2>
            <p className="mt-0.5 text-xs text-neutral-400">
              Modern color palettes crafted for studio color grading, analog warmth, high-contrast daylight, and HDR/OLED displays.
            </p>
          </div>

          {/* Operating system status banner */}
          <div className="flex items-center gap-3 rounded-lg border border-neutral-800 bg-neutral-900/60 px-3 py-1.5 text-xs">
            <span className="flex items-center gap-1.5 text-neutral-300">
              <span>🖥️ OS:</span>
              <strong className="text-neutral-100 font-medium">{sysDark ? "Dark" : "Light"}</strong>
            </span>
            {activeTheme !== "system" && (
              <button
                onClick={() => handleSelect("system")}
                className="text-[11px] font-medium text-sky-400 hover:text-sky-300 transition-colors"
                title="Synchronize appearance with your system appearance settings"
              >
                Sync OS
              </button>
            )}
          </div>
        </div>

        {/* Category Filter Pills */}
        <div className="flex items-center gap-1.5 border-b border-neutral-800/80 pb-3 overflow-x-auto">
          {categories.map((cat) => {
            const count =
              cat === "All"
                ? THEMES.length
                : THEMES.filter((t) => (cat === "Neutral" ? t.category === "Neutral" || t.id === "system" : t.category === cat)).length;
            const isCur = categoryFilter === cat;
            return (
              <button
                key={cat}
                onClick={() => setCategoryFilter(cat)}
                className={`flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-medium transition-all ${isCur
                    ? "bg-neutral-100 text-neutral-900 shadow-sm"
                    : "bg-neutral-900/80 text-neutral-400 hover:bg-neutral-800 hover:text-neutral-200 border border-neutral-800"
                  }`}
              >
                <span>{cat}</span>
                <span className={`text-[10px] px-1 rounded-full ${isCur ? "bg-neutral-300/80 text-neutral-900" : "bg-neutral-800 text-neutral-400"}`}>
                  {count}
                </span>
              </button>
            );
          })}
        </div>

        {/* Theme selection grid */}
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {filteredThemes.map((th) => {
            const isSelected = activeTheme === th.id;
            return (
              <div
                key={th.id}
                onClick={() => handleSelect(th.id)}
                style={
                  isSelected
                    ? {
                      borderColor: th.colors.accent,
                      boxShadow: `0 0 18px ${th.colors.accentGlow}`,
                    }
                    : undefined
                }
                className={`group relative flex cursor-pointer flex-col justify-between rounded-xl border p-4 transition-all duration-200 ${isSelected
                    ? "bg-neutral-900/90 ring-1"
                    : "border-neutral-800/90 bg-neutral-900/40 hover:border-neutral-700 hover:bg-neutral-900/80 hover:shadow-md"
                  }`}
              >
                <div>
                  <div className="flex items-start justify-between gap-2">
                    <div className="flex items-center gap-2.5">
                      <div
                        className="flex h-8 w-8 items-center justify-center rounded-lg border border-neutral-700/60 text-base shadow-xs"
                        style={{ backgroundColor: th.colors.card }}
                      >
                        {th.icon}
                      </div>
                      <div>
                        <div className="text-xs font-semibold text-neutral-100 group-hover:text-white flex items-center gap-1.5">
                          {th.label}
                        </div>
                        <div className="text-[10px] font-medium" style={{ color: th.colors.accent }}>
                          {th.tagline}
                        </div>
                      </div>
                    </div>

                    {isSelected ? (
                      <span
                        className="flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-semibold tracking-wide shadow-xs"
                        style={{
                          backgroundColor: th.colors.accent,
                          color: th.colors.bg === "#000000" || th.colors.bg.startsWith("#0") || th.colors.bg.startsWith("#1") ? "#ffffff" : "#0f172a",
                        }}
                      >
                        ✓ Active
                      </span>
                    ) : (
                      <span className="rounded bg-neutral-800/90 px-1.5 py-0.5 text-[10px] text-neutral-400">
                        {th.category}
                      </span>
                    )}
                  </div>

                  <p className="mt-3 text-[11px] leading-relaxed text-neutral-400">
                    {th.description}
                  </p>
                </div>

                <div className="mt-4 pt-3 border-t border-neutral-800/80">
                  <div className="flex items-center justify-between">
                    <span className="text-[10px] text-neutral-500 font-mono uppercase tracking-wider">Palette</span>
                    <div className="flex items-center gap-1.5">
                      <span
                        className="h-3.5 w-3.5 rounded-full border border-neutral-700/80 shadow-xs"
                        style={{ backgroundColor: th.colors.bg }}
                        title={`Canvas Canvas: ${th.colors.bg}`}
                      />
                      <span
                        className="h-3.5 w-3.5 rounded-full border border-neutral-700/80 shadow-xs"
                        style={{ backgroundColor: th.colors.card }}
                        title={`Panel Surface: ${th.colors.card}`}
                      />
                      <span
                        className="h-3.5 w-3.5 rounded-full border border-neutral-700/80 shadow-xs"
                        style={{ backgroundColor: th.colors.text }}
                        title={`Primary Text: ${th.colors.text}`}
                      />
                      <span
                        className="h-4 w-4 rounded-full border border-white/20 shadow-sm ring-1 ring-white/10"
                        style={{ backgroundColor: th.colors.accent, boxShadow: `0 0 8px ${th.colors.accentGlow}` }}
                        title={`Signature Accent: ${th.colors.accent}`}
                      />
                      {th.colors.secondaryAccent && (
                        <span
                          className="h-3 w-3 rounded-full border border-white/15 shadow-xs"
                          style={{ backgroundColor: th.colors.secondaryAccent }}
                          title={`Secondary Accent: ${th.colors.secondaryAccent}`}
                        />
                      )}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>

        {/* Live Interface Preview */}
        <div className="rounded-xl border border-neutral-800 bg-neutral-900/90 p-5 shadow-lg space-y-4">
          <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between border-b border-neutral-800 pb-3">
            <div>
              <div className="flex items-center gap-2">
                <h3 className="text-xs font-semibold uppercase tracking-wider text-neutral-300">
                  Live Theme Preview
                </h3>
                <span
                  className="rounded px-2 py-0.5 text-[10px] font-semibold"
                  style={{
                    backgroundColor: activeThemeMeta.colors.accent,
                    color: activeThemeMeta.colors.bg.startsWith("#f") ? "#0f172a" : "#ffffff",
                  }}
                >
                  {activeThemeMeta.label}
                </span>
              </div>
              <p className="text-[11px] text-neutral-400 mt-0.5">
                {activeThemeMeta.tagline} — Active studio color tokens rendered in real time.
              </p>
            </div>
            <div className="flex items-center gap-2 text-xs text-neutral-400">
              <span className="flex items-center gap-1.5 rounded bg-neutral-800 px-2 py-1">
                <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
                <span>Engine Ready</span>
              </span>
            </div>
          </div>

          <div className="space-y-4 rounded-lg border border-neutral-800 bg-neutral-950 p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex flex-wrap items-center gap-2">
                <button className="rounded bg-neutral-800 px-3 py-1.5 text-xs font-medium text-neutral-200 border border-neutral-700/60 hover:bg-neutral-700">
                  Default Button
                </button>
                <button
                  className="rounded px-3 py-1.5 text-xs font-medium shadow-md transition-shadow"
                  style={{
                    backgroundColor: "var(--theme-accent, #0ea5e9)",
                    color: "var(--theme-accent-contrast, #ffffff)",
                    boxShadow: "0 0 12px var(--theme-accent-glow, rgba(14,165,233,0.3))",
                  }}
                >
                  Active Accent Button
                </button>
                <button
                  className="rounded border px-3 py-1.5 text-xs font-medium"
                  style={{
                    borderColor: "var(--theme-accent, #0ea5e9)",
                    color: "var(--theme-accent, #0ea5e9)",
                    backgroundColor: "var(--theme-accent-subtle, rgba(14,165,233,0.1))",
                  }}
                >
                  Subtle Accent Pill
                </button>
              </div>

              {/* Color Labels Preview */}
              <div className="flex items-center gap-1.5">
                <span className="text-[11px] text-neutral-500 mr-1">Labels:</span>
                <span className="h-3.5 w-3.5 rounded-full bg-red-500 shadow-xs" title="Red label" />
                <span className="h-3.5 w-3.5 rounded-full bg-amber-400 shadow-xs" title="Yellow label" />
                <span className="h-3.5 w-3.5 rounded-full bg-emerald-500 shadow-xs" title="Green label" />
                <span className="h-3.5 w-3.5 rounded-full bg-sky-500 shadow-xs" title="Blue label" />
                <span className="h-3.5 w-3.5 rounded-full bg-purple-500 shadow-xs" title="Purple label" />
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2">
              <div className="rounded-md border border-neutral-800/80 bg-neutral-900/60 p-3">
                <div className="mb-1 flex justify-between text-xs text-neutral-400">
                  <span>Interactive Exposure Slider</span>
                  <span className="font-mono text-neutral-200 font-medium">
                    {previewExposure > 0 ? `+${previewExposure.toFixed(2)}` : previewExposure.toFixed(2)} EV
                  </span>
                </div>
                <input
                  type="range"
                  className="ol-range w-full"
                  min={-3}
                  max={3}
                  step={0.05}
                  value={previewExposure}
                  onChange={(e) => setPreviewExposure(+e.target.value)}
                />
              </div>

              <div className="flex items-center justify-between rounded-md border border-neutral-800/80 bg-neutral-900/60 p-3 text-xs text-neutral-400">
                <div className="flex items-center gap-2">
                  <span className="text-base">{activeThemeMeta.icon}</span>
                  <div>
                    <div className="font-medium text-neutral-200">{activeThemeMeta.label}</div>
                    <div className="text-[10px] text-neutral-500">Contrast ratio compliant (WCAG AA)</div>
                  </div>
                </div>
                <span
                  className="h-5 w-5 rounded-full border border-white/20 shadow-xs"
                  style={{ backgroundColor: activeThemeMeta.colors.accent }}
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

/** Storage usage of the catalog folder, a library analysis and clean-up. */
function Usage({ onSettingsChanged }: { onSettingsChanged: () => void }) {
  const [r, setR] = useState<StorageReport | null>(null);
  const [busy, setBusy] = useState("");
  const [note, setNote] = useState("");
  const [showDetails, setShowDetails] = useState(false);
  useEffect(() => void api.storage().then(setR, (e) => setNote(`Could not measure storage: ${e}`)), []);
  if (!r) return <p className="p-4 text-[12px] text-neutral-500">{note || "Measuring storage…"}</p>;

  const run = async (key: string, title: string, job: () => Promise<number>, confirm?: string) => {
    if (confirm && !(await ask(confirm, { title, kind: "warning" }))) return;
    setBusy(key);
    setNote("");
    try {
      const n = await job();
      if (key === "cloud") onSettingsChanged(); // the accounts forget their downloaded folders
      const next = await api.storage();
      setNote(`${title}: ${n ? `${num(n)} removed, ` : ""}${size(Math.max(0, r.total - next.total))} freed`);
      setR(next);
    } catch (e) {
      setNote(`${title} failed: ${e}`);
    }
    setBusy("");
  };
  const removeFolder = async (folder: string) => {
    const ids = (await api.query({ folder })).map((p) => p.id);
    const msg = `${folder} is not available. If it is on a drive that isn't connected, connect the drive instead.\n\nRemove its ${ids.length} photo(s) from the catalog? Their ratings, keywords and edit history are lost.`;
    run(folder, "Remove offline folder", () => api.remove(ids).then(() => ids.length), msg);
  };

  const s = r.summary;
  const sep = r.data_dir.includes("\\") ? "\\" : "/";
  const own = r.folders.filter((f) => !f.folder.startsWith(r.data_dir + sep));
  const google = r.folders.filter((f) => f.folder.startsWith(`${r.data_dir}${sep}remote${sep}Google Photos`)).reduce((n, f) => n + f.photos, 0);
  const immichAlbumsBytes = r.immich_albums_bytes ?? 0;
  const cloudDir = `${r.data_dir}${sep}remote${sep}Cloud`;
  const cloudPhotos = r.folders.filter((f) => f.folder === cloudDir || f.folder.startsWith(cloudDir + sep)).reduce((n, f) => n + f.photos, 0);
  const otherBytes = r.other ?? Math.max(0, r.total - (r.catalog + r.thumbs + r.originals + immichAlbumsBytes + r.google + (r.cloud ?? 0)));
  const parts: [string, number, string][] = [
    ["Catalog", r.catalog, "bg-sky-500"],
    ["Thumbnails", r.thumbs, "bg-violet-500"],
    ...(immichAlbumsBytes > 0 ? [["Downloaded Immich albums", immichAlbumsBytes, "bg-cyan-500"] as [string, number, string]] : []),
    ["Immich photo cache", r.originals, "bg-amber-500"],
    ["Google Photos picks", r.google, "bg-emerald-500"],
    ...((r.cloud ?? 0) > 0 ? [["Cloud downloads", r.cloud ?? 0, "bg-rose-500"] as [string, number, string]] : []),
    ["Other", otherBytes, "bg-neutral-500"],
  ];
  const tiles: [string, number, string?][] = [
    ["Photos", s.total_photos - s.virtual_copies],
    ["Virtual copies", s.virtual_copies],
    ["Edited", s.edited],
    ["Edited with AI", r.ai.photos, `${num(r.ai.edits)} AI and MCP edits in all`],
    ["Picks", s.picks],
    ["Rejects", s.rejects],
    ["Rated ★3+", s.rated_3_plus],
    ["History steps", r.history_steps],
  ];
  const c = r.clean;
  // key, title, what it is, amount, button, confirmation for what can't be recovered
  const cleans: [CleanUp, string, string, string, string, string?][] = [
    ["thumbs", "Orphaned thumbnails", "Previews of photos that are no longer in the catalog.", `${plural(c.thumbs.count, "file")} · ${size(c.thumbs.bytes)}`, "Delete"],
    ["originals", "Immich photo cache", "Kept after you open or export an Immich photo; downloaded again when needed while Immich is connected.", `${plural(c.originals.count, "file")} · ${size(c.originals.bytes)}`, "Clear"],
    ...(c.immich_albums && c.immich_albums.bytes > 0
      ? [
        [
          "immich_albums" as CleanUp,
          "Downloaded Immich albums",
          "Full albums downloaded from Immich. Photos remain safe on your Immich server.",
          `${plural(c.immich_albums.count, "photo")} · ${size(c.immich_albums.bytes)}`,
          "Clear",
          `Delete the downloaded Immich albums (${plural(c.immich_albums.count, "photo")}, ${size(c.immich_albums.bytes)})?\n\nTheir photos are also removed from the catalog, with any edits, ratings and keywords you made to them here. The originals stay on your Immich server, where you can browse them in place.`,
        ] as [CleanUp, string, string, string, string, string?],
      ]
      : []),
    ...(c.cloud && c.cloud.bytes > 0
      ? [
        [
          "cloud" as CleanUp,
          "Cloud downloads",
          "Folders downloaded from cloud storage. The originals stay in the cloud.",
          `${plural(c.cloud.count, "file")} · ${size(c.cloud.bytes)}`,
          "Clear",
          `Delete the downloaded cloud folders (${plural(c.cloud.count, "file")}, ${size(c.cloud.bytes)})?\n\nTheir photos are also removed from the catalog, with any edits, ratings and keywords you made to them here. The originals stay in your cloud storage, where you can download them again.`,
        ] as [CleanUp, string, string, string, string, string?],
      ]
      : []),
    [
      "missing",
      "Missing photos",
      "Catalog entries whose file was deleted or moved. Offline folders are left alone.",
      plural(c.missing.count, "photo"),
      "Remove",
      `Remove ${c.missing.count} missing photo(s) and their virtual copies from the catalog? Their ratings, keywords and edit history are lost.`,
    ],
    ["history", "Edit history", "Undo steps. Every photo keeps its current edits.", `${plural(c.history.count, "step")} · ${size(c.history.bytes)}`, "Clear", `Clear ${c.history.count} undo step(s)? Every photo keeps its current edits, but earlier steps can no longer be restored.`],
    ["compact", "Compact catalog", "Rewrites the catalog file without its unused space.", `${size(c.compact.bytes)} unused`, "Compact"],
  ];

  return (
    <div className="flex min-h-0 flex-1 gap-4 overflow-y-auto p-4 text-[12px]">
      <div className="flex w-[520px] shrink-0 flex-col gap-4">
        <section className={card}>
          <div className="flex items-baseline justify-between">
            <h2 className="font-semibold">Storage</h2>
            <span className="tabular-nums text-neutral-300">{size(r.total)}</span>
          </div>
          <p className="mb-3 truncate font-mono text-[11px] text-neutral-500 select-text" title={r.data_dir}>
            {r.data_dir}
          </p>
          <div className="flex h-3 overflow-hidden rounded bg-neutral-800">
            {parts.map(([label, n, color]) => (
              <div key={label} className={color} style={{ width: `${(n / Math.max(1, r.total)) * 100}%` }} title={`${label}: ${size(n)}`} />
            ))}
          </div>
          <div className="mt-3 space-y-1">
            {parts.map(([label, n, color]) => (
              <div key={label} className="flex items-center gap-2">
                <span className={`h-2.5 w-2.5 rounded-sm ${color}`} />
                <span className="flex-1 text-neutral-300">{label}</span>
                <span className="tabular-nums text-neutral-400">{size(n)}</span>
              </div>
            ))}
          </div>

          <div className="mt-3 flex items-center justify-between border-t border-neutral-800/80 pt-2.5">
            <button
              type="button"
              onClick={() => setShowDetails((d) => !d)}
              className="flex items-center gap-1.5 text-[11px] font-medium text-sky-400 hover:text-sky-300"
            >
              <span>{showDetails ? "Hide Storage Analysis & Details" : "Show Storage Analysis & Details"}</span>
              <span className="text-[9px]">{showDetails ? "▲" : "▼"}</span>
            </button>
            <span className="text-[10px] text-neutral-500">
              {r.immich_albums?.length ? `${r.immich_albums.length} Immich album(s) · ` : ""}
              {size(r.catalog)} DB
            </span>
          </div>

          {showDetails && (
            <div className="mt-3 space-y-3 rounded border border-neutral-800 bg-neutral-950 p-3 text-[11px]">
              <div>
                <div className="mb-1 flex items-center justify-between font-semibold text-sky-400">
                  <span>Catalog Database</span>
                  <span className="tabular-nums text-neutral-300">{size(r.catalog)}</span>
                </div>
                <div className="space-y-0.5 pl-2 font-mono text-[10px] text-neutral-400">
                  {r.catalog_details && (
                    <>
                      <div className="flex justify-between">
                        <span>• Database (catalog.olcat):</span>
                        <span>{size(r.catalog_details.db)}</span>
                      </div>
                      <div className="flex justify-between">
                        <span>• Write-Ahead Log (catalog.olcat-wal):</span>
                        <span>{size(r.catalog_details.wal)}</span>
                      </div>
                      {r.catalog_details.free > 0 && (
                        <div className="flex justify-between text-neutral-500">
                          <span>• Unused space in pages:</span>
                          <span>{size(r.catalog_details.free)}</span>
                        </div>
                      )}
                    </>
                  )}
                </div>
              </div>

              {r.immich_albums && r.immich_albums.length > 0 && (
                <div>
                  <div className="mb-1 flex items-center justify-between font-semibold text-cyan-400">
                    <span>Immich Synced Albums ({r.immich_albums.length})</span>
                    <span className="tabular-nums text-neutral-300">{size(immichAlbumsBytes)}</span>
                  </div>
                  <div className="space-y-1 pl-2 font-mono text-[10px] text-neutral-400">
                    {r.immich_albums.map((alb) => (
                      <div key={alb.path} className="flex items-center justify-between gap-2 border-b border-neutral-900 pb-1 last:border-b-0">
                        <div className="truncate">
                          <span className="text-neutral-200">{alb.name}</span>
                          <span className="ml-1.5 text-neutral-500">({plural(alb.photos, "photo")})</span>
                        </div>
                        <span className="shrink-0 tabular-nums text-neutral-300">{size(alb.bytes)}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}

              <div>
                <div className="mb-1 flex items-center justify-between font-semibold text-neutral-300">
                  <span>Analysis of "Other" Storage</span>
                  <span className="tabular-nums text-neutral-300">{size(otherBytes)}</span>
                </div>
                <div className="space-y-0.5 pl-2 font-mono text-[10px] text-neutral-400">
                  {r.other_details && r.other_details.length > 0 ? (
                    r.other_details.map((item) => (
                      <div key={item.name} className="flex justify-between">
                        <span className="truncate">• {item.name}:</span>
                        <span className="shrink-0 tabular-nums text-neutral-400">{size(item.bytes)}</span>
                      </div>
                    ))
                  ) : (
                    <div className="italic text-neutral-500">No miscellaneous files found.</div>
                  )}
                </div>
              </div>
            </div>
          )}

          <p className="mt-3 text-[11px] text-neutral-500">
            Not counted above: your photo files, {size(own.reduce((n, f) => n + f.bytes, 0))} in {own.length} folder(s). OpenLight edits them in place and never copies them.
          </p>
        </section>

        <section className={card}>
          <div className="mb-2 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <h2 className="font-semibold">Clean up</h2>
              {c.all && (c.all.count > 0 || c.all.bytes > 0) && (
                <span className="rounded bg-neutral-800 px-1.5 py-0.5 text-[10px] text-neutral-400">
                  {size(c.all.bytes)} recoverable
                </span>
              )}
            </div>
            <div className="flex items-center gap-2">
              <span className="truncate text-[11px] text-emerald-400" title={note}>
                {note}
              </span>
              <Btn
                active
                className="px-2.5 py-0.5 font-medium text-violet-200 hover:text-white"
                disabled={!!busy || !(c.all?.count || c.all?.bytes)}
                onClick={() =>
                  run(
                    "all",
                    "Clear all caches",
                    () => api.cleanUp("all"),
                    `Clean up all caches, orphaned thumbnails, and edit history, and compact the catalog database?\n\nEstimated space to free: ${size(c.all?.bytes ?? 0)}.\n(Your local and synced photo masters are kept safe).`
                  )
                }
              >
                {busy === "all" ? "Cleaning…" : "Clear All"}
              </Btn>
            </div>
          </div>
          {cleans.map(([key, title, detail, amount, verb, confirm]) => (
            <div key={key} className="flex items-center gap-3 border-t border-neutral-800 py-2 first-of-type:border-t-0">
              <div className="min-w-0 flex-1">
                <div className="text-neutral-200">{title}</div>
                <div className="text-[11px] text-neutral-500">{detail}</div>
              </div>
              <span className="shrink-0 text-right tabular-nums text-neutral-400">{amount}</span>
              <Btn className="w-16 shrink-0" disabled={!!busy || !(c[key]?.count || c[key]?.bytes)} onClick={() => run(key, title, () => api.cleanUp(key), confirm)}>
                {busy === key ? "…" : verb}
              </Btn>
            </div>
          ))}
        </section>
      </div>

      <section className={`${card} flex min-w-0 flex-1 flex-col gap-4`}>
        <h2 className="font-semibold">Library analysis</h2>
        <div className="grid grid-cols-4 gap-2">
          {tiles.map(([label, n, hint]) => (
            <div key={label} className="rounded bg-neutral-950 px-3 py-2" title={hint}>
              <div className="text-lg tabular-nums text-neutral-100">{num(n)}</div>
              <div className="text-[11px] text-neutral-500">{label}</div>
            </div>
          ))}
        </div>
        <p className="text-neutral-400">
          From folders {num(own.reduce((n, f) => n + f.photos, 0))} · Immich {num(r.immich_photos)} · Google Photos {num(google)}
          {cloudPhotos ? ` · Cloud storage ${num(cloudPhotos)}` : ""}
        </p>
        <div className="grid grid-cols-2 gap-4">
          {(
            [
              ["Cameras", s.cameras],
              ["Lenses", s.lenses],
            ] as const
          ).map(([title, list]) => (
            <div key={title}>
              <h3 className={heading}>{title}</h3>
              {list.length === 0 && <p className="text-neutral-600">None recorded</p>}
              {list.slice(0, 5).map((x) => (
                <div key={x.name} className="flex justify-between gap-2">
                  <span className="truncate">{x.name}</span>
                  <span className="tabular-nums text-neutral-500">{num(x.count)}</span>
                </div>
              ))}
            </div>
          ))}
        </div>
        <div className="flex min-h-0 flex-col">
          <h3 className={heading}>Folders by size</h3>
          {r.folders.length === 0 && <p className="text-neutral-600">No folders yet.</p>}
          <div className="grid grid-cols-[minmax(0,1fr)_auto_auto_auto] gap-x-4 gap-y-1 overflow-y-auto">
            {[...r.folders]
              .sort((a, b) => b.bytes - a.bytes)
              .map((f) => (
                <div key={f.folder} className="contents">
                  <span className="truncate" title={f.folder}>
                    {short(f.folder)}
                  </span>
                  <span className="text-right tabular-nums text-neutral-400">{num(f.photos)}</span>
                  <span className="text-right tabular-nums text-neutral-400">{f.online ? size(f.bytes) : "—"}</span>
                  <span className="text-right">
                    {!f.online ? (
                      <button className="text-amber-400 hover:text-amber-300 disabled:opacity-40" disabled={!!busy} title="Folder not found; remove its photos from the catalog…" onClick={() => removeFolder(f.folder)}>
                        offline · remove…
                      </button>
                    ) : f.missing ? (
                      <span className="text-red-400">{num(f.missing)} missing</span>
                    ) : null}
                  </span>
                </div>
              ))}
          </div>
        </div>
      </section>
    </div>
  );
}
