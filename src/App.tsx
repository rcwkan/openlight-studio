import { listen } from "@tauri-apps/api/event";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { Agent, Log } from "./Agent";
import { api, AppInfo, Culling, Develop as Dev, Filter, LABELS, Photo, setThumbsDir, Settings, Source } from "./api";
import { Develop } from "./Develop";
import { ExportDialog, Library, RemoteDialog, SelectMods } from "./Library";

type Module = "library" | "develop" | "agent";
const MODULES: [Module, string][] = [
  ["library", "Library"],
  ["develop", "Develop"],
  ["agent", "AI & MCP"],
];

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [module, setModule] = useState<Module>("library");
  const [filter, setFilter] = useState<Filter>({});
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [folders, setFolders] = useState<[string, number][]>([]);
  const [selected, setSelected] = useState<number[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [defaults, setDefaults] = useState<Dev | null>(null);
  const [logs, setLogs] = useState<Log[]>([]);
  const [status, setStatus] = useState("");
  const [exportIds, setExportIds] = useState<number[] | null>(null);
  const [remote, setRemote] = useState<Source | null>(null);

  const reload = useCallback(async () => {
    const [ps, fs] = await Promise.all([api.query(filter), api.folders()]);
    setPhotos(ps);
    setFolders(fs);
  }, [filter]);
  const reloadRef = useRef(reload);
  reloadRef.current = reload;
  useEffect(() => void reload(), [reload]);

  useEffect(() => {
    api.appInfo().then((i) => (setThumbsDir(i.thumbs_dir), setInfo(i)));
    api.settings().then(setSettings);
    api.defaultDevelop().then(setDefaults);
    const subs = [
      listen("catalog-changed", () => reloadRef.current()),
      listen<{ id: number }>("thumb-changed", async (e) => {
        const ph = await api.photo(e.payload.id);
        if (ph) setPhotos((ps) => ps.map((p) => (p.id === ph.id ? ph : p)));
      }),
      listen<{ label?: string; done: number; total: number }>("import-progress", (e) => setStatus(`${e.payload.label ?? "Importing"} ${e.payload.done} / ${e.payload.total}`)),
      listen<{ source: string; message: string }>("agent-log", (e) => setLogs((l) => [...l.slice(-499), { t: new Date().toLocaleTimeString(), ...e.payload }])),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  }, []);
  useEffect(() => void (module === "agent" && api.appInfo().then(setInfo)), [module]);
  useEffect(() => void api.setSelection(selected), [selected]);

  const active = photos.find((p) => p.id === activeId) ?? null;
  const targets = () => (selected.length ? selected : activeId != null ? [activeId] : []);

  const select = (id: number, m: SelectMods) => {
    setSelected((sel) => {
      if (m.toggle) return sel.includes(id) ? sel.filter((s) => s !== id) : [...sel, id];
      if (m.shift && activeId != null) {
        const [a, b] = [photos.findIndex((p) => p.id === activeId), photos.findIndex((p) => p.id === id)];
        return photos.slice(Math.min(a, b), Math.max(a, b) + 1).map((p) => p.id);
      }
      return [id];
    });
    if (!m.shift) setActiveId(id);
  };
  const focus = (id: number) => (setActiveId(id), setSelected([id]));

  const cull = async (u: Culling) => {
    const ids = targets();
    if (!ids.length) return;
    try {
      await api.update(ids, u);
    } catch (e) {
      return setStatus(String(e));
    }
    setPhotos((ps) =>
      ps.map((p) =>
        ids.includes(p.id)
          ? {
              ...p,
              ...(u.rating != null && { rating: u.rating }),
              ...(u.flag && { flag: u.flag === "pick" ? 1 : u.flag === "reject" ? -1 : 0 }),
              ...(u.color_label && { color_label: u.color_label === "none" ? "" : u.color_label }),
              ...(u.keywords != null && { keywords: u.keywords }),
            }
          : p,
      ),
    );
  };

  const doImport = async () => {
    const dir = await open({ directory: true, title: "Import a folder of photos" });
    if (typeof dir !== "string") return;
    setStatus("Scanning…");
    try {
      const n = await api.importFolder(dir);
      setStatus(`Imported ${n} new photo(s)`);
      setFilter((f) => ({ ...f, folder: dir }));
    } catch (e) {
      setStatus(`Import failed: ${e}`);
    }
    setTimeout(() => setStatus(""), 5000);
  };
  const importRemote = async (source: Source, album: string | null) => {
    setStatus(source === "immich" ? "Searching Immich…" : "Pick photos in the Google Photos tab, then press Done…");
    try {
      const r = source === "immich" ? await api.immichImport(album) : await api.googleImport();
      const notes = [r.skipped && `${r.skipped} skipped (format not supported)`, r.failed && `${r.failed} failed (${r.error})`];
      setStatus([`Imported ${r.imported} new photo(s)`, ...notes.filter(Boolean)].join(", "));
      setFilter((f) => ({ ...f, folder: r.folder }));
    } catch (e) {
      setStatus(`Import failed: ${e}`);
    }
    setTimeout(() => setStatus(""), 8000);
  };
  const virtualCopy = async () => {
    if (activeId == null) return;
    const n = await api.virtualCopy(activeId);
    await reload();
    focus(n);
  };
  const remove = async () => {
    const ids = targets();
    if (!ids.length || !(await ask(`Remove ${ids.length} photo(s) from the catalog? Files on disk are not touched.`, { title: "Remove from catalog", kind: "warning" }))) return;
    await api.remove(ids);
    setSelected([]);
    setActiveId(null);
  };

  const keys = useRef<(e: KeyboardEvent) => void>(() => {});
  keys.current = (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey || module === "agent") return;
    const k = e.key.toLowerCase();
    if (k === "g") setModule("library");
    else if (k === "d" && active) setModule("develop");
    else if (/^[0-5]$/.test(k)) cull({ rating: +k });
    else if (k === "p") cull({ flag: "pick" });
    else if (k === "x") cull({ flag: "reject" });
    else if (k === "u") cull({ flag: "unflagged" });
    else if (/^[6-9]$/.test(k)) {
      const label = LABELS[+k - 6];
      cull({ color_label: active?.color_label === label ? "none" : label });
    } else if (k === "arrowright" || k === "arrowleft") {
      e.preventDefault();
      const i = photos.findIndex((p) => p.id === activeId) + (k === "arrowright" ? 1 : -1);
      if (photos[i]) focus(photos[i].id);
    }
  };
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement;
      if (!["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName)) keys.current(e);
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-10 shrink-0 items-center gap-5 border-b border-neutral-800 bg-neutral-950 px-4">
        <span className="font-semibold tracking-wide text-neutral-100">
          <span className="text-amber-300">◐</span> OpenLight Studio
        </span>
        <nav className="flex gap-1">
          {MODULES.map(([m, label]) => (
            <button
              key={m}
              onClick={() => setModule(m)}
              disabled={m === "develop" && !active}
              className={`rounded px-3 py-1 text-[12px] disabled:opacity-40 ${module === m ? "bg-neutral-800 text-white" : "text-neutral-400 hover:text-neutral-200"}`}
            >
              {label}
            </button>
          ))}
        </nav>
        <span className="ml-auto truncate text-[11px] text-neutral-400" title={status}>
          {status}
        </span>
        <span className="flex items-center gap-1.5 text-[11px] text-neutral-500" title={info?.mcp_url}>
          <span className={`h-2 w-2 rounded-full ${info?.mcp_status === "running" ? "bg-emerald-500" : "bg-neutral-600"}`} />
          MCP
        </span>
      </header>

      {module === "library" && (
        <Library
          photos={photos}
          folders={folders}
          filter={filter}
          setFilter={setFilter}
          selected={selected}
          active={active}
          select={select}
          openDevelop={(id) => (focus(id), setModule("develop"))}
          cull={cull}
          onImport={doImport}
          onRemote={setRemote}
          onExport={() => setExportIds(targets())}
          onVirtualCopy={virtualCopy}
          onRemove={remove}
        />
      )}
      {module === "develop" && defaults && (
        <Develop
          photos={photos}
          active={active}
          selected={selected}
          setActive={focus}
          defaults={defaults}
          settings={settings}
          setSettings={setSettings}
          onExport={() => setExportIds(targets())}
        />
      )}
      {module === "agent" && <Agent info={info} settings={settings} setSettings={setSettings} logs={logs} clearLogs={() => setLogs([])} />}
      {exportIds && exportIds.length > 0 && <ExportDialog ids={exportIds} settings={settings} onClose={() => setExportIds(null)} />}
      {remote && settings && (
        <RemoteDialog source={remote} settings={settings} setSettings={setSettings} onImport={(album) => importRemote(remote, album)} onClose={() => setRemote(null)} />
      )}
    </div>
  );
}
