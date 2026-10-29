import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { ReactNode, useEffect, useRef, useState } from "react";
import { api, CloudAccount, CloudFolder, Culling, Filter, flagName, ImmichAlbum, LABEL_COLORS, LABELS, NetworkDrive, Photo, Settings, Source, thumbUrl } from "./api";
import { Btn, Panel, Stars } from "./controls";
import { CloudBrowser } from "./Storage";

export type SelectMods = { shift: boolean; toggle: boolean };

type Props = {
  photos: Photo[];
  folders: [string, number][];
  filter: Filter;
  setFilter: (f: Filter) => void;
  selected: number[];
  active: Photo | null;
  select: (id: number, mods: SelectMods) => void;
  openDevelop: (id: number) => void;
  cull: (u: Culling) => void;
  settings: Settings | null;
  dataDir: string;
  onAddFolder: () => void;
  onOpenFolder: (folder: string) => void;
  onImmich: (album: string | null, name: string) => void;
  /** Next page of the Immich album being shown, null when it is fully loaded */
  nextPage: number | null;
  onLoadMore: () => void;
  onPickGoogle: () => void;
  /** Connects the drive when needed; settles once its new photos are read */
  onOpenDrive: (drive: NetworkDrive) => Promise<void>;
  onOpenCloud: (folder: CloudFolder, account: string) => void;
  /** Download a folder of a cloud account into the catalog */
  onAddCloudFolder: (account: string, path: string, name: string) => Promise<void>;
  /** Settings › Storage, where drives and cloud storage are added */
  onStorage: () => void;
  onConnect: (source: Source) => void;
  onExport: () => void;
  onVirtualCopy: () => void;
  onRemove: () => void;
};

const short = (folder: string) => folder.split(/[\\/]/).slice(-2).join("/");

export function Library(p: Props) {
  const [size, setSize] = useState(170);
  const [albums, setAlbums] = useState<ImmichAlbum[]>([]);
  const [online, setOnline] = useState<Record<string, boolean>>({});
  const [cloudDirs, setCloudDirs] = useState<CloudFolder[]>([]);
  const [browse, setBrowse] = useState<CloudAccount | null>(null);
  const f = p.filter;
  const immichUrl = p.settings?.immich_url;
  const drives = p.settings?.network_drives ?? [];
  const clouds = p.settings?.clouds ?? [];
  useEffect(() => {
    if (immichUrl) api.immichAlbums().then(setAlbums, () => setAlbums([]));
  }, [immichUrl]);
  const driveKey = drives.map((d) => d.id).join();
  const checkDrives = () => api.networkStatus().then((l) => setOnline(Object.fromEntries(l.map((d) => [d.id, d.online]))));
  useEffect(() => void (driveKey && checkDrives()), [driveKey]);
  const cloudKey = JSON.stringify(clouds.map((a) => [a.id, a.folders]));
  useEffect(() => void api.cloudLocalDirs().then(setCloudDirs), [cloudKey]);
  // Google Photos picks are downloaded into the catalog folder; list them under Google Photos, not Folders.
  const sep = p.dataDir.includes("\\") ? "\\" : "/";
  const remoteDir = `${p.dataDir}${sep}remote`;
  const googleDir = `${remoteDir}${sep}Google Photos`;
  const under = (folder: string, root: string) => folder === root || folder.startsWith(root.replace(/[\\/]+$/, "") + sep);
  const count = (root: string) => p.folders.filter(([folder]) => under(folder, root)).reduce((n, [, k]) => n + k, 0);
  // network drives are listed under their own heading
  const local = p.folders.filter(([folder]) => !under(folder, remoteDir) && !drives.some((d) => under(folder, d.path)));
  const show = (patch: Filter) => p.setFilter({ ...f, folder: null, immich_album: null, ids: null, view: null, ...patch });
  const gridRef = useRef<HTMLDivElement>(null);
  const endRef = useRef<HTMLDivElement>(null);
  const loadMore = useRef(p.onLoadMore);
  loadMore.current = p.onLoadMore;
  // Large Immich albums load a page at a time: ask for the next one when the end of the grid
  // comes near. Observing again after each page keeps loading until the grid overflows.
  useEffect(() => {
    if (!p.nextPage || !endRef.current) return;
    const io = new IntersectionObserver(([e]) => e.isIntersecting && loadMore.current(), { root: gridRef.current, rootMargin: "600px" });
    io.observe(endRef.current);
    return () => io.disconnect();
  }, [p.nextPage, p.photos.length]);
  const side = "px-1 text-neutral-500 hover:text-white";
  const manage = (source: Source) => (
    <button title="Connection" className={side} onClick={() => p.onConnect(source)}>
      ⋯
    </button>
  );
  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-56 shrink-0 flex-col overflow-y-auto border-r border-neutral-800 bg-neutral-900">
        <Panel title="Library">
          <FolderRow label="All Photographs" active={!f.folder && !f.immich_album && !f.ids} onClick={() => show({})} />
        </Panel>
        <Panel
          title="Folders"
          right={
            <button title="Add a folder" className={side} onClick={p.onAddFolder}>
              ＋
            </button>
          }
        >
          {local.length === 0 && <p className="text-[11px] text-neutral-500">Add a folder to browse and edit its photos in place.</p>}
          {local.map(([folder, n]) => (
            <FolderRow key={folder} label={short(folder)} title={folder} count={n} active={f.folder === folder} onClick={() => p.onOpenFolder(folder)} />
          ))}
        </Panel>
        <Panel
          title="Network drives"
          right={
            <button title="Add a network drive" className={side} onClick={p.onStorage}>
              ＋
            </button>
          }
        >
          {drives.length === 0 && <p className="text-[11px] text-neutral-500">Add a share on your NAS or another computer to browse it like a folder.</p>}
          {drives.map((d) => (
            <FolderRow
              key={d.id}
              label={d.name}
              title={`${d.url ? `${d.url} → ` : ""}${d.path}${online[d.id] ? "" : " (not connected: opening it connects it)"}`}
              icon={<span className={`h-1.5 w-1.5 shrink-0 rounded-full ${online[d.id] ? "bg-emerald-500" : "bg-neutral-600"}`} />}
              count={count(d.path) || undefined}
              active={f.view === `drive:${d.id}`}
              onClick={() => p.onOpenDrive(d).then(checkDrives)}
            />
          ))}
        </Panel>
        <Panel
          title="Cloud storage"
          right={
            <button title="Connect cloud storage" className={side} onClick={p.onStorage}>
              ＋
            </button>
          }
        >
          {clouds.length === 0 && <p className="text-[11px] text-neutral-500">Connect Nextcloud, Dropbox, OneDrive, S3 or WebDAV storage to bring in its photos.</p>}
          {clouds.map((a) => (
            <div key={a.id} className="mb-1">
              <div className="flex items-center justify-between pl-2 text-[10px] uppercase tracking-wider text-neutral-500">
                <span className="truncate">{a.name}</span>
                <button title={`Download a folder of ${a.name}`} className={side} onClick={() => setBrowse(a)}>
                  ＋
                </button>
              </div>
              {cloudDirs
                .filter((c) => c.id === a.id)
                .map((c) => (
                  <FolderRow key={c.path} label={c.path || "All files"} title={`${a.name} › ${c.path}`} count={count(c.local)} active={f.view === `cloud:${a.id}:${c.path}`} onClick={() => p.onOpenCloud(c, a.name)} />
                ))}
              {!a.folders.length && (
                <button className="px-2 text-[11px] text-sky-400 hover:text-sky-300" onClick={() => setBrowse(a)}>
                  Choose a folder…
                </button>
              )}
            </div>
          ))}
        </Panel>
        <Panel title="Immich" right={immichUrl && manage("immich")}>
          {immichUrl ? (
            <>
              <FolderRow label="★ Favorites" active={f.view === "immich:"} onClick={() => p.onImmich(null, "Favorites")} />
              {albums.map((a) => (
                <FolderRow key={a.id} label={a.name} count={a.count} active={f.view === `immich:${a.id}`} onClick={() => p.onImmich(a.id, a.name)} />
              ))}
            </>
          ) : (
            <Btn className="w-full" onClick={() => p.onConnect("immich")}>
              Connect Immich…
            </Btn>
          )}
        </Panel>
        <Panel title="Google Photos" right={p.settings?.google_client_id && manage("google")}>
          {p.settings?.google_client_id ? (
            <>
              <Btn className="mb-2 w-full" onClick={p.onPickGoogle}>
                Pick photos…
              </Btn>
              <FolderRow label="Picked photos" count={p.folders.find(([d]) => d === googleDir)?.[1] ?? 0} active={f.folder === googleDir} onClick={() => show({ folder: googleDir })} />
            </>
          ) : (
            <Btn className="w-full" onClick={() => p.onConnect("google")}>
              Connect Google Photos…
            </Btn>
          )}
        </Panel>
      </aside>

      <main className="flex min-w-0 flex-1 flex-col">
        <div className="flex flex-wrap items-center gap-2 border-b border-neutral-800 bg-neutral-900 px-3 py-2 text-[11px]">
          <input
            className="w-56 rounded bg-neutral-800 px-2 py-1 outline-none placeholder:text-neutral-500 focus:ring-1 focus:ring-sky-600"
            placeholder="Search filename, camera, lens, keywords"
            value={f.text ?? ""}
            onChange={(e) => p.setFilter({ ...f, text: e.target.value })}
          />
          <label className="flex items-center gap-1 text-neutral-400">
            Rating ≥
            <select className="rounded bg-neutral-800 px-1 py-1" value={f.min_rating ?? 0} onChange={(e) => p.setFilter({ ...f, min_rating: +e.target.value || null })}>
              {[0, 1, 2, 3, 4, 5].map((r) => (
                <option key={r} value={r}>
                  {r ? "★".repeat(r) : "any"}
                </option>
              ))}
            </select>
          </label>
          <select className="rounded bg-neutral-800 px-1 py-1" value={f.flag ?? ""} onChange={(e) => p.setFilter({ ...f, flag: e.target.value || null })}>
            <option value="">Any flag</option>
            <option value="pick">Picks</option>
            <option value="unflagged">Unflagged</option>
            <option value="reject">Rejects</option>
          </select>
          <select className="rounded bg-neutral-800 px-1 py-1" value={f.color_label ?? ""} onChange={(e) => p.setFilter({ ...f, color_label: e.target.value || null })}>
            <option value="">Any label</option>
            <option value="none">No label</option>
            {LABELS.map((l) => (
              <option key={l} value={l}>
                {l}
              </option>
            ))}
          </select>
          <select className="rounded bg-neutral-800 px-1 py-1" value={f.edited == null ? "" : String(f.edited)} onChange={(e) => p.setFilter({ ...f, edited: e.target.value === "" ? null : e.target.value === "true" })}>
            <option value="">Edited or not</option>
            <option value="true">Edited</option>
            <option value="false">Untouched</option>
          </select>
          {f.ids ? (
            <span className="ml-auto flex items-center gap-1.5 rounded bg-sky-900/60 px-2 py-0.5 text-sky-100" title="Photos chosen by an auto function">
              {p.photos.length} from {f.view?.startsWith("auto:") ? f.view.slice(5) : "a result"}
              <button title="Show all photos again" className="text-sky-300 hover:text-white" onClick={() => show({})}>
                ✕
              </button>
            </span>
          ) : (
            <span className="ml-auto text-neutral-500">
              {p.selected.length > 1 ? `${p.selected.length} selected / ` : ""}
              {p.photos.length}
              {p.nextPage ? "+" : ""} photos
            </span>
          )}
          <input type="range" className="ol-range w-24" min={110} max={320} value={size} onChange={(e) => setSize(+e.target.value)} title="Thumbnail size" />
        </div>

        {p.photos.length === 0 && !p.nextPage ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-neutral-500">
            <p>{p.folders.length || f.immich_album ? "No photos here." : "Add a folder of JPEG, TIFF, PNG or RAW files, or connect Immich or Google Photos, to start."}</p>
            {!p.folders.length && !f.immich_album && <Btn onClick={p.onAddFolder}>Add folder…</Btn>}
          </div>
        ) : (
          <div ref={gridRef} className="grid flex-1 content-start gap-2 overflow-y-auto p-3" style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${size}px, 1fr))` }}>
            {p.photos.map((ph) => (
              <Cell key={ph.id} photo={ph} size={size} selected={p.selected.includes(ph.id)} active={p.active?.id === ph.id} onClick={(m) => p.select(ph.id, m)} onOpen={() => p.openDevelop(ph.id)} />
            ))}
            {p.nextPage && (
              <div ref={endRef} className="col-span-full py-4 text-center text-[11px] text-neutral-500">
                Loading more…
              </div>
            )}
          </div>
        )}
      </main>

      <aside className="w-72 shrink-0 overflow-y-auto border-l border-neutral-800 bg-neutral-900">
        {p.active ? <Metadata {...p} photo={p.active} /> : <p className="p-4 text-[11px] text-neutral-500">Select a photo.</p>}
      </aside>
      {browse && (
        <CloudBrowser
          account={browse}
          action="Download this folder"
          onPick={(path) => {
            setBrowse(null);
            p.onAddCloudFolder(browse.id, path, browse.name);
          }}
          onClose={() => setBrowse(null)}
        />
      )}
    </div>
  );
}

function FolderRow({ label, count, active, onClick, title, icon }: { label: string; count?: number; active: boolean; onClick: () => void; title?: string; icon?: ReactNode }) {
  return (
    <div title={title} onClick={onClick} className={`flex cursor-pointer items-center justify-between gap-1.5 rounded px-2 py-1 text-[11px] ${active ? "bg-neutral-700 text-white" : "text-neutral-300 hover:bg-neutral-800"}`}>
      {icon}
      <span className="flex-1 truncate">{label}</span>
      {count != null && <span className="text-neutral-500">{count}</span>}
    </div>
  );
}

export function Cell({ photo, size, selected, active, onClick, onOpen }: { photo: Photo; size: number; selected: boolean; active: boolean; onClick: (m: SelectMods) => void; onOpen: () => void }) {
  return (
    <div
      onClick={(e) => onClick({ shift: e.shiftKey, toggle: e.metaKey || e.ctrlKey })}
      onDoubleClick={onOpen}
      className={`group relative flex cursor-pointer flex-col rounded-md p-1.5 ${active ? "bg-neutral-600" : selected ? "bg-neutral-700" : "bg-neutral-800 hover:bg-neutral-700/60"} ${photo.flag === -1 ? "opacity-40" : ""}`}
    >
      <div className="flex items-center justify-center" style={{ height: size * 0.72 }}>
        <img src={thumbUrl(photo)} loading="lazy" draggable={false} className="max-h-full max-w-full object-contain shadow-md" alt="" />
      </div>
      <div className="mt-1 flex items-center gap-1 text-[10px] text-neutral-400">
        <Stars value={photo.rating} />
        {photo.flag === 1 && <span className="text-white" title="Pick">⚑</span>}
        {photo.flag === -1 && <span className="text-red-400" title="Reject">✕</span>}
        {photo.color_label && <span className="h-2 w-2 rounded-full" style={{ background: LABEL_COLORS[photo.color_label] }} />}
        <span className="ml-auto flex gap-1">
          {photo.copy_name && <span title={photo.copy_name}>VC</span>}
          {photo.edited && <span title="Edited">±</span>}
        </span>
      </div>
      <div className="truncate text-[10px] text-neutral-500">{photo.filename}</div>
    </div>
  );
}

function Metadata(p: Props & { photo: Photo }) {
  const ph = p.photo;
  const [keywords, setKeywords] = useState(ph.keywords);
  useEffect(() => setKeywords(ph.keywords), [ph.id, ph.keywords]);
  const rows: [string, string | number | null][] = [
    ["File", ph.filename + (ph.copy_name ? ` (${ph.copy_name})` : "")],
    ["Folder", ph.folder],
    ["Size", ph.width ? `${ph.width} × ${ph.height}` : null],
    ["Captured", ph.captured_at],
    ["Camera", ph.camera],
    ["Lens", ph.lens],
    ["ISO", ph.iso],
    ["Aperture", ph.aperture ? `f/${ph.aperture.toFixed(1)}` : null],
    ["Shutter", ph.shutter ? `${ph.shutter} s` : null],
    ["Focal length", ph.focal_length ? `${Math.round(ph.focal_length)} mm` : null],
  ];
  return (
    <>
      <Panel title="Quick culling">
        <div className="space-y-2 text-[11px]">
          <div className="flex items-center justify-between">
            <span className="text-neutral-400">Rating</span>
            <span className="text-base">
              <Stars value={ph.rating} onChange={(r) => p.cull({ rating: r })} />
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-neutral-400">Flag</span>
            <span className="flex gap-1">
              {(["pick", "unflagged", "reject"] as const).map((fl) => (
                <Btn key={fl} active={flagName(ph.flag) === fl} onClick={() => p.cull({ flag: fl })}>
                  {fl === "pick" ? "⚑ Pick" : fl === "reject" ? "✕ Reject" : "—"}
                </Btn>
              ))}
            </span>
          </div>
          <div className="flex items-center justify-between">
            <span className="text-neutral-400">Label</span>
            <span className="flex gap-1.5">
              {LABELS.map((l) => (
                <button
                  key={l}
                  title={l}
                  onClick={() => p.cull({ color_label: ph.color_label === l ? "none" : l })}
                  className={`h-4 w-4 rounded-full ${ph.color_label === l ? "ring-2 ring-white" : ""}`}
                  style={{ background: LABEL_COLORS[l] }}
                />
              ))}
            </span>
          </div>
          <input
            className="w-full rounded bg-neutral-800 px-2 py-1 outline-none placeholder:text-neutral-500"
            placeholder="Keywords, comma separated"
            value={keywords}
            onChange={(e) => setKeywords(e.target.value)}
            onBlur={() => keywords !== ph.keywords && p.cull({ keywords })}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
          <p className="text-[10px] text-neutral-500">Keys: 0–5 rating · P/X/U flag · 6–9 label · D develop · ←/→ navigate</p>
        </div>
      </Panel>
      <Panel title="Metadata">
        <dl className="grid grid-cols-[80px_1fr] gap-x-2 gap-y-1 text-[11px]">
          {rows
            .filter(([, v]) => v != null && v !== "")
            .map(([k, v]) => (
              <div key={k} className="contents">
                <dt className="text-neutral-500">{k}</dt>
                <dd className="break-all text-neutral-200">{v}</dd>
              </div>
            ))}
        </dl>
      </Panel>
      <Panel title="Actions">
        <div className="flex flex-wrap gap-1">
          <Btn onClick={() => p.openDevelop(ph.id)}>Develop</Btn>
          <Btn onClick={p.onExport}>
            {ph.path.startsWith("immich://") ? "Save to Immich" : "Export"} {p.selected.length > 1 ? `${p.selected.length}` : ""}…
          </Btn>
          <Btn onClick={p.onVirtualCopy}>Virtual copy</Btn>
          <Btn onClick={p.onRemove} title="Removes from the catalog only; files stay on disk">
            Remove
          </Btn>
        </div>
      </Panel>
    </>
  );
}

const lastFolder = () => {
  try {
    return localStorage.getItem("ols.exportFolder") ?? "";
  } catch {
    return "";
  }
};

/** dest: "" (a folder) | immich | google | cloud:<account id> */
export function ExportDialog({ ids, settings, dest: initialDest = "", onClose }: { ids: number[]; settings: Settings | null; dest?: string; onClose: () => void }) {
  const [format, setFormat] = useState("JPEG");
  const [space, setSpace] = useState("sRGB");
  const [quality, setQuality] = useState(90);
  const [edge, setEdge] = useState("");
  const [folder, setFolder] = useState(lastFolder);
  const [dest, setDest] = useState(initialDest); // "" = folder
  const [uploadPath, setUploadPath] = useState("OpenLight exports");
  const account = dest.startsWith("cloud:") ? settings?.clouds.find((a) => `cloud:${a.id}` === dest) : undefined;
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const [result, setResult] = useState<string | null>(null);
  useEffect(() => {
    const un = listen<{ label?: string; done: number; total: number }>("export-progress", (e) => setProgress(`${e.payload.label ?? "Exporting"} ${e.payload.done}/${e.payload.total}`));
    return () => void un.then((f) => f());
  }, []);
  const hdrOk = format === "PNG_16" || format === "TIFF_16";
  const run = async () => {
    setBusy(true);
    setResult(null);
    setProgress("");
    try {
      localStorage.setItem("ols.exportFolder", folder);
    } catch {
      /* per-viewer convenience only */
    }
    try {
      const files = await api.export({ ids, folder, format, color_space: space, quality, max_edge: edge ? +edge : null, upload: dest || null, upload_path: uploadPath });
      const where = account ? `${account.name} › ${uploadPath.trim() || "/"}` : dest === "immich" ? "Immich" : "Google Photos";
      setResult(dest ? `Uploaded ${files.length} photo(s) to ${where}` : `Exported ${files.length} file(s) to ${folder}`);
    } catch (e) {
      setResult(`Export failed: ${e}`);
    }
    setBusy(false);
  };
  const field = "rounded bg-neutral-800 px-2 py-1 outline-none";
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={() => !busy && onClose()}>
      <div className="w-[440px] space-y-3 rounded-lg border border-neutral-700 bg-neutral-900 p-5 text-[12px] shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <h2 className="text-sm font-semibold">Export {ids.length} photo(s)</h2>
        <div className="grid grid-cols-[110px_minmax(0,1fr)] items-center gap-2">
          <span className="text-neutral-400">Format</span>
          <select className={field} value={format} onChange={(e) => setFormat(e.target.value)}>
            <option value="JPEG">JPEG</option>
            <option value="AVIF">AVIF</option>
            <option value="PNG_16">PNG 16-bit</option>
            <option value="TIFF_16">TIFF 16-bit</option>
          </select>
          <span className="text-neutral-400">Color space</span>
          <select className={field} value={space} onChange={(e) => setSpace(e.target.value)}>
            <option value="sRGB">sRGB</option>
            <option value="DisplayP3">Display P3</option>
            <option value="AdobeRGB">Adobe RGB (1998)</option>
            <option value="Rec2020_HDR" disabled={!hdrOk}>
              Rec. 2020 HDR (PQ){hdrOk ? "" : " — 16-bit formats only"}
            </option>
          </select>
          {(format === "JPEG" || format === "AVIF") && (
            <>
              <span className="text-neutral-400">Quality {quality}</span>
              <input type="range" className="ol-range" min={40} max={100} value={quality} onChange={(e) => setQuality(+e.target.value)} />
            </>
          )}
          <span className="text-neutral-400">Long edge (px)</span>
          <input className={field} placeholder="Full resolution" value={edge} onChange={(e) => setEdge(e.target.value.replace(/\D/g, ""))} />
          <span className="text-neutral-400">Destination</span>
          <select className={field} value={dest} onChange={(e) => setDest(e.target.value)}>
            <option value="">Folder</option>
            <option value="immich" disabled={!settings?.immich_url}>
              Upload to Immich{settings?.immich_url ? "" : " — connect it in Library first"}
            </option>
            <option value="google" disabled={!settings?.google_client_id}>
              Upload to Google Photos{settings?.google_client_id ? "" : " — connect it in Library first"}
            </option>
            {settings?.clouds.map((a) => (
              <option key={a.id} value={`cloud:${a.id}`}>
                Upload to {a.name}
              </option>
            ))}
          </select>
          {account && (
            <>
              <span className="text-neutral-400">Into folder</span>
              <input className={field} value={uploadPath} onChange={(e) => setUploadPath(e.target.value)} placeholder="The top of the storage" />
            </>
          )}
          {!dest && (
            <>
              <span className="text-neutral-400">Folder</span>
              <div className="flex gap-1">
                <input className={`${field} min-w-0 flex-1`} value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="Choose a destination" />
                <Btn
                  onClick={async () => {
                    const d = await open({ directory: true, defaultPath: folder || undefined });
                    if (typeof d === "string") setFolder(d);
                  }}
                >
                  Choose…
                </Btn>
              </div>
            </>
          )}
        </div>
        {result && <p className={result.startsWith("Export failed") ? "text-red-400" : "text-emerald-400"}>{result}</p>}
        <div className="flex justify-end gap-2">
          <Btn onClick={onClose} disabled={busy}>
            Close
          </Btn>
          <Btn active onClick={run} disabled={busy || (!dest && !folder) || (space === "Rec2020_HDR" && !hdrOk)}>
            {busy ? progress || "Exporting…" : dest ? "Upload" : "Export"}
          </Btn>
        </div>
      </div>
    </div>
  );
}

/** Connect or disconnect Immich / Google Photos; their photos are browsed from the sidebar. */
export function RemoteDialog({ source, settings, setSettings, onClose }: { source: Source; settings: Settings; setSettings: (s: Settings) => void; onClose: () => void }) {
  const immich = source === "immich";
  const connected = immich ? !!settings.immich_url : !!settings.google_client_id;
  const [url, setUrl] = useState(settings.immich_url);
  const [clientId, setClientId] = useState(settings.google_client_id);
  const [secret, setSecret] = useState(""); // Immich API key or Google client secret
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const run = async (label: string, job: () => Promise<Settings>) => {
    setBusy(label);
    setError("");
    try {
      setSettings(await job());
      onClose();
    } catch (e) {
      setError(String(e));
    }
    setBusy("");
  };
  const field = "rounded bg-neutral-800 px-2 py-1 outline-none placeholder:text-neutral-500";
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60" onClick={onClose}>
      <div className="w-[460px] space-y-3 rounded-lg border border-neutral-700 bg-neutral-900 p-5 text-[12px] shadow-2xl" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-semibold">{immich ? "Immich" : "Google Photos"}</h2>
          {connected && (
            <Btn disabled={!!busy} onClick={() => run("Disconnecting…", () => api.remoteDisconnect(source))}>
              Disconnect
            </Btn>
          )}
        </div>
        {!connected ? (
          <>
            <div className="grid grid-cols-[110px_1fr] items-center gap-2">
              <span className="text-neutral-400">{immich ? "Server URL" : "Client ID"}</span>
              {immich ? (
                <input className={field} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="http://192.168.1.10:2283" />
              ) : (
                <input className={field} value={clientId} onChange={(e) => setClientId(e.target.value)} placeholder="….apps.googleusercontent.com" />
              )}
              <span className="text-neutral-400">{immich ? "API key" : "Client secret"}</span>
              <input className={field} type="password" value={secret} onChange={(e) => setSecret(e.target.value)} />
            </div>
            <p className="text-[11px] text-neutral-500">
              {immich
                ? "Create a key in Immich under Account Settings → API Keys with album.read, asset.read, asset.view, asset.download, asset.upload, asset.update and stack.create. It is stored in the OS keychain."
                : "In Google Cloud Console, enable the Photos Picker API (and the Photos Library API to save edits), create an OAuth client of type Desktop app and add your account as a test user. Signing in opens your browser."}
            </p>
          </>
        ) : immich ? (
          <p className="text-neutral-300">
            Connected to <span className="text-neutral-100">{settings.immich_url}</span>. Albums appear in the sidebar. Opening a photo downloads it once; Save to Immich stacks your edit on top of the
            original, which stays untouched. Albums shared with you show only the photos in your own library.
          </p>
        ) : (
          <p className="text-neutral-300">
            Signed in. Pick photos opens Google Photos in your browser. Google only lets apps open photos you pick and add new ones, so picked photos are downloaded and saving an edit uploads it as
            a new photo.
          </p>
        )}
        {busy && <p className="text-sky-300">{busy}</p>}
        {error && <p className="break-words text-red-400">{error}</p>}
        <div className="flex justify-end gap-2">
          <Btn onClick={onClose}>Close</Btn>
          {!connected && (
            <Btn
              active
              disabled={!!busy || !secret || !(immich ? url : clientId)}
              onClick={() =>
                run(immich ? "Connecting…" : "Finish signing in to Google in your browser…", () => (immich ? api.immichConnect(url, secret) : api.googleConnect(clientId, secret)))
              }
            >
              {immich ? "Connect" : "Sign in with Google"}
            </Btn>
          )}
        </div>
      </div>
    </div>
  );
}
