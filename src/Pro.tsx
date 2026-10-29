import { ReactNode, useState } from "react";
import { Develop as Dev, FAMILIES, getPath, setPath } from "./api";
import { Btn, Panel, Seg, Slider } from "./controls";

type SliderFor = (label: string, path: (string | number)[], min: number, max: number, extra?: Partial<Parameters<typeof Slider>[0]>) => ReactNode;

const hex = (c: number[]) => `#${c.map((v) => Math.round(v * 255).toString(16).padStart(2, "0")).join("")}`;
const rgb = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
const field = "rounded bg-neutral-800 px-1 py-0.5 text-[11px]";
const hint = "mt-1 text-[10px] text-neutral-500";
const swatch = "h-6 w-8 shrink-0 cursor-pointer rounded bg-transparent";

/** Photoshop's most used adjustments that the Lightroom panels lack, applied on top of them. With a
 *  selection selected they tune only it, like an adjustment layer with a layer mask; image rotation
 *  is for the whole photo. */
export function ProTab(p: {
  /** the photo, or the selected selection's tuning */
  dev: Dev;
  edit: (d: Dev, label: string) => void;
  S: SliderFor;
  reset: (key: keyof Dev, label: string) => ReactNode;
  /** no selection: whole-photo panels and auto */
  whole: boolean;
  autoLevels: () => void;
}) {
  const { dev, edit, S, reset } = p;
  const [ch, setCh] = useState<"rgb" | "red" | "green" | "blue">("rgb");
  const [fam, setFam] = useState<(typeof FAMILIES)[number]>("reds");
  const [out, setOut] = useState<"red" | "green" | "blue">("red");
  const set = (path: string[], v: unknown, label: string) => edit(setPath(dev, path, v), label);
  const check = (label: string, path: string[], title?: string) => (
    <label className="mt-1.5 flex items-center gap-1.5 text-[11px] text-neutral-300" title={title}>
      <input type="checkbox" checked={getPath(dev, path) as boolean} onChange={(e) => set(path, e.target.checked, label)} />
      {label}
    </label>
  );
  const { hue_saturation: hs, channel_mixer: cm, gradient_map: gm, image_rotation: r } = dev;
  // the crop turns with the photo, so the same area stays cropped
  const { x, y, w, h } = dev.crop;
  const orient = (label: string, rot: Partial<Dev["image_rotation"]>, crop: Partial<Dev["crop"]>) => edit({ ...dev, image_rotation: { ...r, ...rot }, crop: { ...dev.crop, ...crop } }, label);
  // the engine turns before it mirrors, so with one mirror on the stored turn goes the other way
  const turn = (dir: 1 | -1) =>
    orient(
      dir > 0 ? "Rotate 90° clockwise" : "Rotate 90° counter-clockwise",
      { angle: (r.angle + (r.flip_h !== r.flip_v ? -dir : dir) * 90 + 360) % 360 },
      dir > 0 ? { x: 1 - y - h, y: x, w: h, h: w } : { x: y, y: 1 - x - w, w: h, h: w },
    );

  return (
    <>
      <p className="px-3 pt-2 text-[10px] text-neutral-500">
        Photoshop adjustments on top of Adjust{p.whole ? "." : ", inside this selection like an adjustment layer with a mask."}
      </p>
      <Panel
        title="Levels"
        right={
          <span className="flex gap-2">
            {p.whole && (
              <button className="text-[10px] font-normal normal-case text-sky-400 hover:text-sky-300" onClick={p.autoLevels} title="Black and white point of each channel from the histogram (clips 0.1%)">
                auto
              </button>
            )}
            {reset("levels", "levels")}
          </span>
        }
      >
        <div className="mb-2">
          <Seg options={[["rgb", "RGB"], ["red", "Red"], ["green", "Green"], ["blue", "Blue"]]} value={ch} onChange={setCh} />
        </div>
        {S("Black", ["levels", ch, "black"], 0, 253)}
        {S("Gamma", ["levels", ch, "gamma"], 0.1, 3, { step: 0.01, digits: 2 })}
        {S("White", ["levels", ch, "white"], 2, 255)}
        {S("Output black", ["levels", ch, "out_black"], 0, 255)}
        {S("Output white", ["levels", ch, "out_white"], 0, 255)}
      </Panel>

      <Panel title="Hue / Saturation" right={reset("hue_saturation", "hue/saturation")}>
        {hs.colorize ? (
          <>
            {S("Hue", ["hue_saturation", "colorize_hue"], 0, 360, { track: "track-hue" })}
            {S("Saturation", ["hue_saturation", "colorize_saturation"], 0, 100)}
          </>
        ) : (
          <>
            {S("Hue", ["hue_saturation", "hue"], -180, 180, { track: "track-hue-shift" })}
            {S("Saturation", ["hue_saturation", "saturation"], -100, 100)}
          </>
        )}
        {S("Lightness", ["hue_saturation", "lightness"], -100, 100)}
        {check("Colorize", ["hue_saturation", "colorize"], "Tint everything one hue")}
      </Panel>

      <Panel title="Selective color" open={false} right={reset("selective_color", "selective color")}>
        <div className="mb-2 flex gap-1">
          <select className={`${field} min-w-0 flex-1`} value={fam} onChange={(e) => setFam(e.target.value as typeof fam)}>
            {FAMILIES.map((f) => (
              <option key={f} value={f}>
                {f[0].toUpperCase() + f.slice(1)}
                {dev.selective_color[f].some((v) => v) ? " •" : ""}
              </option>
            ))}
          </select>
          <Seg
            options={[["rel", "Relative"], ["abs", "Absolute"]]}
            value={dev.selective_color.absolute ? "abs" : "rel"}
            onChange={(v) => set(["selective_color", "absolute"], v === "abs", "Selective color method")}
          />
        </div>
        {["Cyan", "Magenta", "Yellow", "Black"].map((l, i) => S(l, ["selective_color", fam, i], -100, 100))}
      </Panel>

      <Panel title="Channel mixer" open={false} right={reset("channel_mixer", "channel mixer")}>
        <div className="mb-2 flex items-center gap-2 text-[10px] text-neutral-500">
          Output
          {cm.monochrome ? (
            <span className="text-neutral-300">Gray</span>
          ) : (
            <div className="flex-1">
              <Seg options={[["red", "Red"], ["green", "Green"], ["blue", "Blue"]]} value={out} onChange={setOut} />
            </div>
          )}
        </div>
        {["Red", "Green", "Blue", "Constant"].map((l, i) => S(l, ["channel_mixer", cm.monochrome ? "gray" : out, i], -200, 200))}
        {check("Monochrome", ["channel_mixer", "monochrome"], "Black and white from the gray mix")}
      </Panel>

      <Panel title="Gradient map" open={false} right={reset("gradient_map", "gradient map")}>
        <div className="mb-1 flex items-center gap-2">
          <input type="color" className={swatch} title="Shadows" value={hex(gm.shadows)} onChange={(e) => set(["gradient_map", "shadows"], rgb(e.target.value), "Gradient map shadows")} />
          <div className="h-3 flex-1 rounded" style={{ background: `linear-gradient(90deg, ${hex(gm.shadows)}, ${hex(gm.highlights)})` }} />
          <input type="color" className={swatch} title="Highlights" value={hex(gm.highlights)} onChange={(e) => set(["gradient_map", "highlights"], rgb(e.target.value), "Gradient map highlights")} />
        </div>
        {S("Amount", ["gradient_map", "amount"], 0, 100)}
      </Panel>

      <Panel title="Filters" open={false} right={reset("filters", "filters")}>
        {S("Gaussian blur", ["filters", "blur"], 0, 100)}
        {S("Posterize", ["filters", "posterize"], 0, 32)}
        {S("Threshold", ["filters", "threshold"], 0, 255)}
        {check("Invert", ["filters", "invert"])}
        <p className={hint}>Posterize below 2 and threshold at 0 are off. Blur a selection to soften just it, e.g. an inverted subject for the background.</p>
      </Panel>

      {p.whole && (
        <Panel title="Image rotation" right={<span className="text-[10px] font-normal normal-case text-neutral-500">{r.angle}°</span>}>
          <div className="grid grid-cols-4 gap-1">
            <Btn title="Rotate 90° counter-clockwise" onClick={() => turn(-1)}>
              ⟲ 90°
            </Btn>
            <Btn title="Rotate 90° clockwise" onClick={() => turn(1)}>
              ⟳ 90°
            </Btn>
            <Btn active={r.flip_h} title="Flip horizontal" onClick={() => orient("Flip horizontal", { flip_h: !r.flip_h }, { x: 1 - x - w })}>
              ⇋ Flip
            </Btn>
            <Btn active={r.flip_v} title="Flip vertical" onClick={() => orient("Flip vertical", { flip_v: !r.flip_v }, { y: 1 - y - h })}>
              ⇵ Flip
            </Btn>
          </div>
        </Panel>
      )}
    </>
  );
}
