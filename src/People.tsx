import { listen } from "@tauri-apps/api/event";
import { ask } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState } from "react";
import { api, Face, faceUrl, FaceStats, JobProgress, Person } from "./api";
import { Btn } from "./controls";

const hint = "text-[10px] text-neutral-500";
const card = "rounded-lg border border-neutral-800 bg-neutral-900 p-3";
const nameOf = (p: Person) => p.name || `Unnamed ${p.id}`;

/** Simple bar chart: horizontal bars with labels, or vertical columns. */
function Bars({ data, vertical, onPick }: { data: [string, number][]; vertical?: boolean; onPick?: (i: number) => void }) {
  const max = Math.max(1, ...data.map((d) => d[1]));
  if (!data.length) return <p className={hint}>Nothing yet.</p>;
  if (vertical)
    return (
      <div className="flex h-28 items-end gap-1">
        {data.map(([k, v], i) => (
          <div key={k} className="flex min-w-0 flex-1 flex-col items-center gap-0.5" title={`${k}: ${v}`}>
            <span className="text-[9px] text-neutral-400">{v}</span>
            <div className="w-full rounded-t bg-[var(--theme-accent,#0ea5e9)]/80" style={{ height: `${(v / max) * 80}px`, minHeight: 2 }} onClick={() => onPick?.(i)} />
            <span className="w-full truncate text-center text-[9px] text-neutral-500">{k}</span>
          </div>
        ))}
      </div>
    );
  return (
    <div className="space-y-1">
      {data.map(([k, v], i) => (
        <button key={i} className="flex w-full items-center gap-2 text-left text-[10px] hover:text-white" onClick={() => onPick?.(i)} disabled={!onPick}>
          <span className="w-24 shrink-0 truncate text-neutral-300">{k}</span>
          <span className="h-2.5 flex-1 overflow-hidden rounded bg-neutral-800">
            <span className="block h-full rounded bg-[var(--theme-accent,#0ea5e9)]/80" style={{ width: `${(v / max) * 100}%` }} />
          </span>
          <span className="w-8 shrink-0 text-right text-neutral-400">{v}</span>
        </button>
      ))}
    </div>
  );
}

function FaceImg({ id, size = 64, selected, onClick, title }: { id: number | null; size?: number; selected?: boolean; onClick?: () => void; title?: string }) {
  return (
    <button
      title={title}
      onClick={onClick}
      style={{ width: size, height: size }}
      className={`shrink-0 overflow-hidden rounded-full bg-neutral-800 ring-2 transition-all ${selected ? "ring-[var(--theme-accent,#0ea5e9)]" : "ring-transparent hover:ring-neutral-600"}`}
    >
      {id != null && <img src={faceUrl(id)} alt="" className="h-full w-full object-cover" loading="lazy" draggable={false} />}
    </button>
  );
}

const years = (faces: Face[]): [string, number][] => {
  const m = new Map<string, Set<number>>();
  for (const f of faces) {
    const y = f.captured_at?.slice(0, 4);
    if (y) m.set(y, (m.get(y) ?? new Set()).add(f.photo_id));
  }
  return [...m].sort().map(([y, s]) => [y, s.size]);
};

/** People: the faces found in the library grouped into people, to name, merge, tidy up and search by. */
export function People({ jobs, onShowPhotos, onOpenSettings }: { jobs: Record<string, JobProgress>; onShowPhotos: (people: number[], label: string) => void; onOpenSettings: () => void }) {
  const [people, setPeople] = useState<Person[]>([]);
  const [stats, setStats] = useState<FaceStats | null>(null);
  const [sel, setSel] = useState<number[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  const [minPhotos, setMinPhotos] = useState(2);
  const [showHidden, setShowHidden] = useState(false);
  const [sort, setSort] = useState<"photos" | "name">("photos");
  const [error, setError] = useState<string | null>(null);
  const scan = jobs["faces-scan"];

  const load = useCallback(() => {
    void api.people().then(setPeople).catch((e) => setError(String(e)));
    void api.faceStats().then(setStats).catch(() => {});
  }, []);
  useEffect(() => {
    load();
    const un = listen("faces-changed", load);
    return () => void un.then((f) => f());
  }, [load]);
  useEffect(() => void (!scan && load()), [scan, load]);

  const run = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(String(e));
    }
    load();
  };

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = people.filter((p) => (showHidden || !p.hidden) && p.photos >= minPhotos && (!q || nameOf(p).toLowerCase().includes(q)));
    return sort === "name" ? [...list].sort((a, b) => (!a.name ? 1 : 0) - (!b.name ? 1 : 0) || nameOf(a).localeCompare(nameOf(b))) : list;
  }, [people, query, minPhotos, showHidden, sort]);
  const byId = useMemo(() => new Map(people.map((p) => [p.id, p])), [people]);
  const selPeople = sel.map((id) => byId.get(id)).filter(Boolean) as Person[];
  const toggle = (id: number) => setSel((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));
  const show = (ids: number[]) => onShowPhotos(ids, ids.map((id) => (byId.get(id) ? nameOf(byId.get(id)!) : `#${id}`)).join(" & "));

  if (open != null && byId.get(open))
    return <PersonView person={byId.get(open)!} people={people} onBack={() => (setOpen(null), load())} onShow={() => show([open])} onChanged={load} onRenamed={(id) => (setOpen(id), load())} />;

  const remaining = stats ? stats.eligible - stats.scanned : 0;
  return (
    <div className="flex-1 overflow-y-auto p-5 text-[11px] text-neutral-300">
      <div className="mx-auto max-w-6xl space-y-4">
        <div className="flex flex-wrap items-center gap-2">
          <h1 className="text-base font-semibold text-neutral-100">People</h1>
          <span className={hint}>{stats ? `${stats.scanned} of ${stats.eligible} photos scanned` : ""}</span>
          <div className="ml-auto flex items-center gap-1.5">
            {scan ? (
              <>
                <span className="text-sky-400">
                  {scan.label} ({Math.round((scan.done / Math.max(1, scan.total)) * 100)}%)
                </span>
                <Btn onClick={() => void api.cancelJob("faces-scan")}>Stop</Btn>
              </>
            ) : (
              <Btn active={remaining > 0} disabled={remaining <= 0} onClick={() => void run(() => api.facesScan())} title="Find faces in the photos not scanned yet">
                {remaining > 0 ? `Scan ${remaining} photo${remaining === 1 ? "" : "s"}` : "All photos scanned"}
              </Btn>
            )}
            <Btn
              disabled={!!scan}
              title="Forget every face and person, to scan again from scratch"
              onClick={async () => (await ask("Forget all faces and people, including their names? Photos are not touched.", { title: "Reset People", kind: "warning" })) && void run(api.resetFaces)}
            >
              Reset…
            </Btn>
          </div>
        </div>
        {error && (
          <p className="break-words text-red-400">
            {error}
            {error.includes("model") && (
              <button className="ml-2 underline" onClick={onOpenSettings}>
                Open AI models
              </button>
            )}
          </p>
        )}

        {stats && (
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-6">
            {(
              [
                ["Photos scanned", `${stats.scanned} / ${stats.eligible}`],
                ["Photos with people", stats.with_faces],
                ["Faces", stats.faces],
                ["People", stats.people],
                ["Named", stats.named],
                ["Hidden", stats.hidden],
              ] as [string, string | number][]
            ).map(([k, v]) => (
              <div key={k} className={card}>
                <div className={hint}>{k}</div>
                <div className="text-lg font-semibold text-neutral-100">{v}</div>
              </div>
            ))}
          </div>
        )}

        {stats && stats.faces > 0 && (
          <div className="grid gap-2 lg:grid-cols-3">
            <div className={card}>
              <h2 className="mb-2 font-semibold">Most photographed</h2>
              <Bars data={people.filter((p) => !p.hidden).slice(0, 10).map((p) => [nameOf(p), p.photos])} onPick={(i) => setOpen(people.filter((p) => !p.hidden)[i].id)} />
            </div>
            <div className={card}>
              <h2 className="mb-2 font-semibold">Photos with people, by year</h2>
              <Bars vertical data={stats.by_year} />
            </div>
            <div className={card}>
              <h2 className="mb-2 font-semibold">Faces per photo</h2>
              <Bars vertical data={stats.per_photo.map(([n, c]) => [n >= 6 ? "6+" : String(n), c])} />
            </div>
          </div>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Find a person…" className="w-44 rounded bg-neutral-800 px-2 py-1 text-[11px] text-neutral-100 outline-none" />
          <label className="flex items-center gap-1">
            In at least
            <select value={minPhotos} onChange={(e) => setMinPhotos(+e.target.value)} className="rounded bg-neutral-800 px-1 py-0.5">
              {[1, 2, 3, 5, 10, 25].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
            photo{minPhotos === 1 ? "" : "s"}
          </label>
          <label className="flex items-center gap-1">
            <input type="checkbox" checked={showHidden} onChange={(e) => setShowHidden(e.target.checked)} /> Show hidden
          </label>
          <select value={sort} onChange={(e) => setSort(e.target.value as "photos" | "name")} className="rounded bg-neutral-800 px-1 py-0.5">
            <option value="photos">Most photos first</option>
            <option value="name">By name</option>
          </select>
          <span className={hint}>
            {shown.length} of {people.length} people
          </span>
          {sel.length > 0 && (
            <div className="ml-auto flex flex-wrap items-center gap-1.5">
              <span className="text-neutral-400">{sel.length} selected</span>
              <Btn active onClick={() => show(sel)} title="Library: the photos showing all of them">
                Show photos
              </Btn>
              <Btn
                disabled={sel.length < 2}
                title="They are the same person"
                onClick={() => void run(async () => (await api.mergePeople(sel), setSel([])))}
              >
                Merge
              </Btn>
              <Btn onClick={() => void run(async () => (await api.hidePeople(sel, !selPeople.every((p) => p.hidden)), setSel([])))}>
                {selPeople.every((p) => p.hidden) ? "Unhide" : "Hide"}
              </Btn>
              <Btn
                onClick={async () =>
                  (await ask(`Forget ${sel.length} ${sel.length === 1 ? "person" : "people"} and their faces? Photos are not touched, and these faces are not found again.`, { title: "Delete people", kind: "warning" })) &&
                  void run(async () => (await api.deletePeople(sel), setSel([])))
                }
              >
                Delete
              </Btn>
              <Btn onClick={() => setSel([])}>Clear</Btn>
            </div>
          )}
        </div>

        {stats && stats.faces === 0 && !scan && (
          <div className={`${card} text-center text-neutral-400`}>
            No faces yet. Scan the library to find them{remaining > 0 ? "" : ", or import some photos"}. Faces are grouped automatically; name the people you know, merge any that were split.
          </div>
        )}
        <div className="grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-3">
          {shown.map((p) => (
            <div key={p.id} className={`flex flex-col items-center gap-1 rounded-lg p-2 ${sel.includes(p.id) ? "bg-neutral-800" : "hover:bg-neutral-900"} ${p.hidden ? "opacity-50" : ""}`}>
              <FaceImg id={p.cover} size={88} selected={sel.includes(p.id)} onClick={() => toggle(p.id)} title="Click to select" />
              <NameInput person={p} onDone={load} />
              <button className="text-[10px] text-neutral-500 hover:text-neutral-200" onClick={() => setOpen(p.id)}>
                {p.photos} photo{p.photos === 1 ? "" : "s"} · open
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function NameInput({ person, onDone }: { person: Person; onDone: (person: number) => void }) {
  const [v, setV] = useState(person.name);
  useEffect(() => setV(person.name), [person.name]);
  const save = () => void (v.trim() !== person.name && api.renamePerson(person.id, v).then(onDone));
  return (
    <input
      value={v}
      title="A name someone else already has merges the two"
      placeholder="Add a name"
      onChange={(e) => setV(e.target.value)}
      onBlur={save}
      onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
      className="w-full rounded bg-transparent px-1 text-center text-[11px] text-neutral-100 outline-none placeholder:text-neutral-600 focus:bg-neutral-800"
    />
  );
}

/** Library toolbar: pick people by face to show only the photos (of the current view) that show all of them. */
export function FaceSearch({ people: picked, label, onChange }: { people: number[]; label?: string; onChange: (people: number[], label: string) => void }) {
  const [open, setOpen] = useState(false);
  const [all, setAll] = useState<Person[]>([]);
  const [q, setQ] = useState("");
  useEffect(() => void api.people().then(setAll).catch(() => setAll([])), [open]);
  const byId = new Map(all.map((p) => [p.id, p]));
  const names = (ids: number[]) => ids.map((id) => (byId.get(id) ? nameOf(byId.get(id)!) : `#${id}`)).join(" & ");
  const toggle = (id: number) => {
    const ids = picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id];
    onChange(ids, names(ids));
  };
  const query = q.trim().toLowerCase();
  // named people first, then the most photographed; people seen once only when searched for
  const list = all
    .filter((p) => !p.hidden && (query ? nameOf(p).toLowerCase().includes(query) : p.name || p.photos > 1))
    .sort((a, b) => (!a.name ? 1 : 0) - (!b.name ? 1 : 0) || b.photos - a.photos);

  return (
    <div className="relative">
      <div className={`flex items-center overflow-hidden rounded ${picked.length ? "bg-[var(--theme-accent,#0ea5e9)]/20 text-[var(--theme-accent,#0ea5e9)]" : "bg-neutral-800 text-neutral-300"}`}>
        <button className="max-w-56 truncate px-2 py-1 hover:brightness-125" title="Search by faces: photos showing all the people picked" onClick={() => setOpen((o) => !o)}>
          👤 {picked.length ? label || names(picked) : "Faces"}
        </button>
        {picked.length > 0 && (
          <button className="px-1.5 py-1 hover:brightness-125" title="Show everyone" onClick={() => (onChange([], ""), setOpen(false))}>
            ✕
          </button>
        )}
      </div>
      {open && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setOpen(false)} />
          <div className="absolute left-0 top-full z-50 mt-1 w-72 rounded-lg border border-neutral-700 bg-neutral-900 p-2 shadow-2xl">
            <input autoFocus value={q} onChange={(e) => setQ(e.target.value)} placeholder="Find a person…" className="mb-2 w-full rounded bg-neutral-800 px-2 py-1 text-[11px] text-neutral-100 outline-none" />
            <div className="max-h-80 space-y-0.5 overflow-y-auto">
              {list.map((p) => (
                <button key={p.id} onClick={() => toggle(p.id)} className={`flex w-full items-center gap-2 rounded px-1.5 py-1 text-left ${picked.includes(p.id) ? "bg-neutral-800" : "hover:bg-neutral-800/60"}`}>
                  <input type="checkbox" readOnly checked={picked.includes(p.id)} className="pointer-events-none" />
                  <FaceImg id={p.cover} size={28} />
                  <span className={`flex-1 truncate ${p.name ? "text-neutral-100" : "text-neutral-500"}`}>{nameOf(p)}</span>
                  <span className="text-[10px] text-neutral-500">{p.photos}</span>
                </button>
              ))}
              {!list.length && <p className="px-1 py-2 text-[10px] text-neutral-500">{all.length ? "Nobody matches." : "No people yet: scan your photos in People."}</p>}
            </div>
            {picked.length > 1 && <p className="mt-1.5 px-1 text-[10px] text-neutral-500">Photos showing all {picked.length} of them.</p>}
          </div>
        </>
      )}
    </div>
  );
}

/** One person: their faces, to set the cover, move wrong ones out, delete non-faces, and pull in similar faces. */
function PersonView({ person, people, onBack, onShow, onChanged, onRenamed }: { person: Person; people: Person[]; onBack: () => void; onShow: () => void; onChanged: () => void; onRenamed: (person: number) => void }) {
  const [faces, setFaces] = useState<Face[]>([]);
  const [sel, setSel] = useState<number[]>([]);
  const [similar, setSimilar] = useState<Face[] | null>(null);
  const [simSel, setSimSel] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(() => void api.facesOf(person.id).then(setFaces).catch((e) => setError(String(e))), [person.id]);
  useEffect(load, [load]);
  const run = async (f: () => Promise<unknown>) => {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(String(e));
    }
    setSel([]);
    load();
    onChanged();
  };
  const toggle = (id: number, set: typeof setSel) => set((s) => (s.includes(id) ? s.filter((x) => x !== id) : [...s, id]));
  const others = people.filter((p) => p.id !== person.id);
  const byId = new Map(people.map((p) => [p.id, p]));

  return (
    <div className="flex-1 overflow-y-auto p-5 text-[11px] text-neutral-300">
      <div className="mx-auto max-w-6xl space-y-4">
        <div className="flex flex-wrap items-center gap-3">
          <Btn onClick={onBack}>← People</Btn>
          <FaceImg id={person.cover} size={56} />
          <div className="w-48 text-base">
            <NameInput person={person} onDone={onRenamed} />
          </div>
          <span className={hint}>
            {person.photos} photos · {person.faces} faces
          </span>
          <div className="ml-auto flex gap-1.5">
            <Btn active onClick={onShow}>
              Show photos
            </Btn>
            <Btn onClick={() => void api.hidePeople([person.id], !person.hidden).then(onChanged)}>{person.hidden ? "Unhide" : "Hide"}</Btn>
          </div>
        </div>
        {error && <p className="break-words text-red-400">{error}</p>}
        <div className={card}>
          <h2 className="mb-2 font-semibold">Photos by year</h2>
          <Bars vertical data={years(faces)} />
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          <span className="text-neutral-400">{sel.length ? `${sel.length} selected` : "Click faces to select them"}</span>
          {sel.length > 0 && (
            <>
              <Btn disabled={sel.length !== 1} onClick={() => void run(() => api.setCover(person.id, sel[0]))}>
                Use as cover
              </Btn>
              <Btn disabled={sel.length !== 1} onClick={() => void api.facesSimilar(sel[0]).then((r) => (setSimilar(r), setSimSel([])))} title="Faces of other people that look like this one">
                Find similar
              </Btn>
              <select
                value=""
                onChange={(e) => e.target.value && void run(() => api.moveFaces(sel, e.target.value === "new" ? null : +e.target.value))}
                className="rounded bg-neutral-800 px-1 py-1"
              >
                <option value="">Move to…</option>
                <option value="new">A new person</option>
                {others
                  .filter((p) => p.name)
                  .sort((a, b) => a.name.localeCompare(b.name))
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
              </select>
              <Btn onClick={() => void run(() => api.deleteFaces(sel))} title="Not a face, or nobody to keep: forget these faces">
                Delete faces
              </Btn>
              <Btn onClick={() => setSel(faces.map((f) => f.id))}>Select all</Btn>
              <Btn onClick={() => setSel([])}>Clear</Btn>
            </>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          {faces.map((f) => (
            <FaceImg key={f.id} id={f.id} size={72} selected={sel.includes(f.id)} onClick={() => toggle(f.id, setSel)} title={`${f.filename}${f.captured_at ? ` · ${f.captured_at}` : ""}`} />
          ))}
        </div>

        {similar && (
          <div className={card}>
            <div className="mb-2 flex flex-wrap items-center gap-1.5">
              <h2 className="font-semibold">Similar faces of other people</h2>
              <span className={hint}>{similar.length ? "Pick the ones that are this person" : "None found"}</span>
              <div className="ml-auto flex gap-1.5">
                <Btn
                  active
                  disabled={!simSel.length}
                  onClick={() => void run(async () => (await api.moveFaces(simSel, person.id), setSimilar(null)))}
                >
                  Add {simSel.length || ""} to {nameOf(person)}
                </Btn>
                <Btn onClick={() => setSimSel(similar.map((f) => f.id))}>Select all</Btn>
                <Btn onClick={() => setSimilar(null)}>Close</Btn>
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              {similar.map((f) => (
                <div key={f.id} className="flex flex-col items-center">
                  <FaceImg id={f.id} size={64} selected={simSel.includes(f.id)} onClick={() => toggle(f.id, setSimSel)} title={f.filename} />
                  <span className="max-w-16 truncate text-[9px] text-neutral-500">{f.person_id != null && byId.get(f.person_id) ? nameOf(byId.get(f.person_id)!) : ""}</span>
                  <span className="text-[9px] text-neutral-600">{Math.round((f.similarity ?? 0) * 100)}%</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
