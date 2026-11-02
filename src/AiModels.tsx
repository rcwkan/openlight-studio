import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import { AiHardware, AiModel, api, Settings } from "./api";
import { Btn } from "./controls";

const hint = "text-[10px] text-neutral-500";

/** Settings → AI models: this machine's GPU, and the downloadable models for AI denoise and object removal,
 *  with the best one for the hardware marked. The photo tools in Develop's AI tab use what is chosen here. */
export function AiModelsSettings({ settings, setSettings }: { settings: Settings; setSettings: (s: Settings) => void }) {
  const [hw, setHw] = useState<AiHardware | null>(null);
  const [models, setModels] = useState<AiModel[]>([]);
  const [dl, setDl] = useState<{ id: string; done: number; total: number } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => api.aiModels().then((r) => (setHw(r.hardware), setModels(r.models)));
  useEffect(() => void load().catch((e) => setError(String(e))), []);
  useEffect(() => {
    const un = listen<{ id: string; done: number; total: number }>("ai-model-progress", (e) => setDl(e.payload));
    return () => void un.then((f) => f());
  }, []);

  const pick = (key: "ai_denoise_model" | "ai_removal_model", id: string) => {
    const updated = { ...settings, [key]: id };
    setSettings(updated);
    void api.saveSettings(updated);
  };
  const download = async (id: string) => {
    setError(null);
    setDl({ id, done: 0, total: 1 });
    try {
      await api.aiDownload(id);
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setDl(null);
    }
  };
  const remove = (id: string) => void api.aiRemove(id).then(load);

  const gb = (mb: number) => `${(mb / 1024).toFixed(0)} GB`;
  // the model a tool uses: the one chosen here if it is installed, else the best installed one
  const inUse = (group: AiModel[], chosen?: string) => (group.find((m) => m.id === chosen && m.installed) ?? group.find((m) => m.installed))?.id;

  const section = (title: string, text: string, group: AiModel[], key: "ai_denoise_model" | "ai_removal_model") => {
    const current = inUse(group, settings[key]);
    return (
      <section className="rounded-lg border border-neutral-800 bg-neutral-900 p-4">
        <h2 className="font-semibold">{title}</h2>
        <p className={`${hint} mb-3`}>{text}</p>
        <div className="space-y-2">
          {group.map((m) => {
            const busy = dl?.id === m.id;
            return (
              <div key={m.id} className={`rounded border px-3 py-2 ${current === m.id ? "border-sky-600 bg-sky-950/30" : "border-neutral-800 bg-neutral-900/40"}`}>
                <div className="flex flex-wrap items-center gap-1.5">
                  <span className="font-medium text-neutral-100">{m.name}</span>
                  {m.recommended && <span className="rounded bg-emerald-600/80 px-1.5 text-[9px] font-semibold text-white">Recommended for your GPU</span>}
                  {!m.fits && <span className="rounded bg-amber-600/70 px-1.5 text-[9px] text-white">Needs more memory</span>}
                  {current === m.id && <span className="rounded bg-sky-600/80 px-1.5 text-[9px] font-semibold text-white">In use</span>}
                </div>
                <div className="mt-0.5 text-neutral-400">{m.blurb}</div>
                <div className={`${hint} mt-0.5`}>
                  {m.note} · {m.size_mb >= 1024 ? `${(m.size_mb / 1024).toFixed(1)} GB` : `${m.size_mb.toFixed(0)} MB`} · {m.license} · {m.source}
                </div>
                <div className="mt-2 flex items-center gap-1.5">
                  {m.installed ? (
                    <>
                      {current !== m.id && <Btn onClick={() => pick(key, m.id)}>Use this one</Btn>}
                      <Btn disabled={!!dl} onClick={() => remove(m.id)} title="Delete the downloaded files">
                        Remove
                      </Btn>
                    </>
                  ) : (
                    <Btn active={busy} disabled={!!dl} onClick={() => void download(m.id)} title={`Downloads from ${m.source}`}>
                      {busy ? `Downloading… ${Math.round((dl!.done / Math.max(1, dl!.total)) * 100)}%` : `Download (${m.size_mb >= 1024 ? `${(m.size_mb / 1024).toFixed(1)} GB` : `${m.size_mb.toFixed(0)} MB`})`}
                    </Btn>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </section>
    );
  };

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="mx-auto max-w-3xl space-y-4 text-[11px] text-neutral-300">
        <section className="rounded-lg border border-neutral-800 bg-neutral-900 p-4">
          <h2 className="font-semibold">Your hardware</h2>
          {hw ? (
            <div className="mt-1">
              <div className="text-neutral-100">{hw.gpu ?? "No GPU found"}</div>
              <div className="text-neutral-500">
                {hw.vram_mb ? `${gb(hw.vram_mb)} video memory · ` : ""}AI runs on {hw.device}
              </div>
              <p className={`${hint} mt-1`}>The models below are checked against this. Nothing downloads until you click Download; every file is checked after it arrives.</p>
            </div>
          ) : (
            <p className="mt-1 text-neutral-500">Checking…</p>
          )}
        </section>
        {section(
          "Object removal",
          "Used by Develop → AI → Remove objects. Generative fill paints new content into the gap; the fast one only continues what surrounds it.",
          models.filter((m) => m.kind !== "denoise"),
          "ai_removal_model",
        )}
        {section("Denoise", "Used by Develop → AI → Denoise. Both make a new, cleaner copy of the photo.", models.filter((m) => m.kind === "denoise"), "ai_denoise_model")}
        {error && <p className="break-words text-red-400">{error}</p>}
      </div>
    </div>
  );
}
