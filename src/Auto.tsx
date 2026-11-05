import { ReactNode, useState } from "react";
import { Develop as Dev } from "./api";
import { Btn, Panel, Slider } from "./controls";

/** id (develop::PROFILES), label, what it does */
export const PROFILES: [string, string, string][] = [
  ["natural", "Natural", "Balanced colour and tone (the same as ✦ Auto)"],
  ["neutral", "Neutral", "Flat and low contrast: a starting point for your own grade"],
  ["vivid", "Vivid", "More contrast, clarity and colour"],
  ["portrait", "Portrait", "Warmer, softer skin texture, gentle contrast"],
  ["landscape", "Landscape", "Held-back skies, open shadows, more clarity and colour"],
  ["bw", "B&W", "Black and white with extra contrast"],
  ["film", "Film", "Matte blacks, teal shadows, warm highlights and grain"],
  ["moody", "Moody", "Darker and muted, with a vignette"],
  ["street", "Street", "Muted and cinematic: teal shadows, warm highlights, matte blacks, light grain"],
  ["food", "Food", "Bright and warm, open shadows, rich reds, oranges and greens"],
  ["architecture", "Architecture", "Clean and cool: crisp lines, even tones, pastel skies, little colour"],
  ["night", "Night", "Teal and orange city light, neon held back, deep blacks"],
  ["nature", "Nature", "Macro, flowers and wildlife: lush greens, fine detail, soft vignette"],
  ["teal_orange", "Teal & Orange", "Cinematic: teal shadows, amber highlights, deep shadows, held-back highlights"],
  ["soft_editorial", "Editorial", "Soft and airy: bright, warm midtones, luminous skin, dusty muted colour"],
  ["dark_moody", "Dark Moody", "Low key and atmospheric: compressed highlights, pine greens, muted yellows, vignette"],
  ["analog_film", "35mm Film", "Vintage emulsion: lifted blacks, rolled-off whites, golden warmth, heavy grain"],
  ["fine_art_bw", "Fine-Art B&W", "Zone-system black and white: deep S-curve, dark skies, bright skin, fine grain"],
];
const sub = "mb-1 mt-3 text-[10px] uppercase tracking-wider text-neutral-500";
const hint = "mt-1 text-[10px] text-neutral-500";

type Ask = { ask: (prompt: string) => void; aiBusy?: boolean; aiPrompt?: string };

/** the one-click buttons: measured ones run while showing "Working…", ✦ AI ones send a prompt to the AI bar */
function useRunners(p: Ask) {
  const [busy, setBusy] = useState<string | null>(null);
  const run = (key: string, label: ReactNode, f: () => Promise<void>, title: string, className = "", isActive = false) => (
    <Btn
      key={key}
      className={className}
      title={title}
      disabled={busy != null}
      active={busy === key || isActive}
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
  const ai = (label: string, prompt: string, title: string, first?: () => Promise<void>) => {
    const isRunningThis = !!p.aiBusy && p.aiPrompt === prompt;
    return (
      <Btn
        key={label}
        disabled={!!p.aiBusy}
        active={isRunningThis}
        title={`${title} — asks the AI provider; follow it in the AI bar`}
        onClick={async () => (await first?.(), p.ask(prompt))}
      >
        {isRunningThis ? `✦ ${label}…` : `✦ ${label}`}
      </Btn>
    );
  };
  return { run, ai };
}

/** Every automatic adjustment in one place, always for the whole photo. Measured ones run on the photo
 *  and land as one history step each; ✦ AI ones send a prompt to the AI provider through the AI bar.
 *  Auto HDR lives with the rest of HDR in the HDR tab, AI selections in the Masks tab. */
export function AutoTab(
  p: Ask & {
    profile: (id: string, label: string, strength?: number) => Promise<void>;
    activeProfile?: string | null;
    profileStrength?: number;
    onStrengthChange?: (strength: number) => void;
    /** the profile for what the photo shows (landscape, portrait, food…), picked by the AI provider */
    byType: () => Promise<void>;
    whiteBalance: () => Promise<void>;
    tone: () => Promise<void>;
    levels: () => Promise<void>;
    ca: () => Promise<void>;
    upright: (mode: string) => Promise<void>;
    /** level a tilted horizon (Upright Level) as its own step; ✦ AI fine tune runs it first */
    level: () => Promise<void>;
  },
) {
  const { run, ai } = useRunners(p);
  const [localProfile, setLocalProfile] = useState<string | null>(null);
  const [localStrength, setLocalStrength] = useState<number>(100);

  const activeProfile = p.activeProfile !== undefined ? p.activeProfile : localProfile;
  const strength = p.profileStrength !== undefined ? p.profileStrength : localStrength;

  const onSelect = async (id: string, label: string) => {
    setLocalProfile(id);
    await p.profile(id, label, strength);
  };

  const onStrength = (v: number) => {
    setLocalStrength(v);
    if (p.onStrengthChange) {
      p.onStrengthChange(v);
    } else {
      const profId = activeProfile || "natural";
      const profLabel = PROFILES.find(([k]) => k === profId)?.[1] || "Natural";
      p.profile(profId, profLabel, v);
    }
  };

  return (
    <>
      <Panel title="Auto fine tune">
        <div className="grid grid-cols-2 gap-1">
          {run("auto", "✦ Auto", () => onSelect("natural", "Natural"), "White balance and tone measured on the photo", "", activeProfile === "natural")}
          {run("type", "✦ By photo type", p.byType, "The AI provider tells what the photo shows (landscape, portrait, street, food, architecture, night, nature, B&W), then tunes it to that type's profile")}
          {ai(
            "AI fine tune",
            "Fine tune this photo: natural colour, balanced exposure, no clipped highlights. Keep it realistic. The horizon is levelled separately, so leave crop.angle and transform unchanged.",
            "Level a tilted horizon, then let the AI edit the whole photo",
            p.level,
          )}
        </div>
        <div className={sub}>To a profile</div>
        <div className="grid grid-cols-3 gap-1">
          {PROFILES.map(([id, label, title]) => run(id, label, () => onSelect(id, label), title, "", activeProfile === id))}
        </div>
        <div className="mt-2.5">
          <Slider
            label="Strength"
            value={strength}
            min={0}
            max={200}
            step={1}
            def={100}
            format={(v) => `${v}%`}
            onChange={onStrength}
          />
        </div>
        <p className={hint}>Sets white balance, Basic and presence from the photo, then the profile's look at {strength}% strength. Your other edits stay.</p>
      </Panel>

      <Panel title="One at a time">
        <div className="grid grid-cols-2 gap-1">
          {run("wb", "White balance", p.whiteBalance, "Neutralise a colour cast, judged from the near-grey surfaces")}
          {run("tone", "Tone", p.tone, "Exposure, contrast, highlights, shadows, whites and blacks")}
          {run("levels", "Levels", p.levels, "Each channel's black and white point (Adjust › Levels)")}
          {run("ca", "Chromatic aberration", p.ca, "Line up the colour fringes along edges (Geometry › Lens corrections)")}
        </div>
        <p className={hint}>Auto HDR is in the HDR tab.</p>
      </Panel>

      <Panel title="Auto rotate">
        <div className="grid grid-cols-2 gap-1">
          {run("level", "Straighten", () => p.upright("level"), "Level the horizon from the photo's lines")}
          {run("upright", "Upright", () => p.upright("auto"), "Level and balance the perspective (Geometry › Transform)")}
          {run("vertical", "Verticals", () => p.upright("vertical"), "Level and make vertical lines parallel")}
          {run("full", "Full perspective", () => p.upright("full"), "Level, vertical and horizontal perspective")}
          {ai(
            "Fix orientation",
            "If this photo is sideways or upside down, set image_rotation.angle (clockwise: 90, 180 or 270) so it is upright. If it is already upright, change nothing.",
            "Turn a sideways or upside-down photo upright",
          )}
        </div>
      </Panel>
    </>
  );
}

/** ✦ AI selections in one click, and healing away a selected object; shown above the selections list. */
export function AiSelectPanel(
  p: Ask & {
    dev: Dev;
    /** heal away the objects of selection `mask` */
    remove: (mask: number) => Promise<void>;
  },
) {
  const { run, ai } = useRunners(p);
  const objects = p.dev.masks.flatMap((m, i) => (m.components.some((c) => c.type === "polygon" && c.op === "add") ? [{ m, i }] : []));
  return (
    <Panel title="Quick select & remove">
      <div className="grid grid-cols-3 gap-1">
        {ai("Subject", "Select the main subject.", "A selection of the main subject")}
        {ai("Sky", "Select the sky.", "A selection of the sky")}
        {ai("Remove people", "Remove the people.", "Heal away every person")}
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
        <p className={hint}>Click an object with ✦ Select (S), or use ✦ Subject, then remove it here.</p>
      )}
    </Panel>
  );
}
