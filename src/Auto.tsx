import { ReactNode, useState } from "react";
import { Develop as Dev } from "./api";
import { Btn, Panel } from "./controls";

/** id (develop::PROFILES), label, what it does */
const PROFILES: [string, string, string][] = [
  ["natural", "Natural", "Balanced colour and tone (the same as ✦ Auto)"],
  ["neutral", "Neutral", "Flat and low contrast: a starting point for your own grade"],
  ["vivid", "Vivid", "More contrast, clarity and colour"],
  ["portrait", "Portrait", "Warmer, softer skin texture, gentle contrast"],
  ["landscape", "Landscape", "Held-back skies, open shadows, more clarity and colour"],
  ["bw", "B&W", "Black and white with extra contrast"],
  ["film", "Film", "Matte blacks, teal shadows, warm highlights and grain"],
  ["moody", "Moody", "Darker and muted, with a vignette"],
];
const sub = "mb-1 mt-3 text-[10px] uppercase tracking-wider text-neutral-500";
const hint = "mt-1 text-[10px] text-neutral-500";

/** Every automatic adjustment in one place, always for the whole photo. Measured ones run on the photo
 *  and land as one history step each; ✦ AI ones send a prompt to the AI provider through the AI bar. */
export function AutoTab(p: {
  dev: Dev;
  profile: (id: string, label: string) => Promise<void>;
  whiteBalance: () => Promise<void>;
  tone: () => Promise<void>;
  levels: () => Promise<void>;
  ca: () => Promise<void>;
  upright: (mode: string) => Promise<void>;
  /** heal away the objects of selection `mask` */
  remove: (mask: number) => Promise<void>;
  ask: (prompt: string) => void;
}) {
  const [busy, setBusy] = useState<string | null>(null);
  const run = (key: string, label: ReactNode, f: () => Promise<void>, title: string, className = "") => (
    <Btn
      key={key}
      className={className}
      title={title}
      disabled={busy != null}
      active={busy === key}
      onClick={async () => {
        setBusy(key);
        try {
          await f();
        } finally {
          setBusy(null);
        }
      }}
    >
      {busy === key ? "Working…" : label}
    </Btn>
  );
  const ai = (label: string, prompt: string, title: string) => (
    <Btn key={label} title={`${title} — asks the AI provider; follow it in the AI bar`} onClick={() => p.ask(prompt)}>
      ✦ {label}
    </Btn>
  );
  const objects = p.dev.masks.flatMap((m, i) => (m.components.some((c) => c.type === "polygon" && c.op === "add") ? [{ m, i }] : []));

  return (
    <>
      <Panel title="Auto fine tune">
        {run("auto", "✦ Auto", () => p.profile("natural", "Natural"), "White balance and tone measured on the photo", "w-full")}
        <div className={sub}>To a profile</div>
        <div className="grid grid-cols-4 gap-1">{PROFILES.map(([id, label, title]) => run(id, label, () => p.profile(id, label), title))}</div>
        <p className={hint}>Sets white balance, Basic and presence from the photo, then the profile's look. Your other edits stay.</p>
        <div className={sub}>One at a time</div>
        <div className="grid grid-cols-2 gap-1">
          {run("wb", "White balance", p.whiteBalance, "Neutralise a colour cast, judged from the near-grey surfaces")}
          {run("tone", "Tone", p.tone, "Exposure, contrast, highlights, shadows, whites and blacks")}
          {run("levels", "Levels", p.levels, "Each channel's black and white point (Pro › Levels)")}
          {run("ca", "Chromatic aberration", p.ca, "Line up the colour fringes along edges (Advanced › Lens corrections)")}
        </div>
      </Panel>

      <Panel title="Auto rotate">
        <div className="grid grid-cols-2 gap-1">
          {run("level", "Straighten", () => p.upright("level"), "Level the horizon from the photo's lines")}
          {run("upright", "Upright", () => p.upright("auto"), "Level and balance the perspective (Advanced › Transform)")}
          {run("vertical", "Verticals", () => p.upright("vertical"), "Level and make vertical lines parallel")}
          {run("full", "Full perspective", () => p.upright("full"), "Level, vertical and horizontal perspective")}
          {ai(
            "Fix orientation",
            "If this photo is sideways or upside down, set image_rotation.angle (clockwise: 90, 180 or 270) so it is upright. If it is already upright, change nothing.",
            "Turn a sideways or upside-down photo upright",
          )}
        </div>
      </Panel>

      <Panel title="Auto select & remove">
        <div className="grid grid-cols-2 gap-1">
          {ai("Select subject", "Select the main subject.", "A selection of the main subject")}
          {ai("Select sky", "Select the sky.", "A selection of the sky")}
          {ai("Remove people", "Remove the people.", "Heal away every person")}
          {ai("AI fine tune", "Fine tune this photo: natural colour, balanced exposure, no clipped highlights. Keep it realistic.", "Let the AI edit the whole photo")}
        </div>
        <div className={sub}>Remove a selected object</div>
        {objects.length ? (
          <div className="space-y-1">
            {objects.map(({ m, i }) => (
              <div key={m.mask_id || i} className="flex items-center gap-2 rounded border border-neutral-800 px-2 py-1 text-[11px]">
                <span className="min-w-0 flex-1 truncate" title={m.name}>
                  {m.name}
                </span>
                {run(`remove${i}`, "Remove", () => p.remove(i), `Heal ${m.name} away from a matching nearby area`)}
              </div>
            ))}
          </div>
        ) : (
          <p className={hint}>Click an object with ✦ Select (S), or use ✦ Select subject, then remove it here.</p>
        )}
      </Panel>
    </>
  );
}
