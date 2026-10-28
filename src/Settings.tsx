import { ask } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";
import { api, CleanUp, StorageReport } from "./api";
import { Btn } from "./controls";

const card = "rounded-lg border border-neutral-800 bg-neutral-900 p-4";
const heading = "mb-1 text-[11px] font-semibold uppercase tracking-wider text-neutral-400";

/** Decimal units, as Finder shows them. */
const size = (n: number) => {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  for (; n >= 1000 && i < units.length - 1; i++) n /= 1000;
  return `${n.toFixed(i && n < 10 ? 1 : 0)} ${units[i]}`;
};
const num = (n: number) => n.toLocaleString();
const short = (folder: string) => folder.split(/[\\/]/).slice(-2).join("/");

/** Storage usage of the catalog folder, a library analysis and clean-up. */
export function SettingsView() {
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
  const otherBytes = r.other ?? Math.max(0, r.total - (r.catalog + r.thumbs + r.originals + immichAlbumsBytes + r.google));
  const parts: [string, number, string][] = [
    ["Catalog", r.catalog, "bg-sky-500"],
    ["Thumbnails", r.thumbs, "bg-violet-500"],
    ...(immichAlbumsBytes > 0 ? [["Downloaded Immich albums", immichAlbumsBytes, "bg-cyan-500"] as [string, number, string]] : []),
    ["Immich photo cache", r.originals, "bg-amber-500"],
    ["Google Photos picks", r.google, "bg-emerald-500"],
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
    ["thumbs", "Orphaned thumbnails", "Previews of photos that are no longer in the catalog.", `${num(c.thumbs.count)} files · ${size(c.thumbs.bytes)}`, "Delete"],
    ["originals", "Immich photo cache", "Kept after you open or export an Immich photo; downloaded again when needed while Immich is connected.", `${num(c.originals.count)} files · ${size(c.originals.bytes)}`, "Clear"],
    ...(c.immich_albums && c.immich_albums.bytes > 0
      ? [
          [
            "immich_albums" as CleanUp,
            "Downloaded Immich albums",
            "Full albums downloaded from Immich. Photos remain safe on your Immich server.",
            `${num(c.immich_albums.count)} files · ${size(c.immich_albums.bytes)}`,
            "Clear",
            `Remove all downloaded Immich albums (${size(c.immich_albums.bytes)}) from local storage? Photos remain on your Immich server and can be re-synced anytime.`,
          ] as [CleanUp, string, string, string, string, string?],
        ]
      : []),
    [
      "missing",
      "Missing photos",
      "Catalog entries whose file was deleted or moved. Offline folders are left alone.",
      `${num(c.missing.count)} photos`,
      "Remove",
      `Remove ${c.missing.count} missing photo(s) and their virtual copies from the catalog? Their ratings, keywords and edit history are lost.`,
    ],
    ["history", "Edit history", "Undo steps. Every photo keeps its current edits.", `${num(c.history.count)} steps · ${size(c.history.bytes)}`, "Clear", `Clear ${c.history.count} undo step(s)? Every photo keeps its current edits, but earlier steps can no longer be restored.`],
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
                          <span className="ml-1.5 text-neutral-500">({num(alb.photos)} photos)</span>
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
