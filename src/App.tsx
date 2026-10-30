import { listen } from "@tauri-apps/api/event";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { Log } from "./Agent";
import { api, AppInfo, AutoFunction, CloudFolder, Culling, Develop as Dev, Filter, FolderLoading, JobProgress, LABELS, NetworkDrive, Photo, setThumbsDir, Settings, Source } from "./api";
import { Develop } from "./Develop";
import { ExportDialog, Library, RemoteDialog, SelectMods } from "./Library";
import { CompareModal } from "./CompareModal";
import { SettingsTab, SettingsView } from "./Settings";
import { applyTheme, getStoredTheme } from "./theme";

type Module = "library" | "develop" | "settings";
const MODULES: [Module, string][] = [
  ["library", "Library"],
  ["develop", "Develop"],
  ["settings", "Settings"],
];

export default function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [module, setModule] = useState<Module>("library");
  const [libraryAutoFn, setLibraryAutoFn] = useState<AutoFunction | null>(null);
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
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("appearance");
  const [loadingFolder, setLoadingFolder] = useState<FolderLoading | null>(null);
  const [binCount, setBinCount] = useState<number>(0);
  const [compareOpen, setCompareOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<{ ids: number[] } | null>(null);

  const reload = useCallback(async () => {
    const [ps, fs, bc] = await Promise.all([
      api.query(filter),
      api.folders(),
      api.binCount().catch(() => 0),
    ]);
    setPhotos(ps);
    setFolders(fs);
    setBinCount(bc);
  }, [filter]);
  const reloadRef = useRef(reload);
  reloadRef.current = reload;
  useEffect(() => void reload(), [reload]);

  useEffect(() => {
    api.appInfo().then((i) => (setThumbsDir(i.thumbs_dir), setInfo(i)));
    api.settings().then((s) => {
      setSettings(s);
      if (s.theme) {
        applyTheme(s.theme);
      } else {
        applyTheme(getStoredTheme());
      }
    });
    api.defaultDevelop().then(setDefaults);
    const subs = [
      listen("catalog-changed", () => reloadRef.current()),
      listen<{ id: number }>("thumb-changed", async (e) => {
        const ph = await api.photo(e.payload.id);
        if (ph) setPhotos((ps) => ps.map((p) => (p.id === ph.id ? ph : p)));
      }),
      listen<{ label?: string; done: number; total: number }>("import-progress", (e) => {
        setStatus(`${e.payload.label ?? "Adding photos"} ${e.payload.done} / ${e.payload.total}`);
        setLoadingFolder((cur) => (cur ? { ...cur, progress: e.payload } : cur));
      }),
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
  useEffect(() => void (module === "settings" && settingsTab === "agent" && api.appInfo().then(setInfo)), [module, settingsTab]);
  useEffect(() => void api.setSelection(selected), [selected]);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = () => {
      const cur = settings?.theme ?? getStoredTheme();
      if (cur === "system") {
        applyTheme("system");
      }
    };
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, [settings?.theme]);

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

  const cull = async (u: Culling, idsOverride?: number[]) => {
    const ids = idsOverride || targets();
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

  const shortFolder = (f: string) => f.split(/[\\/]/).filter(Boolean).slice(-2).join("/") || f;

  // A folder is a live source: opening it picks up files added since it was last read.
  const openFolder = async (folder: string) => {
    const viewId = `folder:${folder}`;
    const name = shortFolder(folder);
    const isSwitching = filter.folder !== folder || filter.immich_album != null || filter.ids != null;
    setLoadingFolder({ viewId, name });
    if (isSwitching) {
      setPhotos([]);
    }
    setFilter((f) => ({ ...f, folder, immich_album: null, ids: null, view: viewId, in_bin: null }));
    setStatus(`Reading ${name}…`);
    try {
      const n = await api.importFolder(folder);
      setStatus(n ? `${n} new photo(s) in ${name}` : "");
      setTimeout(() => setStatus(""), 8000);
    } catch (e) {
      setStatus(`Could not read ${folder}: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
  };

  const openAll = async () => {
    const viewId = "all";
    const isSwitching = filter.folder != null || filter.immich_album != null || filter.ids != null;
    setLoadingFolder({ viewId, name: "All Photographs" });
    if (isSwitching) {
      setPhotos([]);
    }
    setFilter((f) => ({ ...f, folder: null, immich_album: null, ids: null, view: null, in_bin: null }));
    try {
      await reloadRef.current();
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
    }
  };

  const addFolder = async () => {
    const dir = await open({ directory: true, title: "Add a folder of photos" });
    if (typeof dir === "string") openFolder(dir);
  };

  const openImmich = async (album: string | null, name: string) => {
    const viewId = `immich:${album ?? ""}`;
    const isSwitching = filter.view !== viewId;
    setLoadingFolder({ viewId, name });
    if (isSwitching) {
      setPhotos([]);
    }
    setFilter((f) => ({ ...f, folder: null, immich_album: album ?? "favorites", ids: null, view: viewId, in_bin: null }));
    setStatus(`Opening ${name}…`);
    try {
      const { next } = await api.immichOpen(album, 1);
      setAlbumPages({ album, next });
      setStatus("");
    } catch (e) {
      setStatus(`Immich: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
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
    const viewId = "google";
    setLoadingFolder({ viewId, name: "Google Photos" });
    setStatus("Pick photos in the Google Photos tab, then press Done…");
    try {
      const r = await api.googleImport();
      const notes = [r.skipped && `${r.skipped} skipped (format not supported)`, r.failed && `${r.failed} failed (${r.error})`];
      setStatus([`Added ${r.imported} photo(s) from Google Photos`, ...notes.filter(Boolean)].join(", "));
      setFilter((f) => ({ ...f, folder: r.folder, immich_album: null, ids: null, view: "google", in_bin: null }));
    } catch (e) {
      setStatus(`Google Photos: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
    setTimeout(() => setStatus(""), 8000);
  };

  const syncApple = async () => {
    const viewId = "apple";
    setLoadingFolder({ viewId, name: "Apple iCloud Photos" });
    setStatus("Syncing Apple iCloud Photos…");
    try {
      const r = await api.appleImport();
      const notes = [r.skipped && `${r.skipped} up to date`, r.failed && `${r.failed} failed`];
      setStatus([`Synced ${r.imported} photo(s) from Apple iCloud Photos`, ...notes.filter(Boolean)].join(", "));
      setFilter((f) => ({ ...f, folder: r.folder, immich_album: null, ids: null, view: "apple", in_bin: null }));
    } catch (e) {
      setStatus(`Apple iCloud Photos: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
    setTimeout(() => setStatus(""), 8000);
  };

  const syncAmazon = async () => {
    const viewId = "amazon";
    setLoadingFolder({ viewId, name: "Amazon Photos" });
    setStatus("Syncing Amazon Photos…");
    try {
      const r = await api.amazonImport();
      const notes = [r.skipped && `${r.skipped} up to date`, r.failed && `${r.failed} failed`];
      setStatus([`Synced ${r.imported} photo(s) from Amazon Photos`, ...notes.filter(Boolean)].join(", "));
      setFilter((f) => ({ ...f, folder: r.folder, immich_album: null, ids: null, view: "amazon", in_bin: null }));
    } catch (e) {
      setStatus(`Amazon Photos: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
    setTimeout(() => setStatus(""), 8000);
  };

  const syncAdobe = async () => {
    const viewId = "adobe";
    setLoadingFolder({ viewId, name: "Adobe Cloud Photos" });
    setStatus("Syncing Adobe Cloud Photos…");
    try {
      const r = await api.adobeImport();
      const notes = [r.skipped && `${r.skipped} up to date`, r.failed && `${r.failed} failed`];
      setStatus([`Synced ${r.imported} photo(s) from Adobe Cloud Photos`, ...notes.filter(Boolean)].join(", "));
      setFilter((f) => ({ ...f, folder: r.folder, immich_album: null, ids: null, view: "adobe", in_bin: null }));
    } catch (e) {
      setStatus(`Adobe Cloud Photos: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
    setTimeout(() => setStatus(""), 8000);
  };

  // A network drive opens like a folder; it is connected first when it isn't mounted.
  const openDrive = async (d: NetworkDrive) => {
    const viewId = `drive:${d.id}`;
    const isSwitching = filter.view !== viewId;
    setLoadingFolder({ viewId, name: d.name });
    if (isSwitching) {
      setPhotos([]);
    }
    setFilter((f) => ({ ...f, folder: d.path, immich_album: null, ids: null, view: viewId, in_bin: null }));
    setStatus(`Opening ${d.name}…`);
    try {
      const r = await api.networkOpen(d.id);
      setStatus(r.new ? `${r.new} new photo(s) on ${d.name}` : "");
      setTimeout(() => setStatus(""), 8000);
    } catch (e) {
      setStatus(`${d.name}: ${e}`);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
  };

  // A downloaded cloud folder shows at once; what's new in the cloud is downloaded meanwhile.
  const openCloud = async (c: CloudFolder, name: string) => {
    const viewId = `cloud:${c.id}:${c.path}`;
    const folderName = `${name} › ${c.path || "All files"}`;
    const isSwitching = filter.view !== viewId;
    setLoadingFolder({ viewId, name: folderName });
    if (isSwitching) {
      setPhotos([]);
    }
    setFilter((f) => ({ ...f, folder: c.local, immich_album: null, ids: null, view: viewId, in_bin: null }));
    try {
      await syncCloud(c.id, c.path, name);
    } finally {
      setLoadingFolder((cur) => (cur?.viewId === viewId ? null : cur));
      await reloadRef.current();
    }
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
  const handleSaveAsNewPhoto = async (newPhoto: Photo) => {
    await reload();
    focus(newPhoto.id);
    const label = newPhoto.copy_name ? `${newPhoto.filename} (${newPhoto.copy_name})` : newPhoto.filename;
    setStatus(`Saved edit as new photo: ${label}`);
    setTimeout(() => setStatus(""), 4000);
  };
  const openBin = () => {
    setSelected([]);
    setActiveId(null);
    setFilter({ folder: null, immich_album: null, ids: null, view: "bin", in_bin: true });
  };

  const moveToBin = async (idsOverride?: number[]) => {
    const ids = idsOverride || targets();
    if (!ids.length) return;
    try {
      const n = await api.moveToBin(ids);
      setStatus(`Moved ${n} photo(s) to Bin`);
      setSelected((prev) => prev.filter((id) => !ids.includes(id)));
      if (activeId && ids.includes(activeId)) setActiveId(null);
      await reload();
      setTimeout(() => setStatus(""), 4000);
    } catch (e) {
      setStatus(`Move to Bin failed: ${e}`);
    }
  };

  const restoreFromBin = async () => {
    const ids = targets();
    if (!ids.length) return;
    try {
      const n = await api.restoreFromBin(ids);
      setStatus(`Restored ${n} photo(s) from Bin`);
      setSelected([]);
      setActiveId(null);
      await reload();
      setTimeout(() => setStatus(""), 4000);
    } catch (e) {
      setStatus(`Restore failed: ${e}`);
    }
  };

  const permanentDelete = () => {
    const ids = targets();
    if (!ids.length) return;
    setConfirmDelete({ ids });
  };

  const executePermanentDelete = async () => {
    if (!confirmDelete) return;
    const ids = confirmDelete.ids;
    setConfirmDelete(null);
    try {
      const n = await api.permanentDelete(ids);
      setStatus(`Permanently deleted ${n} photo(s)`);
      setSelected([]);
      setActiveId(null);
      await reload();
      setTimeout(() => setStatus(""), 4000);
    } catch (e) {
      setStatus(`Permanent delete failed: ${e}`);
    }
  };

  const emptyBin = async () => {
    const count = binCount || photos.length;
    if (count === 0) return;
    const ok = await ask(
      `Empty Bin (${count} photo(s))?\n\nAll photos in the Bin will be permanently deleted from your disk. This action cannot be undone.`,
      { title: "Empty Bin", kind: "error" }
    );
    if (!ok) return;
    try {
      const n = await api.emptyBin();
      setStatus(`Emptied Bin (${n} photo(s) permanently deleted)`);
      setSelected([]);
      setActiveId(null);
      await reload();
      setTimeout(() => setStatus(""), 4000);
    } catch (e) {
      setStatus(`Empty Bin failed: ${e}`);
    }
  };

  const remove = async () => {
    const ids = targets();
    if (!ids.length || !(await ask(`Remove ${ids.length} photo(s) from the catalog? Files on disk are not touched.`, { title: "Remove from catalog", kind: "warning" }))) return;
    await api.remove(ids);
    setSelected([]);
    setActiveId(null);
  };

  const keys = useRef<(e: KeyboardEvent) => void>(() => { });
  keys.current = (e) => {
    if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey || module === "settings") return;
    const k = e.key.toLowerCase();
    if (k === "c" && selected.length >= 2) {
      e.preventDefault();
      setCompareOpen((prev) => !prev);
    } else if (k === "g") {
      setModule("library");
      setLibraryAutoFn(null);
    } else if (k === "d" && active) setModule("develop");
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
    } else if (e.key === "Backspace" || e.key === "Delete") {
      e.preventDefault();
      if (filter.view === "bin" || e.shiftKey) {
        permanentDelete();
      } else {
        moveToBin();
      }
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
        <span className="font-semibold tracking-wide text-neutral-100 flex items-center gap-2">
          <img
            src="/icon.png"
            alt="OpenLight Studio"
            className="h-5 w-5 rounded object-cover shadow-xs"
          />
          <span>
            OpenLight <span className="font-normal text-neutral-400">Studio</span>
          </span>
        </span>
        <nav className="flex gap-1">
          {MODULES.map(([m, label]) => {
            const isCur = module === m;
            return (
              <button
                key={m}
                onClick={() => {
                  setModule(m);
                  if (m === "library" && module === "library") {
                    setLibraryAutoFn(null);
                  }
                }}
                disabled={m === "develop" && !active}
                style={
                  isCur
                    ? {
                      backgroundColor: "var(--theme-accent, #0ea5e9)",
                      color: "var(--theme-accent-contrast, #ffffff)",
                      boxShadow: "0 0 10px var(--theme-accent-glow, rgba(14,165,233,0.3))",
                    }
                    : undefined
                }
                className={`rounded-md px-3 py-1 text-[12px] transition-all duration-150 disabled:opacity-40 ${isCur ? "font-semibold shadow-xs" : "text-neutral-400 hover:text-neutral-200 hover:bg-neutral-900"
                  }`}
              >
                {label}
              </button>
            );
          })}
        </nav>
        <span className="ml-auto truncate text-[11px] text-neutral-400" title={status}>
          {status || background}
        </span>
        {!status && culling && (
          <button className="text-[11px] text-neutral-500 hover:text-white" title="Stop culling the new photos; what is done stays" onClick={() => api.cancelJob(culling.job)}>
            ✕
          </button>
        )}
        <button
          className="flex items-center gap-1.5 text-[11px] text-neutral-400 hover:text-neutral-200 transition-colors cursor-pointer"
          title={`${info?.mcp_url ?? "MCP server"} — Click to open AI & MCP Settings`}
          onClick={() => {
            setSettingsTab("agent");
            setModule("settings");
          }}
        >
          <span
            className={`h-2 w-2 rounded-full transition-all ${info?.mcp_status === "running" ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.6)]" : "bg-neutral-600"
              }`}
          />
          <span className="font-mono text-[10px]">MCP</span>
        </button>
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
          setSettings={setSettings}
          dataDir={info?.data_dir ?? ""}
          loadingFolder={loadingFolder}
          onShowAll={openAll}
          onAddFolder={addFolder}
          onOpenFolder={openFolder}
          onImmich={openImmich}
          nextPage={nextPage}
          onLoadMore={loadMore}
          onPickGoogle={pickGoogle}
          onSyncApple={syncApple}
          onSyncAmazon={syncAmazon}
          onSyncAdobe={syncAdobe}
          onOpenDrive={openDrive}
          onOpenCloud={openCloud}
          onAddCloudFolder={syncCloud}
          onStorage={() => (setSettingsTab("locations"), setModule("settings"))}
          onConnect={setRemote}
          onExport={() => setExportIds(targets())}
          onVirtualCopy={virtualCopy}
          onRemove={remove}
          binCount={binCount}
          onOpenBin={openBin}
          onMoveToBin={moveToBin}
          onRestoreFromBin={restoreFromBin}
          onPermanentDelete={permanentDelete}
          onEmptyBin={emptyBin}
          onOpenCompare={() => setCompareOpen(true)}
          autoFn={libraryAutoFn}
          onSelectAutoFn={setLibraryAutoFn}
          jobs={jobs}
          onAiSettings={() => {
            setSettingsTab("agent");
            setModule("settings");
          }}
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
          onSaveAsNewPhoto={handleSaveAsNewPhoto}
          logs={logs}
          clearLogs={() => setLogs([])}
        />
      )}
      {module === "settings" && (
        <SettingsView
          tab={settingsTab}
          setTab={setSettingsTab}
          settings={settings}
          setSettings={setSettings}
          selected={selected}
          jobs={jobs}
          onConnect={setRemote}
          onAuto={() => {
            setModule("library");
            setLibraryAutoFn("cull");
          }}
          info={info}
          logs={logs}
          clearLogs={() => setLogs([])}
        />
      )}
      {exportIds && exportIds.length > 0 && <ExportDialog ids={exportIds} settings={settings} dest={exportDest(exportIds)} onClose={() => setExportIds(null)} />}
      {remote && settings && <RemoteDialog source={remote} settings={settings} setSettings={setSettings} onClose={() => setRemote(null)} />}
      {compareOpen && selected.length >= 2 && (
        <CompareModal
          groups={[
            {
              id: "selection",
              title: `Selected Photos (${selected.length})`,
              kind: "manual",
              photos: selected.map((id) => photos.find((p) => p.id === id)).filter(Boolean) as Photo[],
              keepId: selected[0],
            },
          ]}
          initialIndex={0}
          onClose={() => setCompareOpen(false)}
          onCull={(id, culling) => cull(culling, [id])}
          onMoveToBin={(id) => moveToBin([id])}
        />
      )}
      {confirmDelete && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-xs p-4"
          onClick={() => setConfirmDelete(null)}
        >
          <div
            className="w-full max-w-md rounded-xl border border-red-500/40 bg-neutral-900 p-6 text-neutral-100 shadow-2xl space-y-4"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-start gap-3.5">
              <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-red-500/20 text-red-400 border border-red-500/30 text-xl font-bold">
                ⚠️
              </div>
              <div className="space-y-1.5 flex-1">
                <h3 className="text-base font-semibold text-white">
                  Permanently Delete {confirmDelete.ids.length} Photo{confirmDelete.ids.length === 1 ? "" : "s"}?
                </h3>
                <p className="text-xs text-neutral-300 leading-relaxed">
                  This will permanently delete the original file{confirmDelete.ids.length === 1 ? "" : "s"} from your disk. All associated edit history, sidecar metadata, and cached thumbnails will also be removed.
                </p>
                <div className="rounded-md bg-red-950/60 border border-red-800/60 px-3 py-2 text-xs font-semibold text-red-300">
                  This action cannot be undone.
                </div>
              </div>
            </div>

            <div className="flex items-center justify-end gap-2.5 pt-3 border-t border-neutral-800">
              <button
                type="button"
                autoFocus
                onClick={() => setConfirmDelete(null)}
                className="rounded-lg border border-neutral-700 bg-neutral-800 px-4 py-2 text-xs font-medium text-neutral-200 hover:bg-neutral-700 hover:text-white transition-colors cursor-pointer"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={executePermanentDelete}
                className="rounded-lg bg-red-600 hover:bg-red-500 px-4 py-2 text-xs font-semibold text-white shadow-md transition-colors cursor-pointer flex items-center gap-1.5 border border-red-500"
              >
                <span>✕ Delete Permanently</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
