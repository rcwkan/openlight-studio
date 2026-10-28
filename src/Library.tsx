import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, Culling, Filter, flagName, ImmichAlbum, LABEL_COLORS, LABELS, Photo, Settings, Source, thumbUrl } from "./api";
import { Btn, Panel, Stars } from "./controls";

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
  onImport: () => void;
  onRemote: (source: Source) => void;
  onExport: () => void;
  onVirtualCopy: () => void;
  onRemove: () => void;
};

const short = (folder: string) => folder.split(/[\\/]/).slice(-2).join("/");

export function Library(p: Props) {
  const [size, setSize] = useState(170);
  const f = p.filter;
  return (
    <div className="flex min-h-0 flex-1">
      <aside className="flex w-56 shrink-0 flex-col overflow-y-auto border-r border-neutral-800 bg-neutral-900">
        <Panel title="Catalog">
          <Btn className="mb-2 w-full" onClick={p.onImport}>
            Import folder…
          </Btn>
          <Btn className="mb-2 w-full" onClick={() => p.onRemote("immich")}>
            Import from Immich…
          </Btn>
          <Btn className="mb-2 w-full" onClick={() => p.onRemote("google")}>
            Import from Google Photos…
          </Btn>
          <FolderRow label="All Photographs" active={!f.folder} onClick={() => p.setFilter({ ...f, folder: null })} />
        </Panel>
        <Panel title="Folders">
          {p.folders.length === 0 && <p className="text-[11px] text-neutral-500">No folders yet.</p>}
          {p.folders.map(([folder, n]) => (
            <FolderRow key={folder} label={short(folder)} title={folder} count={n} active={f.folder === folder} onClick={() => p.setFilter({ ...f, folder })} />
          ))}
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
          <span className="ml-auto text-neutral-500">
            {p.selected.length > 1 ? `${p.selected.length} selected / ` : ""}
            {p.photos.length} photos
          </span>
          <input type="range" className="ol-range w-24" min={110} max={320} value={size} onChange={(e) => setSize(+e.target.value)} title="Thumbnail size" />
        </div>

        {p.photos.length === 0 ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-neutral-500">
            <p>{p.folders.length ? "No photos match the filter." : "Import a folder of JPEG, TIFF, PNG or RAW files to start."}</p>
            {!p.folders.length && <Btn onClick={p.onImport}>Import folder…</Btn>}
          </div>
        ) : (
          <div className="grid flex-1 content-start gap-2 overflow-y-auto p-3" style={{ gridTemplateColumns: `repeat(auto-fill, minmax(${size}px, 1fr))` }}>
            {p.photos.map((ph) => (
              <Cell key={ph.id} photo={ph} size={size} selected={p.selected.includes(ph.id)} active={p.active?.id === ph.id} onClick={(m) => p.select(ph.id, m)} onOpen={() => p.openDevelop(ph.id)} />
            ))}
          </div>
        )}
      </main>

      <aside className="w-72 shrink-0 overflow-y-auto border-l border-neutral-800 bg-neutral-900">
        {p.active ? <Metadata {...p} photo={p.active} /> : <p className="p-4 text-[11px] text-neutral-500">Select a photo.</p>}
      </aside>
    </div>
  );
}

function FolderRow({ label, count, active, onClick, title }: { label: string; count?: number; active: boolean; onClick: () => void; title?: string }) {
  return (
    <div title={title} onClick={onClick} className={`flex cursor-pointer justify-between rounded px-2 py-1 text-[11px] ${active ? "bg-neutral-700 text-white" : "text-neutral-300 hover:bg-neutral-800"}`}>
      <span className="truncate">{label}</span>
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
          <Btn onClick={p.onExport}>Export {p.selected.length > 1 ? `${p.selected.length}` : ""}…</Btn>
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

export function ExportDialog({ ids, onClose }: { ids: number[]; onClose: () => void }) {
  const [format, setFormat] = useState("JPEG");
  const [space, setSpace] = useState("sRGB");
  const [quality, setQuality] = useState(90);
  const [edge, setEdge] = useState("");
  const [folder, setFolder] = useState(lastFolder);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState("");
  const [result, setResult] = useState<string | null>(null);
  useEffect(() => {
    const un = listen<{ done: number; total: number }>("export-progress", (e) => setProgress(`${e.payload.done}/${e.payload.total}`));
    return () => void un.then((f) => f());
  }, []);
  const hdrOk = format === "PNG_16" || format === "TIFF_16";
  const run = async () => {
    setBusy(true);
    setResult(null);
    try {
      localStorage.setItem("ols.exportFolder", folder);
    } catch {
      /* per-viewer convenience only */
    }
    try {
      const files = await api.export({ ids, folder, format, color_space: space, quality, max_edge: edge ? +edge : null });
      setResult(`Exported ${files.length} file(s) to ${folder}`);
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
        <div className="grid grid-cols-[110px_1fr] items-center gap-2">
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
        </div>
        {result && <p className={result.startsWith("Export failed") ? "text-red-400" : "text-emerald-400"}>{result}</p>}
        <div className="flex justify-end gap-2">
          <Btn onClick={onClose} disabled={busy}>
            Close
          </Btn>
          <Btn active onClick={run} disabled={busy || !folder || (space === "Rec2020_HDR" && !hdrOk)}>
            {busy ? `Exporting ${progress}` : "Export"}
          </Btn>
        </div>
      </div>
    </div>
  );
}

/** Connect Immich / Google Photos and pick what to import; the import itself runs in App (status bar). */
export function RemoteDialog({ source, settings, setSettings, onImport, onClose }: { source: Source; settings: Settings; setSettings: (s: Settings) => void; onImport: (album: string | null) => void; onClose: () => void }) {
  const immich = source === "immich";
  const connected = immich ? !!settings.immich_url : !!settings.google_client_id;
  const [url, setUrl] = useState(settings.immich_url);
  const [clientId, setClientId] = useState(settings.google_client_id);
  const [secret, setSecret] = useState(""); // Immich API key or Google client secret
  const [albums, setAlbums] = useState<ImmichAlbum[]>([]);
  const [album, setAlbum] = useState(""); // "" = favorites
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  useEffect(() => {
    if (immich && connected) api.immichAlbums().then(setAlbums, (e) => setError(String(e)));
  }, [immich, connected]);
  const run = async (label: string, job: () => Promise<Settings>) => {
    setBusy(label);
    setError("");
    try {
      setSettings(await job());
      setSecret("");
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
                ? "Create a key in Immich under Account Settings → API Keys with album.read, asset.read and asset.download. It is stored in the OS keychain."
                : "In Google Cloud Console, enable the Photos Picker API, create an OAuth client of type Desktop app and add your account as a test user. Signing in opens your browser."}
            </p>
          </>
        ) : immich ? (
          <div className="grid grid-cols-[110px_1fr] items-center gap-2">
            <span className="text-neutral-400">Server</span>
            <span className="truncate text-neutral-200">{settings.immich_url}</span>
            <span className="text-neutral-400">Import</span>
            <select className={field} value={album} onChange={(e) => setAlbum(e.target.value)}>
              <option value="">★ Favorites</option>
              {albums.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name} ({a.count})
                </option>
              ))}
            </select>
          </div>
        ) : (
          <p className="text-neutral-300">Signed in. Pick photos opens Google Photos in your browser; press Done there and OpenLight downloads your picks.</p>
        )}
        {connected && (
          <p className="text-[11px] text-neutral-500">
            Originals download into the catalog folder; formats OpenLight can't open (HEIC, video) are skipped.
            {immich && " Albums shared with you import only the photos in your own library."}
          </p>
        )}
        {busy && <p className="text-sky-300">{busy}</p>}
        {error && <p className="break-words text-red-400">{error}</p>}
        <div className="flex justify-end gap-2">
          <Btn onClick={onClose}>Close</Btn>
          {connected ? (
            <Btn active disabled={!!busy} onClick={() => (onImport(immich ? album || null : null), onClose())}>
              {immich ? "Import" : "Pick photos…"}
            </Btn>
          ) : (
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
