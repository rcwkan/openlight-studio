import { listen } from "@tauri-apps/api/event";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { Agent, Log } from "./Agent";
import { api, AppInfo, CloudFolder, Culling, Develop as Dev, Filter, JobProgress, LABELS, NetworkDrive, Photo, setThumbsDir, Settings, Source } from "./api";
import { AutoView } from "./AutoView";
import { Develop } from "./Develop";
import { ExportDialog, Library, RemoteDialog, SelectMods } from "./Library";
import { SettingsTab, SettingsView } from "./Settings";

type Module = "library" | "develop" | "auto" | "agent" | "settings";
const MODULES: [Module, string][] = [
  ["library", "Library"],
  ["develop", "Develop"],
  ["auto", "Auto"],
  ["agent", "AI & MCP"],
  ["settings", "Settings"],
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
  const [albumPages, setAlbumPages] = useState<{ album: string | null; next: number | null } | null>(null);
  const loadingPage = useRef(false);
  const [jobs, setJobs] = useState<Record<string, JobProgress>>({});
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("locations");

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
      listen<{ label?: string; done: number; total: number }>("import-progress", (e) => setStatus(`${e.payload.label ?? "Adding photos"} ${e.payload.done} / ${e.payload.total}`)),
      listen<string>("remote-error", (e) => setStatus(e.payload)),
      listen<JobProgress>("job-progress", (e) =>
        setJobs((js) => {
          const { [e.payload.job]: _, ...rest } = js;
          return e.payload.finished ? rest : { ...rest, [e.payload.job]: e.payload };
        }),
      ),
      listen<{ source: string; message: string }>("agent-log", (e) => setLogs((l) => [...l.slice(-499), { t: new Date().toLocaleTimeString(), ...e.payload }])),
    ];
    return () => subs.forEach((s) => s.then((f) => f()));
  }, []);
  useEffect(() => void (module === "agent" && api.appInfo().then(setInfo)), [module]);
  useEffect(() => void api.setSelection(selected), [selected]);

  const active = photos.find((p) => p.id === activeId) ?? null;
  // Auto Catalog & Culling of new photos runs in the background
  const culling = Object.values(jobs).find((j) => j.job.startsWith("on-import-"));
  const background = culling ? `Culling new photos${culling.total ? ` ${culling.done} / ${culling.total}` : "…"}` : "";
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

  // A folder is a live source: opening it picks up files added since it was last read.
  const openFolder = (folder: string) => {
    setFilter((f) => ({ ...f, folder, immich_album: null, ids: null, view: null }));
    api.importFolder(folder).then(
      (n) => setStatus(n ? `${n} new photo(s) in ${folder}` : ""),
      (e) => setStatus(`Could not read ${folder}: ${e}`),
    );
    setTimeout(() => setStatus(""), 8000);
  };
  const addFolder = async () => {
    const dir = await open({ directory: true, title: "Add a folder of photos" });
    if (typeof dir === "string") openFolder(dir);
  };
  const openImmich = async (album: string | null, name: string) => {
    setStatus(`Opening ${name}…`);
    try {
      const { next } = await api.immichOpen(album, 1);
      setAlbumPages({ album, next });
      setFilter((f) => ({ ...f, folder: null, immich_album: album ?? "favorites", ids: null, view: `immich:${album ?? ""}` }));
      setStatus("");
    } catch (e) {
      setStatus(`Immich: ${e}`);
    }
  };
  // Albums arrive a page at a time; the grid asks for the next page as its end comes near.
  const nextPage = albumPages?.next && filter.view === `immich:${albumPages.album ?? ""}` ? albumPages.next : null;
  const loadMore = async () => {
    const cur = albumPages;
    if (!nextPage || !cur || loadingPage.current) return;
    loadingPage.current = true;
    try {
      const { next } = await api.immichOpen(cur.album, nextPage);
      setAlbumPages((s) => (s === cur ? { ...cur, next } : s)); // ignore if the album was reopened meanwhile
    } catch (e) {
      setStatus(`Immich: ${e}`);
      setAlbumPages((s) => (s === cur ? { ...cur, next: null } : s)); // stop rather than retry on every scroll
    }
    loadingPage.current = false;
  };
  const pickGoogle = async () => {
    setStatus("Pick photos in the Google Photos tab, then press Done…");
    try {
      const r = await api.googleImport();
      const notes = [r.skipped && `${r.skipped} skipped (format not supported)`, r.failed && `${r.failed} failed (${r.error})`];
      setStatus([`Added ${r.imported} photo(s) from Google Photos`, ...notes.filter(Boolean)].join(", "));
      setFilter((f) => ({ ...f, folder: r.folder, immich_album: null, ids: null, view: null }));
    } catch (e) {
      setStatus(`Google Photos: ${e}`);
    }
    setTimeout(() => setStatus(""), 8000);
  };
  // A network drive opens like a folder; it is connected first when it isn't mounted.
  const openDrive = async (d: NetworkDrive) => {
    setFilter((f) => ({ ...f, folder: d.path, immich_album: null, ids: null, view: `drive:${d.id}` }));
    setStatus(`Opening ${d.name}…`);
    try {
      const r = await api.networkOpen(d.id);
      setStatus(r.new ? `${r.new} new photo(s) on ${d.name}` : "");
    } catch (e) {
      setStatus(`${d.name}: ${e}`);
    }
    setTimeout(() => setStatus(""), 8000);
  };
  // A downloaded cloud folder shows at once; what's new in the cloud is downloaded meanwhile.
  const openCloud = async (c: CloudFolder, name: string) => {
    setFilter((f) => ({ ...f, folder: c.local, immich_album: null, ids: null, view: `cloud:${c.id}:${c.path}` }));
    await syncCloud(c.id, c.path, name);
  };
  const syncing = useRef(new Set<string>());
  const syncCloud = async (id: string, path: string, name: string) => {
    const key = `${id}:${path}`;
    if (syncing.current.has(key)) return; // one download of a folder at a time
    syncing.current.add(key);
    setStatus(`Checking ${name}…`);
    try {
      const r = await api.cloudSync(id, path);
      setStatus([`${r.imported} new photo(s) from ${name}`, r.failed && `${r.failed} failed (${r.error})`].filter(Boolean).join(", "));
      api.settings().then(setSettings); // the folder joins the account's list
    } catch (e) {
      setStatus(`${name}: ${e}`);
    }
    syncing.current.delete(key);
    setTimeout(() => setStatus(""), 8000);
  };
  // Saving an Immich photo uploads the edit back to Immich by default.
  const exportDest = (ids: number[]) => {
    const ps = photos.filter((p) => ids.includes(p.id));
    return ps.length > 0 && ps.every((p) => p.path.startsWith("immich://")) ? "immich" : "";
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
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || module === "auto" || module === "agent" || module === "settings") return;
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
          {status || background}
        </span>
        {!status && culling && (
          <button className="text-[11px] text-neutral-500 hover:text-white" title="Stop culling the new photos; what is done stays" onClick={() => api.cancelJob(culling.job)}>
            ✕
          </button>
        )}
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
          settings={settings}
          dataDir={info?.data_dir ?? ""}
          onAddFolder={addFolder}
          onOpenFolder={openFolder}
          onImmich={openImmich}
          nextPage={nextPage}
          onLoadMore={loadMore}
          onPickGoogle={pickGoogle}
          onOpenDrive={openDrive}
          onOpenCloud={openCloud}
          onAddCloudFolder={syncCloud}
          onStorage={() => (setSettingsTab("locations"), setModule("settings"))}
          onConnect={setRemote}
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
          logs={logs}
          clearLogs={() => setLogs([])}
        />
      )}
      {module === "auto" && (
        <AutoView
          settings={settings}
          setSettings={setSettings}
          selected={selected}
          view={photos}
          jobs={jobs}
          onShow={(ids, label) => (setFilter({ ids, view: `auto:${label}` }), setModule("library"))}
          onAiSettings={() => setModule("agent")}
          onStorage={() => (setSettingsTab("locations"), setModule("settings"))}
        />
      )}
      {module === "agent" && <Agent info={info} settings={settings} setSettings={setSettings} logs={logs} clearLogs={() => setLogs([])} />}
      {module === "settings" && (
        <SettingsView tab={settingsTab} setTab={setSettingsTab} settings={settings} setSettings={setSettings} selected={selected} jobs={jobs} onConnect={setRemote} onAuto={() => setModule("auto")} />
      )}
      {exportIds && exportIds.length > 0 && <ExportDialog ids={exportIds} settings={settings} dest={exportDest(exportIds)} onClose={() => setExportIds(null)} />}
      {remote && settings && <RemoteDialog source={remote} settings={settings} setSettings={setSettings} onClose={() => setRemote(null)} />}
    </div>
  );
}
