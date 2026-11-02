import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { AiModel, api, Settings, Stroke } from "./api";
import { Btn, Slider } from "./controls";

const sub = "mb-1 mt-3 text-[10px] uppercase tracking-wider text-neutral-500";
const hint = "mt-1 text-[10px] text-neutral-500";

/** Photo AI tools: remove objects (paint over them, confirm the snapped selection, fill in) and denoise. Each makes
 *  a new photo next to the original, so the original and its edits are never touched. The models they use are
 *  downloaded and chosen in Settings → AI models. */
export function AiTab(p: {
  photoId: number | null;
  settings: Settings | null;
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
  onOpenPhoto: (id: number) => void;
}) {
  const photoId = p.photoId;
  const [models, setModels] = useState<AiModel[]>([]);
  const [strength, setStrength] = useState(80);
  const [grow, setGrow] = useState(6);
  const [prompt, setPrompt] = useState("");
  const [job, setJob] = useState<{ done: number; total: number } | null>(null);
  const [removing, setRemoving] = useState(false);
  const [fillJob, setFillJob] = useState<{ done: number; total: number } | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => void api.aiModels().then((r) => setModels(r.models), () => setModels([])), []);
  useEffect(() => {
    const un = listen<{ job: string; done: number; total: number }>("job-progress", (e) => {
      if (e.payload.job === "ai-denoise") setJob({ done: e.payload.done, total: e.payload.total });
      if (e.payload.job === "ai-remove") setFillJob({ done: e.payload.done, total: e.payload.total });
    });
    return () => void un.then((f) => f());
  }, []);

  // what each tool uses: the model chosen in Settings if it is installed, else the best installed one
  const use = (group: AiModel[], chosen?: string) => group.find((m) => m.id === chosen && m.installed) ?? group.find((m) => m.installed);
  const denoiseModel = use(models.filter((m) => m.kind === "denoise"), p.settings?.ai_denoise_model);
  const removalModel = use(models.filter((m) => m.kind !== "denoise"), p.settings?.ai_removal_model);
  const generative = removalModel?.kind === "generate";

  const denoise = async () => {
    if (photoId == null || !denoiseModel) return;
    setMsg(null);
    setJob({ done: 0, total: 1 });
    try {
      const r = await api.autoRun("enhance", [photoId], { denoise: true, super_resolution: false, engine: "ai", ai_model: denoiseModel.id, ai_strength: strength }, "ai-denoise");
      const row = r.rows[0] as { error?: string } | undefined;
      setMsg(row?.error ? { ok: false, text: row.error } : { ok: true, text: "Done: the denoised copy is open now; the original is unchanged." });
      if (!row?.error) p.onOpenPhoto((r.rows[0] as { new_id: number }).new_id);
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setJob(null);
    }
  };
  const remove = async () => {
    if (photoId == null || !p.selection || !removalModel) return;
    setMsg(null);
    setRemoving(true);
    try {
      const r = await api.autoRun("enhance", [photoId], { denoise: false, super_resolution: false, engine: "ai", ai_model: removalModel.id, remove_strokes: p.selection, remove_grow: grow, remove_prompt: generative ? prompt : "" }, "ai-remove");
      const row = r.rows[0] as { error?: string } | undefined;
      if (row?.error) setMsg({ ok: false, text: row.error });
      else {
        setMsg({ ok: true, text: "Done: the new photo with the object removed is open now; the original is unchanged." });
        p.onClear();
        p.onOpenPhoto((row as unknown as { new_id: number }).new_id);
      }
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setRemoving(false);
      setFillJob(null);
    }
  };

  const models_link = (
    <button className="underline decoration-dotted hover:text-neutral-200" onClick={p.onOpenModels}>
      Settings → AI models
    </button>
  );
  return (
    <div className="px-3 py-2 text-[11px] text-neutral-300">
      <div className={sub} style={{ marginTop: 0 }}>
        Remove objects
      </div>
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
            <Slider label="Brush size" value={Math.round(p.brush * 1000)} min={5} max={120} def={35} onChange={(v) => p.setBrush(v / 1000)} />
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
              <Btn active className="mt-2" disabled={removing} onClick={() => void remove()} title="Makes a new photo with the object removed; the original is not changed">
                {removing ? (fillJob && fillJob.total > 1 ? `Filling in… ${Math.round(((fillJob.done + 1) / fillJob.total) * 100)}%` : "Removing…") : generative ? "✦ Generative fill" : "✦ Remove this object"}
              </Btn>
            </div>
          )}
          <p className={hint}>Using {removalModel.name}. Change it in {models_link}.</p>
        </>
      ) : (
        <p className="text-neutral-400">Needs an object removal model. Download one in {models_link}.</p>
      )}

      <div className={sub}>Denoise</div>
      {denoiseModel ? (
        <>
          <Slider label="Strength" value={strength} min={0} max={100} def={80} onChange={setStrength} />
          <div className="mt-2 flex items-center gap-2">
            <Btn active disabled={photoId == null || !!job} onClick={() => void denoise()} title="Makes a new, denoised photo next to the original; the original is not changed">
              {job ? (job.total > 1 ? `Denoising… ${Math.round((job.done / job.total) * 100)}%` : "Starting…") : "✦ AI Denoise"}
            </Btn>
            {job && (
              <Btn onClick={() => void api.cancelJob("ai-denoise")} title="Stop">
                Cancel
              </Btn>
            )}
          </div>
          <p className={hint}>Using {denoiseModel.name}. Change it in {models_link}.</p>
        </>
      ) : (
        <p className="text-neutral-400">Needs a denoise model. Download one in {models_link}.</p>
      )}

      <p className={hint}>Both work on the full-size photo, so a big RAW takes a little while.</p>
      {msg && <p className={`mt-2 break-words ${msg.ok ? "text-emerald-400" : "text-red-400"}`}>{msg.text}</p>}
    </div>
  );
}
