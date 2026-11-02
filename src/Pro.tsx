import { ReactNode, useState } from "react";
import { Develop as Dev, FAMILIES, getPath, setPath } from "./api";
import { Btn, CurveEditor, Panel, Seg, Slider } from "./controls";

type SliderFor = (label: string, path: (string | number)[], min: number, max: number, extra?: Partial<Parameters<typeof Slider>[0]>) => ReactNode;

const hex = (c: number[]) => `#${c.map((v) => Math.round(v * 255).toString(16).padStart(2, "0")).join("")}`;
const rgb = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);
const field = "rounded bg-neutral-800 px-1 py-0.5 text-[11px]";
const hint = "mt-1 text-[10px] text-neutral-500";
const swatch = "h-6 w-8 shrink-0 cursor-pointer rounded bg-transparent";
const sub = "mb-0.5 mt-2.5 text-[10px] font-semibold uppercase tracking-wider text-neutral-500";
/** HDR toning methods (Photoshop's), what each does */
const HDR_METHODS: [string, string, string][] = [
  ["fusion", "Merge exposures (simulated bracket)", "Simulates a bracket of exposures from the photo and merges the best exposed, most detailed parts of each"],
  ["local", "Local adaptation", "Squeezes the range region by region and brings the detail back"],
  ["equalize", "Equalize histogram", "Spreads every brightness evenly over the range"],
  ["gamma", "Exposure and gamma", "Exposure and gamma only, the same over the whole photo"],
  ["highlights", "Highlight compression", "Rolls the highlights off into white, shadows untouched"],
];
/** toning curve starting points, [x, y] 0-255 */
const HDR_CURVES: [string, number[][]][] = [
  ["Linear", []],
  ["Contrast", [[0, 0], [64, 52], [192, 204], [255, 255]]],
  ["Strong", [[0, 0], [64, 40], [192, 216], [255, 255]]],
  ["Open shadows", [[0, 0], [64, 84], [255, 255]]],
  ["Matte", [[0, 24], [64, 70], [192, 196], [255, 245]]],
];

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
  autoHdr?: () => void;
  /** the rendered photo's luminance histogram (256 bins), behind the toning curve */
  hist?: Uint32Array | null;
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
  const { hue_saturation: hs, channel_mixer: cm, gradient_map: gm, image_rotation: r, hdr } = dev;
  const local = hdr.method === "local";
  const fusion = hdr.method === "fusion";
  // the simulated bracket, darkest first
  const evs = Array.from({ length: hdr.frames }, (_, k) => (k - (hdr.frames - 1) / 2) * hdr.ev_spacing);
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

      <Panel
        title="HDR toning"
        right={
          <span className="flex gap-2">
            {p.autoHdr && (
              <button
                className="text-[10px] font-normal normal-case text-sky-400 hover:text-sky-300"
                onClick={p.autoHdr}
                title="Automatically optimize HDR toning for this photo"
              >
                auto
              </button>
            )}
            {reset("hdr", "HDR toning")}
          </span>
        }
      >
        {check("HDR toning", ["hdr", "enabled"], "Squeeze the photo's range region by region and bring its detail back, like Photoshop's HDR Toning")}
        <div className={hdr.enabled ? "" : "pointer-events-none opacity-40"}>
          <select
            className={`${field} mt-1.5 w-full`}
            value={hdr.method}
            title={HDR_METHODS.find((m) => m[0] === hdr.method)?.[2]}
            onChange={(e) => set(["hdr", "method"], e.target.value, "HDR method")}
          >
            {HDR_METHODS.map(([v, label, title]) => (
              <option key={v} value={v} title={title}>
                {label}
              </option>
            ))}
          </select>
          {fusion && (
            <>
              <div className={sub}>Simulated bracket</div>
              <Seg
                options={[["3", "3 shots"], ["5", "5 shots"], ["7", "7 shots"]]}
                value={String(hdr.frames) as "3" | "5" | "7"}
                onChange={(v) => set(["hdr", "frames"], Number(v), "HDR bracket shots")}
              />
              {S("Spacing (EV)", ["hdr", "ev_spacing"], 0.5, 4, { step: 0.5, digits: 1 })}
              <div className="mt-1 flex gap-0.5" title="The exposures merged, each as a camera would clip it">
                {evs.map((ev) => (
                  <div
                    key={ev}
                    className="flex-1 rounded-sm py-0.5 text-center text-[9px] font-medium"
                    style={{ background: `hsl(0 0% ${Math.max(8, Math.min(92, 50 + ev * 9))}%)`, color: ev > 0 ? "#111" : "#eee" }}
                  >
                    {ev > 0 ? "+" : ev < 0 ? "−" : ""}
                    {Math.abs(ev)}
                  </div>
                ))}
              </div>
              <div className={sub}>Merge weights</div>
              {S("Well exposed", ["hdr", "exposedness"], 0, 200)}
              {S("Contrast", ["hdr", "contrast_weight"], 0, 200)}
              {S("Saturation", ["hdr", "saturation_weight"], 0, 200)}
              {S("Natural", ["hdr", "natural"], 0, 100)}
            </>
          )}
          {local && (
            <>
              <div className={sub}>Edge glow</div>
              {S("Radius", ["hdr", "radius"], 0, 100)}
              {S("Edge strength", ["hdr", "edge"], 0, 100)}
              {check("Smooth edges", ["hdr", "smooth_edges"], "A second edge-aware pass: smoother regions and fewer halos along strong edges (slower)")}
            </>
          )}
          <div className={sub}>Tone and detail</div>
          {local && S("Compression", ["hdr", "compression"], 0, 100)}
          {S("Exposure", ["hdr", "exposure"], -5, 5, { step: 0.05, digits: 2 })}
          {S("Gamma", ["hdr", "gamma"], 0.1, 3, { step: 0.01, digits: 2 })}
          {local && S("Detail", ["hdr", "detail"], -100, 300)}
          {S("Fine detail", ["hdr", "fine_detail"], -100, 300)}
          <div className={sub}>Advanced</div>
          {local && S("Shadows", ["hdr", "shadows"], -100, 100)}
          {local && S("Highlights", ["hdr", "highlights"], -100, 100)}
          {S("Vibrance", ["hdr", "vibrance"], -100, 100)}
          {S("Saturation", ["hdr", "saturation"], -100, 100)}
          <div className={`${sub} flex items-center justify-between`}>
            Toning curve
            {hdr.curve.length > 0 && (
              <button className="font-normal normal-case tracking-normal text-sky-400 hover:text-sky-300" onClick={() => set(["hdr", "curve"], [], "HDR toning curve")}>
                linear
              </button>
            )}
          </div>
          <CurveEditor points={hdr.curve} hist={p.hist} color="#fbbf24" onChange={(pts) => set(["hdr", "curve"], pts, "HDR toning curve")} />
          <div className="mt-1 flex flex-wrap gap-1">
            {HDR_CURVES.map(([label, pts]) => (
              <Btn key={label} active={JSON.stringify(hdr.curve) === JSON.stringify(pts)} onClick={() => set(["hdr", "curve"], pts, `HDR curve: ${label}`)}>
                {label}
              </Btn>
            ))}
          </div>
        </div>
        <p className={hint}>
          Merge exposures takes each part of the photo from the simulated shots where it is best exposed, most detailed and most colourful; wider spacing and more shots reach deeper into shadows and highlights (a RAW holds the most), but shots beyond the photo's own range are left out. Raise a weight to favour that quality. Natural keeps bright areas such as the sky bright and stops glow along edges; at 0 the merge alone sets the brightness (flatter, greyer skies).
          Radius sets how large the regions are; a low edge strength glows across edges, a high one stops at them. Detail brings back the contrast compression flattened, fine detail the texture. Gamma above 1 opens the shadows.
          The curve works on the toned brightness over the photo's histogram: click to add, drag to move, double-click to delete.
        </p>
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
