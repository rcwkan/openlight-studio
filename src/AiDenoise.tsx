import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { AiModel, api, Develop as Dev, Photo, Settings, Stroke, thumbUrl } from "./api";
import { PROFILES } from "./Auto";
import { Btn, Slider } from "./controls";

const sub = "mb-1 mt-3 text-[10px] uppercase tracking-wider text-neutral-500";
const hint = "mt-1 text-[10px] text-neutral-500";

export type AiJobProgress = {
  label: string;
  done: number;
  total: number;
  percent: number;
  startTime: number;
  elapsedSec: number;
};

/** Progress bar with percentage, phase details, elapsed timer, ETA, and cancellation. */
function AiProgressBar({
  title,
  label,
  done,
  total,
  percent,
  modelName,
  elapsedSec,
  onCancel,
}: {
  title: string;
  label?: string;
  done: number;
  total: number;
  percent: number;
  modelName?: string;
  elapsedSec: number;
  onCancel: () => void;
}) {
  const isIndeterminate = total <= 1 && done === 0;

  const formatTime = (sec: number) => {
    const m = Math.floor(sec / 60);
    const s = sec % 60;
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  const remainingSec =
    done > 0 && total > done && elapsedSec > 0
      ? Math.round((elapsedSec / done) * (total - done))
      : null;

  return (
    <div className="mt-2.5 rounded-lg border border-sky-800/60 bg-neutral-950 p-2.5 shadow-md space-y-2">
      {/* Title & percentage */}
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5 min-w-0">
          <span className="h-2 w-2 rounded-full bg-sky-400 animate-pulse shrink-0" />
          <span className="font-semibold text-neutral-100 truncate">{title}</span>
        </div>
        <div className="tabular-nums font-mono font-bold text-sky-400 text-xs shrink-0">
          {isIndeterminate ? (
            <span className="text-[10px] font-normal text-neutral-400 animate-pulse">Initializing…</span>
          ) : (
            `${percent}%`
          )}
        </div>
      </div>

      {/* Progress Track */}
      <div className="relative h-2 w-full overflow-hidden rounded-full bg-neutral-800">
        <div
          className={`h-full rounded-full bg-gradient-to-r from-sky-500 via-indigo-500 to-purple-500 transition-all duration-300 ${
            isIndeterminate ? "w-full animate-pulse" : ""
          }`}
          style={{ width: isIndeterminate ? "100%" : `${Math.max(4, Math.min(100, percent))}%` }}
        />
      </div>

      {/* Live step label & timer */}
      <div className="flex items-center justify-between gap-2 text-[10px] text-neutral-400">
        <div className="truncate">
          {label ? (
            <span className="text-neutral-200 font-medium">{label}</span>
          ) : total > 1 ? (
            <span>Step {done} of {total}</span>
          ) : (
            <span>Executing neural pass…</span>
          )}
          {modelName && <span className="ml-1 text-neutral-500">({modelName})</span>}
        </div>
        <div className="flex items-center gap-1.5 shrink-0 font-mono text-[10px] text-neutral-400">
          <span>⏱ {formatTime(elapsedSec)}</span>
          {remainingSec != null && remainingSec > 0 && (
            <span className="text-neutral-500">~{remainingSec}s left</span>
          )}
        </div>
      </div>

      {/* Cancel button */}
      <div className="flex justify-end pt-1 border-t border-neutral-900">
        <button
          type="button"
          onClick={onCancel}
          className="rounded px-2 py-0.5 text-[10px] text-neutral-400 hover:text-red-400 hover:bg-neutral-900 border border-neutral-800 transition-colors"
          title="Cancel this AI operation"
        >
          ✕ Cancel
        </button>
      </div>
    </div>
  );
}

/** Photo AI tools: non-destructive in-place object removal and denoise.
 *  Adjustments are stored non-destructively as adjustment layers inside Develop with pixel patches
 *  cached on disk. Original RAW / master files are never duplicated or modified. */
export function AiTab(p: {
  photoId: number | null;
  settings: Settings | null;
  dev: Dev | null;
  onChange: (d: Dev, label: string) => void;
  setDev: (d: Dev) => void;
  /** the brush is on: paint over an object on the photo */
  drawing: boolean;
  onDraw: () => void;
  /** the brush strokes over the object, once snapped to its edges and shown for confirming */
  selection: Stroke[] | null;
  /** brush radius as a fraction of the photo's width */
  brush: number;
  setBrush: (v: number) => void;
  onClear: () => void;
  onOpenModels: () => void;
  /** show a photo this tool just made */
  onOpenPhoto?: (id: number) => void;
  /** run the local AI auto adjustment on the photo: a style (profile id or none), its strength and how far to go, 0-100 */
  onAutoAdjust: (profile: string | null, style: number, amount: number) => Promise<void>;
}) {
  const photoId = p.photoId;
  const [models, setModels] = useState<AiModel[]>([]);
  const [strength, setStrength] = useState(p.dev?.ai_denoise?.strength ?? 80);
  const [grow, setGrow] = useState(6);
  const [prompt, setPrompt] = useState("");
  const [job, setJob] = useState<AiJobProgress | null>(null);
  const [removing, setRemoving] = useState(false);
  const [fillJob, setFillJob] = useState<AiJobProgress | null>(null);
  const [style, setStyle] = useState("");
  const [styleStrength, setStyleStrength] = useState(100);
  const [amount, setAmount] = useState(100);
  const [adjusting, setAdjusting] = useState(false);
  const [swapJob, setSwapJob] = useState<AiJobProgress | null>(null);
  /** the library face picker: open while not null, holding the search text */
  const [faceQuery, setFaceQuery] = useState<string | null>(null);
  const [faces, setFaces] = useState<Photo[]>([]);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => void api.aiModels().then((r) => setModels(r.models), () => setModels([])), []);
  useEffect(() => {
    const un = listen<{ job: string; label?: string; done: number; total: number; finished?: boolean }>("job-progress", (e) => {
      const p = e.payload;
      if (p.finished) {
        if (p.job === "ai-denoise") setJob(null);
        if (p.job === "ai-faceswap") setSwapJob(null);
        if (p.job === "ai-remove") {
          setFillJob(null);
          setRemoving(false);
        }
        return;
      }
      const pct = p.total > 0 ? Math.min(100, Math.round((p.done / p.total) * 100)) : 0;
      if (p.job === "ai-denoise") {
        setJob((prev) => ({
          label: p.label || "AI Denoising…",
          done: p.done,
          total: p.total,
          percent: pct,
          startTime: prev?.startTime ?? Date.now(),
          elapsedSec: Math.floor((Date.now() - (prev?.startTime ?? Date.now())) / 1000),
        }));
      }
      if (p.job === "ai-faceswap") {
        setSwapJob((prev) => ({
          label: p.label || "Swapping faces…",
          done: p.done,
          total: p.total,
          percent: pct,
          startTime: prev?.startTime ?? Date.now(),
          elapsedSec: Math.floor((Date.now() - (prev?.startTime ?? Date.now())) / 1000),
        }));
      }
      if (p.job === "ai-remove") {
        setFillJob((prev) => ({
          label: p.label || "Removing object…",
          done: p.done,
          total: p.total,
          percent: pct,
          startTime: prev?.startTime ?? Date.now(),
          elapsedSec: Math.floor((Date.now() - (prev?.startTime ?? Date.now())) / 1000),
        }));
      }
    });
    return () => void un.then((f) => f());
  }, []);

  useEffect(() => {
    if (!job && !fillJob && !swapJob && !removing) return;
    const interval = setInterval(() => {
      if (job) {
        setJob((prev) => (prev ? { ...prev, elapsedSec: Math.floor((Date.now() - prev.startTime) / 1000) } : null));
      }
      if (fillJob) {
        setFillJob((prev) => (prev ? { ...prev, elapsedSec: Math.floor((Date.now() - prev.startTime) / 1000) } : null));
      }
      if (swapJob) {
        setSwapJob((prev) => (prev ? { ...prev, elapsedSec: Math.floor((Date.now() - prev.startTime) / 1000) } : null));
      }
    }, 1000);
    return () => clearInterval(interval);
  }, [job != null, fillJob != null, swapJob != null, removing]);

  useEffect(() => {
    if (faceQuery == null) return;
    const t = setTimeout(() => void api.query({ text: faceQuery.trim() || null, limit: 120 }).then(setFaces, () => setFaces([])), 200);
    return () => clearTimeout(t);
  }, [faceQuery]);

  useEffect(() => {
    if (p.dev?.ai_denoise?.strength != null) {
      setStrength(p.dev.ai_denoise.strength);
    }
  }, [p.dev?.ai_denoise?.strength]);

  // what each tool uses: the model chosen in Settings if it is installed, else the best installed one
  const use = (group: AiModel[], chosen?: string) => group.find((m) => m.id === chosen && m.installed) ?? group.find((m) => m.installed);
  const denoiseModel = use(models.filter((m) => m.kind === "denoise"), p.settings?.ai_denoise_model);
  const removalModel = use(models.filter((m) => m.kind === "inpaint" || m.kind === "generate"), p.settings?.ai_removal_model);
  const faceModel = use(models.filter((m) => m.kind === "faceswap"));
  const generative = removalModel?.kind === "generate";

  const autoAdjust = async () => {
    setMsg(null);
    setAdjusting(true);
    try {
      await p.onAutoAdjust(style || null, styleStrength, amount);
    } finally {
      setAdjusting(false);
    }
  };
  const denoise = async () => {
    if (photoId == null || !denoiseModel) return;
    setMsg(null);
    setJob({
      label: "Initializing denoise model…",
      done: 0,
      total: 1,
      percent: 0,
      startTime: Date.now(),
      elapsedSec: 0,
    });
    try {
      const newDev = await api.aiDenoiseInplace(photoId, denoiseModel.id, strength);
      p.setDev(newDev);
      setMsg({ ok: true, text: "AI Denoise applied in-place. Adjust strength or toggle anytime." });
    } catch (e) {
      if (String(e).toLowerCase().includes("cancel")) {
        setMsg({ ok: false, text: "AI Denoise cancelled." });
      } else {
        setMsg({ ok: false, text: String(e) });
      }
    } finally {
      setJob(null);
    }
  };

  const remove = async () => {
    if (photoId == null || !p.selection || !removalModel) return;
    setMsg(null);
    setRemoving(true);
    const isMoebius = removalModel.id === "moebius" || removalModel.id.startsWith("moebius");
    setFillJob({
      label: generative ? "Initializing diffusion model…" : isMoebius ? "Initializing Moebius model…" : "Preparing object removal…",
      done: 0,
      total: generative ? 25 : isMoebius ? 19 : 3,
      percent: 0,
      startTime: Date.now(),
      elapsedSec: 0,
    });
    try {
      const newDev = await api.aiInpaintInplace(photoId, removalModel.id, p.selection, grow, generative ? prompt : "");
      p.setDev(newDev);
      p.onClear();
      setMsg({ ok: true, text: "Object removed in-place. You can toggle or delete it below." });
    } catch (e) {
      if (String(e).toLowerCase().includes("cancel")) {
        setMsg({ ok: false, text: "Object removal cancelled." });
      } else {
        setMsg({ ok: false, text: String(e) });
      }
    } finally {
      setRemoving(false);
      setFillJob(null);
    }
  };

  const faceSwap = async (source: number) => {
    if (photoId == null || !faceModel) return;
    setFaceQuery(null);
    setMsg(null);
    setSwapJob({ label: "Loading face models…", done: 0, total: 1, percent: 0, startTime: Date.now(), elapsedSec: 0 });
    try {
      p.setDev(await api.aiFaceSwap(photoId, faceModel.id, source));
      setMsg({ ok: true, text: "Face swapped in-place. Each face is its own layer below: toggle or delete it." });
    } catch (e) {
      setMsg({ ok: false, text: String(e).toLowerCase().includes("cancel") ? "Face swap cancelled." : String(e) });
    } finally {
      setSwapJob(null);
    }
  };

  const toggleRemoval = (removalId: string) => {
    if (!p.dev || !p.dev.ai_removals) return;
    const nextRemovals = p.dev.ai_removals.map((r) =>
      r.id === removalId ? { ...r, enabled: !r.enabled } : r,
    );
    const updated = { ...p.dev, ai_removals: nextRemovals };
    p.onChange(updated, "Toggle AI removal");
  };

  const deleteRemoval = async (removalId: string) => {
    if (photoId == null) return;
    try {
      const newDev = await api.aiDeleteRemoval(photoId, removalId);
      p.setDev(newDev);
      setMsg({ ok: true, text: "AI layer deleted." });
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    }
  };

  const onStrengthChange = (val: number) => {
    setStrength(val);
    if (p.dev && p.dev.ai_denoise?.model) {
      const updated = { ...p.dev, ai_denoise: { ...p.dev.ai_denoise, strength: val } };
      p.onChange(updated, "AI Denoise Strength");
    }
  };

  const toggleDenoise = () => {
    if (!p.dev || !p.dev.ai_denoise) return;
    const updated = {
      ...p.dev,
      ai_denoise: { ...p.dev.ai_denoise, enabled: !p.dev.ai_denoise.enabled },
    };
    p.onChange(updated, updated.ai_denoise.enabled ? "Enable AI Denoise" : "Disable AI Denoise");
  };

  const hasDenoised = !!p.dev?.ai_denoise?.model;
  const denoiseEnabled = p.dev?.ai_denoise?.enabled ?? false;

  const models_link = (
    <button className="underline decoration-dotted hover:text-neutral-200" onClick={p.onOpenModels}>
      Settings → AI models
    </button>
  );

  return (
    <div className="px-3 py-2 text-[11px] text-neutral-300">
      <div className={sub} style={{ marginTop: 0 }}>
        Auto adjust
      </div>
      <p className="text-neutral-400">A retouch model trained on photographers' edits looks at the photo and sets white balance, tone and colour for you. Every slider stays editable.</p>
      <div className="mt-2">
        <div className="mb-1 text-neutral-500">Style</div>
        <select
          className="w-full rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-100 outline-none focus:border-sky-600"
          value={style}
          onChange={(e) => setStyle(e.target.value)}
        >
          <option value="">None: the AI's own look</option>
          {PROFILES.map(([id, label]) => (
            <option key={id} value={id}>
              {label}
            </option>
          ))}
        </select>
        {style && <p className={hint}>{PROFILES.find(([id]) => id === style)?.[2]}</p>}
      </div>
      <Slider label="Adjustment" value={amount} min={0} max={100} def={100} onChange={setAmount} />
      {style && <Slider label="Style strength" value={styleStrength} min={0} max={100} def={100} onChange={setStyleStrength} />}
      <Btn active className="mt-2" disabled={photoId == null || adjusting} onClick={() => void autoAdjust()} title="Sets the sliders on this photo; Ctrl+Z undoes it">
        {adjusting ? "Adjusting…" : "✦ AI Auto Adjust"}
      </Btn>
      <p className={hint}>Built in, runs on this computer in about a second.</p>

      <div className={sub}>Remove objects</div>
      {removalModel ? (
        <>
          <p className="text-neutral-400">Paint over what you want gone; its edges snap to the photo, then you confirm.</p>
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            <Btn active={p.drawing} disabled={photoId == null || removing} onClick={p.onDraw} title="Paint over the object on the photo (Esc cancels)">
              🖌 {p.drawing ? "Painting… drag on the photo" : p.selection ? "Paint more" : "Paint over the object"}
            </Btn>
            {p.selection && (
              <Btn onClick={p.onClear} disabled={removing}>
                Cancel
              </Btn>
            )}
          </div>
          <div className="mt-1">
            <Slider label="Brush size" value={Math.round(p.brush * 1000)} min={5} max={120} def={12} onChange={(v) => p.setBrush(v / 1000)} />
          </div>
          {p.selection && (
            <div className="mt-2">
              <div className="text-emerald-300">The red area will be removed.</div>
              <Slider label="Grow edge (px)" value={grow} min={0} max={30} def={6} onChange={setGrow} />
              {generative && (
                <input
                  className="mt-1 w-full rounded border border-neutral-700 bg-neutral-900 px-2 py-1 text-[11px] text-neutral-100 outline-none focus:border-sky-600"
                  placeholder="What should fill the gap? (optional: grass, sky…)"
                  value={prompt}
                  onChange={(e) => setPrompt(e.target.value)}
                />
              )}
              <Btn active className="mt-2" disabled={removing} onClick={() => void remove()} title="Removes the object non-destructively in-place on this photo">
                {removing
                  ? fillJob && fillJob.total > 1
                    ? `Removing… ${fillJob.percent}%`
                    : "Removing…"
                  : generative
                  ? "✦ Generative fill"
                  : "✦ Remove this object"}
              </Btn>
              {removing && (
                <AiProgressBar
                  title="Removing Object"
                  label={fillJob?.label || (generative ? "Synthesizing generative fill…" : (removalModel.id === "moebius" || removalModel.id.startsWith("moebius")) ? "Synthesizing diffusion fill…" : "Inpainting patch…")}
                  done={fillJob?.done ?? 0}
                  total={fillJob?.total ?? (generative ? 25 : (removalModel.id === "moebius" || removalModel.id.startsWith("moebius")) ? 19 : 3)}
                  percent={fillJob?.percent ?? (generative ? 0 : (removalModel.id === "moebius" || removalModel.id.startsWith("moebius")) ? 0 : 33)}
                  modelName={removalModel.name}
                  elapsedSec={fillJob?.elapsedSec ?? 0}
                  onCancel={() => void api.cancelJob("ai-remove")}
                />
              )}
            </div>
          )}
          <p className={hint}>Using {removalModel.name}. Change it in {models_link}.</p>

        </>
      ) : (
        <p className="text-neutral-400">Needs an object removal model. Download one in {models_link}.</p>
      )}

      <div className={sub}>Face swap</div>
      {faceModel ? (
        <>
          <p className="text-neutral-400">Pick a library photo of someone; their face goes onto every face in this photo.</p>
          <Btn
            active
            className="mt-2"
            disabled={photoId == null || !!swapJob}
            onClick={() => setFaceQuery("")}
            title="Choose the library photo with the face to use"
          >
            {swapJob ? `Swapping… ${swapJob.percent}%` : "✦ Choose a face from the library…"}
          </Btn>
          {faceQuery != null &&
            createPortal(
              <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6" onClick={() => setFaceQuery(null)}>
                <div
                  className="flex max-h-full w-[760px] max-w-full flex-col rounded-lg border border-neutral-700 bg-neutral-900 p-4 text-[12px] text-neutral-300 shadow-2xl"
                  onClick={(e) => e.stopPropagation()}
                >
                  <div className="mb-2 flex items-center justify-between gap-2">
                    <h2 className="text-sm font-semibold text-neutral-100">Choose the face to use</h2>
                    <Btn onClick={() => setFaceQuery(null)}>Close</Btn>
                  </div>
                  <input
                    autoFocus
                    className="w-full rounded border border-neutral-700 bg-neutral-950 px-2 py-1.5 text-neutral-100 outline-none focus:border-sky-600"
                    placeholder="Search the library (name, keyword, camera…)"
                    value={faceQuery}
                    onChange={(e) => setFaceQuery(e.target.value)}
                    onKeyDown={(e) => e.key === "Escape" && setFaceQuery(null)}
                  />
                  <div className="mt-3 min-h-0 flex-1 overflow-y-auto">
                    <div className="grid grid-cols-[repeat(auto-fill,minmax(130px,1fr))] gap-2">
                      {faces
                        .filter((ph) => ph.id !== photoId)
                        .map((ph) => (
                          <button
                            key={ph.id}
                            type="button"
                            onClick={() => void faceSwap(ph.id)}
                            title={`Use the face in ${ph.filename}`}
                            className="group rounded border border-neutral-800 bg-neutral-950 p-1 text-left hover:border-sky-500 focus:border-sky-500 focus:outline-none"
                          >
                            <img src={thumbUrl(ph)} alt={ph.filename} loading="lazy" className="aspect-square w-full rounded-sm object-cover" />
                            <div className="mt-1 truncate text-[10px] text-neutral-400 group-hover:text-neutral-200">{ph.filename}</div>
                          </button>
                        ))}
                    </div>
                    {faces.length === 0 && <p className="py-6 text-center text-neutral-500">No photos match.</p>}
                  </div>
                </div>
              </div>,
              document.body,
            )}
          {swapJob && (
            <AiProgressBar
              title="Face Swap"
              label={swapJob.label}
              done={swapJob.done}
              total={swapJob.total}
              percent={swapJob.percent}
              modelName={faceModel.name}
              elapsedSec={swapJob.elapsedSec}
              onCancel={() => void api.cancelJob("ai-faceswap")}
            />
          )}
          <p className={hint}>Using {faceModel.name}. Only swap faces of people who agreed to it.</p>
        </>
      ) : (
        <p className="text-neutral-400">Needs the face swap model. Download it in {models_link}.</p>
      )}

      {/* AI layers: object removals and face swaps */}
      {p.dev?.ai_removals && p.dev.ai_removals.length > 0 && (
        <div className="mt-3 space-y-1.5 border-t border-neutral-800/80 pt-2">
          <div className="text-[10px] font-medium uppercase tracking-wider text-neutral-400">
            AI layers ({p.dev.ai_removals.length})
          </div>
          <div className="max-h-48 space-y-1 overflow-y-auto pr-0.5">
            {p.dev.ai_removals.map((r, i) => (
              <div
                key={r.id}
                className="group flex items-center justify-between rounded bg-neutral-900/70 px-2 py-1 border border-neutral-800 text-[11px] hover:border-neutral-700"
              >
                <label className="flex items-center gap-2 cursor-pointer flex-1 min-w-0">
                  <input
                    type="checkbox"
                    checked={r.enabled}
                    onChange={() => toggleRemoval(r.id)}
                    className="rounded border-neutral-700 text-sky-500 focus:ring-0"
                  />
                  <span className={`truncate ${r.enabled ? "text-neutral-200" : "text-neutral-500 line-through"}`}>
                    {r.label || `Removal ${i + 1}`}
                  </span>
                </label>
                <div className="flex items-center gap-1.5 shrink-0 ml-2">
                  <span className="text-[9px] px-1 py-0.5 rounded bg-neutral-800 text-neutral-400">
                    {r.model}
                  </span>
                  <button
                    onClick={() => void deleteRemoval(r.id)}
                    className="text-neutral-500 hover:text-red-400 p-0.5 rounded"
                    title="Delete this layer"
                  >
                    ✕
                  </button>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      <div className={sub}>Denoise</div>
      {denoiseModel ? (
        <>
          {hasDenoised && (
            <label className="mb-2 flex items-center gap-2 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={denoiseEnabled}
                onChange={toggleDenoise}
                className="rounded border-neutral-700 text-sky-500 focus:ring-0"
              />
              <span className={`font-medium ${denoiseEnabled ? "text-neutral-200" : "text-neutral-500"}`}>
                Enable AI Denoise {denoiseEnabled ? "" : "(Bypassed)"}
              </span>
            </label>
          )}

          <Slider
            label="Strength"
            value={Math.round(strength)}
            min={0}
            max={100}
            def={80}
            onChange={onStrengthChange}
          />

          <div className="mt-2 flex items-center gap-2">
            <Btn
              active
              disabled={photoId == null || !!job}
              onClick={() => void denoise()}
              title="Runs AI Denoise non-destructively in-place on this photo"
            >
              {job ? (
                job.total > 1 ? (
                  `Denoising… ${job.percent}%`
                ) : (
                  "Starting…"
                )
              ) : hasDenoised ? (
                "✦ Re-run AI Denoise"
              ) : (
                "✦ AI Denoise"
              )}
            </Btn>
            {job && (
              <Btn onClick={() => void api.cancelJob("ai-denoise")} title="Stop">
                Cancel
              </Btn>
            )}
          </div>
          {job && (
            <AiProgressBar
              title="AI Denoise"
              label={job.label}
              done={job.done}
              total={job.total}
              percent={job.percent}
              modelName={denoiseModel.name}
              elapsedSec={job.elapsedSec}
              onCancel={() => void api.cancelJob("ai-denoise")}
            />
          )}
          <p className={hint}>Using {denoiseModel.name}. Change it in {models_link}.</p>
        </>
      ) : (
        <p className="text-neutral-400">Needs a denoise model. Download one in {models_link}.</p>
      )}

      <p className={hint}>Non-destructive in-place adjustments. Never creates duplicate master photos.</p>
      {msg && <p className={`mt-2 break-words ${msg.ok ? "text-emerald-400" : "text-red-400"}`}>{msg.text}</p>}
    </div>
  );
}
