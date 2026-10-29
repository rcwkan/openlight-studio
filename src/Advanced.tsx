import { Fragment, ReactNode } from "react";
import { Develop as Dev, Proof, setPath } from "./api";
import { Btn, Panel, Seg, Slider } from "./controls";

type SliderFor = (label: string, path: (string | number)[], min: number, max: number, extra?: Partial<Parameters<typeof Slider>[0]>) => ReactNode;

const UPRIGHT: [string, string, string][] = [
  ["off", "Off", "No Upright correction"],
  ["auto", "Auto", "Balanced level, vertical and horizontal correction"],
  ["level", "Level", "Level the horizon only"],
  ["vertical", "Vertical", "Level and make vertical lines parallel"],
  ["full", "Full", "Level, vertical and horizontal perspective"],
];
const GAMUTS: [string, string][] = [
  ["", "Off"],
  ["sRGB", "sRGB"],
  ["DisplayP3", "P3"],
  ["AdobeRGB", "Adobe RGB"],
];
const sub = "mb-0.5 mt-2.5 flex items-center justify-between text-[10px] uppercase tracking-wider text-neutral-500";
const hint = "mt-1 text-[10px] text-neutral-500";

/** Lightroom's advanced Develop panels, always for the whole photo: lens corrections, Transform with
 *  Upright, calibration, noise reduction, grain, spot removal and soft proofing. */
export function AdvancedTab(p: {
  dev: Dev;
  defaults: Dev;
  change: (d: Dev, label: string) => void;
  S: SliderFor;
  tool: string;
  setTool: (t: "none" | "guides" | "spot") => void;
  upright: (mode: string) => void;
  autoCa: () => void;
  proof: Proof;
  setProof: (p: Proof) => void;
  spotSize: number;
  setSpotSize: (v: number) => void;
}) {
  const { dev, change, S } = p;
  const t = dev.transform;
  const reset = (key: keyof Dev, label: string) => (
    <button className="text-[10px] font-normal normal-case text-neutral-500 hover:text-neutral-200" onClick={() => change({ ...dev, [key]: structuredClone(p.defaults[key]) }, `Reset ${label}`)}>
      reset
    </button>
  );
  const check = (label: string, checked: boolean, onChange: (v: boolean) => void, title?: string) => (
    <label className="mt-1.5 flex items-center gap-1.5 text-[11px] text-neutral-300" title={title}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
  const clearGuides = () =>
    change({ ...dev, transform: { ...t, guides: [], ...(t.upright === "guided" && { upright: "off", vertical: 0, horizontal: 0, rotate: 0 }) } }, "Clear guides");

  return (
    <>
      <Panel title="Lens corrections" right={reset("lens", "lens corrections")}>
        {S("Distortion", ["lens", "distortion"], -100, 100)}
        <div className={sub}>
          Chromatic aberration
          <button className="normal-case text-sky-400 hover:text-sky-300" onClick={p.autoCa} title="Measure the colour fringes along edges and line red and blue up with green">
            auto
          </button>
        </div>
        {S("Red / cyan", ["lens", "ca_red_cyan"], -100, 100)}
        {S("Blue / yellow", ["lens", "ca_blue_yellow"], -100, 100)}
        <div className={sub}>Defringe</div>
        {S("Purple amount", ["lens", "defringe_purple"], 0, 20)}
        {S("Purple from", ["lens", "defringe_purple_hue", 0], 0, 100, { track: "track-fringe-purple" })}
        {S("Purple to", ["lens", "defringe_purple_hue", 1], 0, 100, { track: "track-fringe-purple" })}
        {S("Green amount", ["lens", "defringe_green"], 0, 20)}
        {S("Green from", ["lens", "defringe_green_hue", 0], 0, 100, { track: "track-fringe-green" })}
        {S("Green to", ["lens", "defringe_green_hue", 1], 0, 100, { track: "track-fringe-green" })}
        <div className={sub}>Lens vignetting</div>
        {S("Amount", ["lens", "vignetting"], -100, 100)}
        {S("Midpoint", ["lens", "vignetting_midpoint"], 0, 100)}
      </Panel>

      <Panel title="Transform" right={reset("transform", "transform")}>
        <div className="mb-1 grid grid-cols-3 gap-1">
          {UPRIGHT.map(([m, label, title]) => (
            <Btn key={m} active={t.upright === m && p.tool !== "guides"} title={title} onClick={() => p.upright(m)}>
              {label}
            </Btn>
          ))}
          <Btn active={p.tool === "guides" || t.upright === "guided"} title="Draw 2–4 guides along lines that should be vertical or horizontal" onClick={() => p.setTool(p.tool === "guides" ? "none" : "guides")}>
            Guided
          </Btn>
        </div>
        {(p.tool === "guides" || t.upright === "guided") && (
          <p className={hint}>
            {p.tool === "guides" ? `Draw guides on the photo (${t.guides.length}/4); press Guided again to see the result.` : `Corrected along ${t.guides.length} guides.`}{" "}
            {t.guides.length > 0 && (
              <button className="text-sky-400 hover:text-sky-300" onClick={clearGuides}>
                clear
              </button>
            )}
          </p>
        )}
        {S("Vertical", ["transform", "vertical"], -100, 100)}
        {S("Horizontal", ["transform", "horizontal"], -100, 100)}
        {S("Rotate", ["transform", "rotate"], -10, 10, { step: 0.1, digits: 1 })}
        {S("Aspect", ["transform", "aspect"], -100, 100)}
        {S("Scale", ["transform", "scale"], 50, 150)}
        {S("X offset", ["transform", "offset_x"], -100, 100, { step: 0.1, digits: 1 })}
        {S("Y offset", ["transform", "offset_y"], -100, 100, { step: 0.1, digits: 1 })}
        {check("Constrain crop", t.constrain_crop, (v) => change(setPath(dev, ["transform", "constrain_crop"], v), "Constrain crop"), "Shrink the crop until no empty corners show")}
      </Panel>

      <Panel title="Calibration" open={false} right={reset("calibration", "calibration")}>
        {S("Shadows tint", ["calibration", "shadows_tint"], -100, 100, { track: "track-tint" })}
        {(["red", "green", "blue"] as const).map((c) => (
          <Fragment key={c}>
            <div className={sub}>{c} primary</div>
            {S("Hue", ["calibration", `${c}_hue`], -100, 100, { track: `track-${c}-hue` })}
            {S("Saturation", ["calibration", `${c}_saturation`], -100, 100)}
          </Fragment>
        ))}
      </Panel>

      <Panel title="Noise reduction" open={false}>
        {S("Luminance", ["detail", "luminance_noise_reduction"], 0, 100)}
        {S("Detail", ["detail", "luminance_detail"], 0, 100)}
        {S("Color", ["detail", "color_noise_reduction"], 0, 100)}
        <p className={hint}>The preview is a 1600px proxy; judge noise on the export.</p>
      </Panel>

      <Panel title="Grain" open={false} right={reset("grain", "grain")}>
        {S("Amount", ["grain", "amount"], 0, 100)}
        {S("Size", ["grain", "size"], 0, 100)}
        {S("Roughness", ["grain", "roughness"], 0, 100)}
      </Panel>

      <Panel title="Spot removal" open={false}>
        <Btn active={p.tool === "spot"} className="w-full" onClick={() => p.setTool(p.tool === "spot" ? "none" : "spot")} title="Click spots on the photo to heal them from a matching nearby area (Q)">
          {p.tool === "spot" ? "⊙ Click spots on the photo… (Q to stop)" : "⊙ Heal spots"}
        </Btn>
        <Slider label="Size %" value={p.spotSize} min={0.3} max={8} step={0.1} def={1.5} digits={1} onChange={p.setSpotSize} />
        {check("Visualize spots", p.proof.spots != null, (v) => p.setProof({ ...p.proof, spots: v ? 50 : null }), "While healing, show edges in black and white so dust stands out")}
        {p.proof.spots != null && <Slider label="Threshold" value={p.proof.spots} min={0} max={100} def={50} onChange={(v) => p.setProof({ ...p.proof, spots: v })} />}
        <div className="mt-2 space-y-1">
          {dev.retouch.map((r, i) => (
            <div key={i} className="flex items-center gap-2 rounded bg-neutral-800 px-2 py-1 text-[11px]">
              <span className="flex-1 truncate" title={r.name}>
                {r.name}
              </span>
              <button className="text-neutral-500 hover:text-white" title="Undo this removal" onClick={() => change({ ...dev, retouch: dev.retouch.filter((_, k) => k !== i) }, "Delete removal")}>
                ✕
              </button>
            </div>
          ))}
        </div>
      </Panel>

      <Panel title="Soft proofing" open={false}>
        <div className="mb-1 text-[10px] text-neutral-500">Gamut warning</div>
        <Seg options={GAMUTS} value={p.proof.gamut ?? ""} onChange={(g) => p.setProof({ ...p.proof, gamut: g || null })} />
        {check("Clipping warning (J)", p.proof.clipping, (v) => p.setProof({ ...p.proof, clipping: v }))}
        <p className={hint}>Violet: colours the destination can't show. Red: clipped highlights; blue: crushed shadows.</p>
      </Panel>
    </>
  );
}
