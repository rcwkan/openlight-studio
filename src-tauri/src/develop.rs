//! Develop engine: the non-destructive edit graph (spec §6), the render pipeline,
//! histogram telemetry and export encoders.
//!
//! ponytail: the pipeline runs on the CPU with rayon over a 1600px proxy (full size on
//! export). One implementation serves the viewport, MCP previews and export, so agents
//! see exactly what the user sees. Port the per-pixel stages to wgpu compute when 60fps
//! at full resolution matters.

use image::{
    DynamicImage, ImageBuffer, ImageDecoder, ImageEncoder, imageops::FilterType,
    metadata::Orientation,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{f32::consts::PI, path::Path, sync::OnceLock};

pub(crate) type M3 = [[f32; 3]; 3];
pub(crate) type Px = [f32; 3];

pub fn es(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Edit graph
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Develop {
    /// "Color" (default) or "Monochrome".
    pub profile: String,
    pub white_balance: WhiteBalance,
    pub basic_tone: BasicTone,
    pub presence: Presence,
    pub tone_curve: ToneCurve,
    pub color_mixer: ColorMixer,
    pub point_color: Vec<Swatch>,
    pub color_grading_3way: ColorGrading,
    pub calibration: Calibration,
    pub detail: Detail,
    pub effects: Effects,
    pub grain: Grain,
    pub lens: Lens,
    pub transform: Transform,
    /// Pro: Photoshop adjustments on top of the developed photo.
    pub levels: Levels,
    pub hue_saturation: HueSaturation,
    pub selective_color: SelectiveColor,
    pub channel_mixer: ChannelMixer,
    pub gradient_map: GradientMap,
    pub filters: Filters,
    pub image_rotation: ImageRotation,
    pub crop: Crop,
    pub retouch: Vec<Heal>,
    pub masks: Vec<Mask>,
    pub ai_provenance_log: Vec<Value>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct WhiteBalance {
    /// ponytail: 5500K is treated as "as shot" for every file; derive the real as-shot
    /// CCT from the camera neutral + colour matrix when RAW WB readouts matter.
    pub temperature_k: f32,
    pub tint: f32,
}
impl Default for WhiteBalance {
    fn default() -> Self {
        Self { temperature_k: 5500.0, tint: 0.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct BasicTone {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Presence {
    pub texture: f32,
    pub clarity: f32,
    pub dehaze: f32,
    pub vibrance: f32,
    pub saturation: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ToneCurve {
    pub parametric: Parametric,
    /// Points are [input, output] in 0–255 display values.
    pub point_luma: Vec<[f32; 2]>,
    pub point_red: Vec<[f32; 2]>,
    pub point_green: Vec<[f32; 2]>,
    pub point_blue: Vec<[f32; 2]>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Parametric {
    pub shadows: f32,
    pub darks: f32,
    pub lights: f32,
    pub highlights: f32,
    pub splits: [f32; 3],
}
impl Default for Parametric {
    fn default() -> Self {
        Self { shadows: 0.0, darks: 0.0, lights: 0.0, highlights: 0.0, splits: [25.0, 50.0, 75.0] }
    }
}

/// 8 bands: red, orange, yellow, green, aqua, blue, purple, magenta.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ColorMixer {
    pub hue: [f32; 8],
    pub saturation: [f32; 8],
    pub luminance: [f32; 8],
}

/// Point Color swatch. `src_hsl` = [Oklch hue°, chroma×250, L×100].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Swatch {
    pub id: String,
    pub src_hsl: [f32; 3],
    pub shift_hsl: [f32; 3],
    pub range_hsl: [f32; 3],
    pub falloff: f32,
}
impl Default for Swatch {
    fn default() -> Self {
        Self { id: String::new(), src_hsl: [0.0; 3], shift_hsl: [0.0; 3], range_hsl: [50.0; 3], falloff: 50.0 }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Wheel {
    pub hue: f32,
    pub sat: f32,
    pub lum: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ColorGrading {
    pub shadows: Wheel,
    pub midtones: Wheel,
    pub highlights: Wheel,
    pub global: Wheel,
    pub blending: f32,
    pub balance: f32,
}
impl Default for ColorGrading {
    fn default() -> Self {
        Self {
            shadows: Wheel::default(),
            midtones: Wheel::default(),
            highlights: Wheel::default(),
            global: Wheel::default(),
            blending: 50.0,
            balance: 0.0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Detail {
    pub sharpening: f32,
    pub sharpen_radius: f32,
    pub sharpen_masking: f32,
    pub color_noise_reduction: f32,
    /// Luminance noise reduction 0-100, and how much edge detail it keeps (0-100).
    pub luminance_noise_reduction: f32,
    pub luminance_detail: f32,
}
impl Default for Detail {
    fn default() -> Self {
        Self {
            sharpening: 0.0,
            sharpen_radius: 1.0,
            sharpen_masking: 0.0,
            color_noise_reduction: 0.0,
            luminance_noise_reduction: 0.0,
            luminance_detail: 50.0,
        }
    }
}

/// Camera calibration, -100..100: shadows tint (+ magenta) and each primary's hue (+ toward the
/// next primary: red to green to blue) and saturation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Calibration {
    pub shadows_tint: f32,
    pub red_hue: f32,
    pub red_saturation: f32,
    pub green_hue: f32,
    pub green_saturation: f32,
    pub blue_hue: f32,
    pub blue_saturation: f32,
}

/// Film grain after the crop: amount, size and roughness 0-100.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Grain {
    pub amount: f32,
    pub size: f32,
    pub roughness: f32,
}
impl Default for Grain {
    fn default() -> Self {
        Self { amount: 0.0, size: 25.0, roughness: 50.0 }
    }
}

/// Manual lens corrections. Distortion is geometry (stage 9); chromatic aberration, vignetting and
/// defringe act on the full frame right after white balance.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Lens {
    /// -100..100: + straightens barrel distortion, - pincushion.
    pub distortion: f32,
    /// Lateral chromatic aberration, -100..100: radial scale of the red and blue channels.
    pub ca_red_cyan: f32,
    pub ca_blue_yellow: f32,
    /// Defringe amount 0-20 and hue range [low, high] 0-100 (purple: blue to magenta; green: yellow to cyan).
    pub defringe_purple: f32,
    pub defringe_purple_hue: [f32; 2],
    pub defringe_green: f32,
    pub defringe_green_hue: [f32; 2],
    /// Lens vignetting -100..100 (+ brightens the corners) and its midpoint 0-100.
    pub vignetting: f32,
    pub vignetting_midpoint: f32,
}
impl Default for Lens {
    fn default() -> Self {
        Self {
            distortion: 0.0,
            ca_red_cyan: 0.0,
            ca_blue_yellow: 0.0,
            defringe_purple: 0.0,
            defringe_purple_hue: [30.0, 70.0],
            defringe_green: 0.0,
            defringe_green_hue: [40.0, 60.0],
            vignetting: 0.0,
            vignetting_midpoint: 50.0,
        }
    }
}

/// Perspective: a virtual camera turn about the frame centre (vertical = tilt, horizontal = pan,
/// rotate = roll), then aspect, scale and offset. Upright modes solve the first three.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Transform {
    /// off | auto | level | vertical | full | guided: the Upright mode that set vertical, horizontal and rotate.
    pub upright: String,
    /// Guided Upright lines [[x, y], [x, y]] in the full frame (normalised).
    pub guides: Vec<[[f32; 2]; 2]>,
    /// -100..100
    pub vertical: f32,
    pub horizontal: f32,
    /// -10..10 degrees, + counter-clockwise like the crop angle.
    pub rotate: f32,
    /// -100..100, + stretches horizontally.
    pub aspect: f32,
    /// 50-150 %
    pub scale: f32,
    /// -100..100: shift by up to half the frame.
    pub offset_x: f32,
    pub offset_y: f32,
    /// Shrink the crop until no empty area shows.
    pub constrain_crop: bool,
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            upright: "off".into(),
            guides: vec![],
            vertical: 0.0,
            horizontal: 0.0,
            rotate: 0.0,
            aspect: 0.0,
            scale: 100.0,
            offset_x: 0.0,
            offset_y: 0.0,
            constrain_crop: true,
        }
    }
}
impl Transform {
    fn is_identity(&self) -> bool {
        [self.vertical, self.horizontal, self.rotate, self.aspect, self.offset_x, self.offset_y] == [0.0; 6] && self.scale == 100.0
    }
}

/// Photoshop Levels for one channel, in 0-255 display values: input black and white points, gamma
/// (midtones, 0.1-9.99), output black and white.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Level {
    pub black: f32,
    pub gamma: f32,
    pub white: f32,
    pub out_black: f32,
    pub out_white: f32,
}
impl Default for Level {
    fn default() -> Self {
        Self { black: 0.0, gamma: 1.0, white: 255.0, out_black: 0.0, out_white: 255.0 }
    }
}
impl Level {
    /// Display value 0-1 -> 0-1.
    fn eval(&self, x: f32) -> f32 {
        let t = ((x * 255.0 - self.black) / (self.white - self.black).max(1.0)).clamp(0.0, 1.0);
        (self.out_black + t.powf(1.0 / self.gamma.max(0.01)) * (self.out_white - self.out_black)) / 255.0
    }
}

/// Photoshop Levels: the RGB composite, then each channel.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Levels {
    pub rgb: Level,
    pub red: Level,
    pub green: Level,
    pub blue: Level,
}

/// Photoshop Hue/Saturation (master): hue -180..180 degrees, saturation and lightness -100..100 (±100
/// lightness = white / black). Colorize gives the photo one hue (0-360, HSL wheel) at colorize_saturation 0-100.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct HueSaturation {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
    pub colorize: bool,
    pub colorize_hue: f32,
    pub colorize_saturation: f32,
}
impl Default for HueSaturation {
    fn default() -> Self {
        Self { hue: 0.0, saturation: 0.0, lightness: 0.0, colorize: false, colorize_hue: 30.0, colorize_saturation: 25.0 }
    }
}

/// Photoshop Selective Color: [cyan, magenta, yellow, black] -100..100 per colour family. Relative (the
/// default) scales the ink a pixel already has, so it cannot touch pure white; absolute adds ink.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct SelectiveColor {
    pub absolute: bool,
    pub reds: [f32; 4],
    pub yellows: [f32; 4],
    pub greens: [f32; 4],
    pub cyans: [f32; 4],
    pub blues: [f32; 4],
    pub magentas: [f32; 4],
    pub whites: [f32; 4],
    pub neutrals: [f32; 4],
    pub blacks: [f32; 4],
}

/// Photoshop Channel Mixer: each output channel as [red, green, blue, constant] % (-200..200);
/// monochrome makes all three the gray mix.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ChannelMixer {
    pub red: [f32; 4],
    pub green: [f32; 4],
    pub blue: [f32; 4],
    pub monochrome: bool,
    pub gray: [f32; 4],
}
impl Default for ChannelMixer {
    fn default() -> Self {
        Self {
            red: [100.0, 0.0, 0.0, 0.0],
            green: [0.0, 100.0, 0.0, 0.0],
            blue: [0.0, 0.0, 100.0, 0.0],
            monochrome: false,
            gray: [40.0, 40.0, 20.0, 0.0],
        }
    }
}

/// Photoshop Gradient Map: each pixel's lightness picks a colour between `shadows` and `highlights`
/// (sRGB 0-1), mixed in at `amount` 0-100.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct GradientMap {
    pub amount: f32,
    pub shadows: [f32; 3],
    pub highlights: [f32; 3],
}
impl Default for GradientMap {
    fn default() -> Self {
        Self { amount: 0.0, shadows: [0.0; 3], highlights: [1.0; 3] }
    }
}

/// Gaussian blur 0-100 (sigma up to 2% of the long edge), invert, posterize levels (0 = off, 2-255)
/// and threshold (0 = off, 1-255 luminance).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Filters {
    pub blur: f32,
    pub invert: bool,
    pub posterize: f32,
    pub threshold: f32,
}

/// Image rotation: `angle` 0 / 90 / 180 / 270 clockwise, then the mirrors. It turns the straightened
/// frame, so the crop rectangle is in the rotated frame.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct ImageRotation {
    pub angle: f32,
    pub flip_h: bool,
    pub flip_v: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Effects {
    pub vignette: f32,
    pub vignette_midpoint: f32,
    pub vignette_feather: f32,
}
impl Default for Effects {
    fn default() -> Self {
        Self { vignette: 0.0, vignette_midpoint: 50.0, vignette_feather: 50.0 }
    }
}

/// Crop rectangle (normalised, in the straightened frame) + straighten angle in degrees.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub angle: f32,
}
impl Default for Crop {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, w: 1.0, h: 1.0, angle: 0.0 }
    }
}
impl Crop {
    fn is_identity(&self) -> bool {
        *self == Crop::default()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Mask {
    pub mask_id: String,
    pub name: String,
    pub enabled: bool,
    pub components: Vec<MaskComponent>,
    pub adjustments: LocalAdjust,
    /// Full develop settings applied on top of the finished photo inside this mask only (the
    /// Develop panels while the mask is selected). Default settings change nothing; crop,
    /// lens, transform, calibration, effects, grain, removals and masks of its own are ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tuning: Option<Develop>,
}
impl Default for Mask {
    fn default() -> Self {
        Self {
            mask_id: String::new(),
            name: String::new(),
            enabled: true,
            components: vec![],
            adjustments: LocalAdjust::default(),
            tuning: None,
        }
    }
}

pub const MASK_TYPES: &[&str] = &["linear_gradient", "radial_gradient", "luminance_range", "color_range", "polygon"];
/// Vertex cap for AI outlines (rasterising costs vertices × rows).
const MAX_POINTS: usize = 512;

/// Object removal: the outlined area is refilled from the area at `source`, colour-matched
/// to its surroundings (a heal, not generative).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Heal {
    pub name: String,
    /// Outline [x, y] in normalised image coords.
    pub points: Vec<[f32; 2]>,
    /// Normalised offset from the outline to its source area.
    pub source: [f32; 2],
}

/// An AI outline must have 3+ vertices given as fractions of the frame.
pub fn check_outline(points: &[[f32; 2]]) -> Result<(), String> {
    if points.len() < 3 {
        return Err("an outline needs at least 3 [x, y] points".into());
    }
    if points.iter().flatten().any(|v| !(-0.05..=1.05).contains(v)) {
        return Err("outline points must be [x, y] fractions of the full frame (0-1), not pixels".into());
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct MaskComponent {
    /// add | subtract | intersect
    pub op: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// Tag of an AI region, e.g. "castle" (empty for drawn components).
    pub name: String,
    pub invert: bool,
    pub feather: f32,
    pub edge_shift: f32,
    /// linear_gradient: full effect at `start`, none at `end` (normalised image coords).
    pub start: [f32; 2],
    pub end: [f32; 2],
    /// radial_gradient: centre and radii (fractions of width / height).
    pub center: [f32; 2],
    pub radius: [f32; 2],
    /// luminance_range: [low, high] in 0–1 perceptual luminance.
    pub range: [f32; 2],
    /// color_range: target sRGB colour (0–1) and tolerance 0–100.
    pub color: [f32; 3],
    pub tolerance: f32,
    /// polygon: rough outline of an object (normalised image coords); the edge snaps to the photo.
    pub points: Vec<[f32; 2]>,
}
impl Default for MaskComponent {
    fn default() -> Self {
        Self {
            op: "add".into(),
            kind: "linear_gradient".into(),
            name: String::new(),
            invert: false,
            feather: 0.0,
            edge_shift: 0.0,
            start: [0.5, 0.0],
            end: [0.5, 0.5],
            center: [0.5, 0.5],
            radius: [0.3, 0.3],
            range: [0.5, 1.0],
            color: [0.5, 0.5, 0.5],
            tolerance: 50.0,
            points: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct LocalAdjust {
    pub exposure: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    pub temperature_shift: f32,
    pub tint_shift: f32,
    pub saturation: f32,
}

fn cl(v: &mut f32, lo: f32, hi: f32) {
    *v = v.clamp(lo, hi);
}

impl Develop {
    /// Clamp every parameter to its spec range (also validates AI/MCP output).
    pub fn clamp(&mut self) {
        cl(&mut self.white_balance.temperature_k, 2000.0, 50000.0);
        cl(&mut self.white_balance.tint, -150.0, 150.0);
        let t = &mut self.basic_tone;
        cl(&mut t.exposure, -5.0, 5.0);
        for v in [&mut t.contrast, &mut t.highlights, &mut t.shadows, &mut t.whites, &mut t.blacks] {
            cl(v, -100.0, 100.0);
        }
        let p = &mut self.presence;
        for v in [&mut p.texture, &mut p.clarity, &mut p.dehaze, &mut p.vibrance, &mut p.saturation] {
            cl(v, -100.0, 100.0);
        }
        let tc = &mut self.tone_curve;
        let pc = &mut tc.parametric;
        for v in [&mut pc.shadows, &mut pc.darks, &mut pc.lights, &mut pc.highlights] {
            cl(v, -100.0, 100.0);
        }
        for v in &mut pc.splits {
            cl(v, 5.0, 95.0);
        }
        pc.splits.sort_by(f32::total_cmp);
        for c in [&mut tc.point_luma, &mut tc.point_red, &mut tc.point_green, &mut tc.point_blue] {
            for p in c.iter_mut() {
                cl(&mut p[0], 0.0, 255.0);
                cl(&mut p[1], 0.0, 255.0);
            }
        }
        let m = &mut self.color_mixer;
        for v in m.hue.iter_mut().chain(&mut m.saturation).chain(&mut m.luminance) {
            cl(v, -100.0, 100.0);
        }
        self.point_color.truncate(8);
        for s in &mut self.point_color {
            s.src_hsl[0] = s.src_hsl[0].rem_euclid(360.0);
            cl(&mut s.src_hsl[1], 0.0, 100.0);
            cl(&mut s.src_hsl[2], 0.0, 100.0);
            for v in &mut s.shift_hsl {
                cl(v, -100.0, 100.0);
            }
            for v in &mut s.range_hsl {
                cl(v, 0.0, 100.0);
            }
            cl(&mut s.falloff, 0.0, 100.0);
        }
        let g = &mut self.color_grading_3way;
        for w in [&mut g.shadows, &mut g.midtones, &mut g.highlights, &mut g.global] {
            w.hue = w.hue.rem_euclid(360.0);
            cl(&mut w.sat, 0.0, 100.0);
            cl(&mut w.lum, -100.0, 100.0);
        }
        cl(&mut g.blending, 0.0, 100.0);
        cl(&mut g.balance, -100.0, 100.0);
        let d = &mut self.detail;
        cl(&mut d.sharpening, 0.0, 150.0);
        cl(&mut d.sharpen_radius, 0.5, 3.0);
        cl(&mut d.sharpen_masking, 0.0, 100.0);
        cl(&mut d.color_noise_reduction, 0.0, 100.0);
        cl(&mut d.luminance_noise_reduction, 0.0, 100.0);
        cl(&mut d.luminance_detail, 0.0, 100.0);
        let e = &mut self.effects;
        cl(&mut e.vignette, -100.0, 100.0);
        cl(&mut e.vignette_midpoint, 0.0, 100.0);
        cl(&mut e.vignette_feather, 0.0, 100.0);
        let c = &mut self.calibration;
        for v in [&mut c.shadows_tint, &mut c.red_hue, &mut c.red_saturation, &mut c.green_hue, &mut c.green_saturation, &mut c.blue_hue, &mut c.blue_saturation] {
            cl(v, -100.0, 100.0);
        }
        let g = &mut self.grain;
        for v in [&mut g.amount, &mut g.size, &mut g.roughness] {
            cl(v, 0.0, 100.0);
        }
        let l = &mut self.lens;
        for v in [&mut l.distortion, &mut l.ca_red_cyan, &mut l.ca_blue_yellow, &mut l.vignetting] {
            cl(v, -100.0, 100.0);
        }
        cl(&mut l.defringe_purple, 0.0, 20.0);
        cl(&mut l.defringe_green, 0.0, 20.0);
        cl(&mut l.vignetting_midpoint, 0.0, 100.0);
        for r in [&mut l.defringe_purple_hue, &mut l.defringe_green_hue] {
            for v in r.iter_mut() {
                cl(v, 0.0, 100.0);
            }
            r.sort_by(f32::total_cmp);
        }
        let t = &mut self.transform;
        for v in [&mut t.vertical, &mut t.horizontal, &mut t.aspect, &mut t.offset_x, &mut t.offset_y] {
            cl(v, -100.0, 100.0);
        }
        cl(&mut t.rotate, -10.0, 10.0);
        cl(&mut t.scale, 50.0, 150.0);
        t.guides.truncate(4);
        for v in t.guides.iter_mut().flatten().flatten() {
            cl(v, 0.0, 1.0);
        }
        let lv = &mut self.levels;
        for l in [&mut lv.rgb, &mut lv.red, &mut lv.green, &mut lv.blue] {
            cl(&mut l.black, 0.0, 253.0);
            cl(&mut l.white, l.black + 2.0, 255.0);
            cl(&mut l.gamma, 0.1, 9.99);
            cl(&mut l.out_black, 0.0, 255.0);
            cl(&mut l.out_white, 0.0, 255.0);
        }
        let hs = &mut self.hue_saturation;
        cl(&mut hs.hue, -180.0, 180.0);
        cl(&mut hs.saturation, -100.0, 100.0);
        cl(&mut hs.lightness, -100.0, 100.0);
        hs.colorize_hue = hs.colorize_hue.rem_euclid(360.0);
        cl(&mut hs.colorize_saturation, 0.0, 100.0);
        let s = &mut self.selective_color;
        for v in [&mut s.reds, &mut s.yellows, &mut s.greens, &mut s.cyans, &mut s.blues, &mut s.magentas, &mut s.whites, &mut s.neutrals, &mut s.blacks]
            .into_iter()
            .flatten()
        {
            cl(v, -100.0, 100.0);
        }
        let m = &mut self.channel_mixer;
        for v in [&mut m.red, &mut m.green, &mut m.blue, &mut m.gray].into_iter().flatten() {
            cl(v, -200.0, 200.0);
        }
        let g = &mut self.gradient_map;
        cl(&mut g.amount, 0.0, 100.0);
        for v in g.shadows.iter_mut().chain(&mut g.highlights) {
            cl(v, 0.0, 1.0);
        }
        let f = &mut self.filters;
        cl(&mut f.blur, 0.0, 100.0);
        cl(&mut f.posterize, 0.0, 255.0);
        cl(&mut f.threshold, 0.0, 255.0);
        let r = &mut self.image_rotation;
        r.angle = ((r.angle / 90.0).round() * 90.0).rem_euclid(360.0);
        let c = &mut self.crop;
        cl(&mut c.angle, -45.0, 45.0);
        cl(&mut c.w, 0.02, 1.0);
        cl(&mut c.h, 0.02, 1.0);
        cl(&mut c.x, 0.0, 1.0 - c.w);
        cl(&mut c.y, 0.0, 1.0 - c.h);
        let outline = |pts: &mut Vec<[f32; 2]>| {
            pts.truncate(MAX_POINTS);
            for v in pts.iter_mut().flatten() {
                cl(v, 0.0, 1.0);
            }
        };
        for r in &mut self.retouch {
            outline(&mut r.points);
            for v in &mut r.source {
                cl(v, -1.0, 1.0);
            }
        }
        // Each AI-selected polygon object is its own selection so it can be tuned independently.
        let mut masks = Vec::with_capacity(self.masks.len());
        for mut m in self.masks.drain(..) {
            if let Some(first) = m.components.first_mut()
                && first.kind == "polygon"
                && first.op == "subtract"
                && !m.name.starts_with("all but ")
            {
                first.op = "add".into();
            }
            let mut extra = vec![];
            if m.components.len() > 1 {
                let mut keep = Vec::with_capacity(m.components.len());
                for (ci, mut c) in m.components.into_iter().enumerate() {
                    if ci > 0 && c.kind == "polygon" && c.op == "add" {
                        c.op = "add".into();
                        extra.push(c);
                    } else {
                        keep.push(c);
                    }
                }
                m.components = keep;
            }
            let base_id = m.mask_id.clone();
            let enabled = m.enabled;
            masks.push(m);
            for (k, c) in extra.into_iter().enumerate() {
                let name = if c.name.is_empty() { format!("Object {}", masks.len() + 1) } else { c.name.clone() };
                masks.push(Mask {
                    mask_id: format!("{base_id}_{}", k + 1),
                    name,
                    enabled,
                    components: vec![c],
                    ..Mask::default()
                });
            }
        }
        self.masks = masks;
        for m in &mut self.masks {
            for c in &mut m.components {
                cl(&mut c.feather, -100.0, 100.0);
                cl(&mut c.edge_shift, -100.0, 100.0);
                cl(&mut c.tolerance, 0.0, 100.0);
                outline(&mut c.points);
            }
            let a = &mut m.adjustments;
            cl(&mut a.exposure, -5.0, 5.0);
            for v in [
                &mut a.contrast,
                &mut a.highlights,
                &mut a.shadows,
                &mut a.whites,
                &mut a.blacks,
                &mut a.temperature_shift,
                &mut a.tint_shift,
                &mut a.saturation,
            ] {
                cl(v, -100.0, 100.0);
            }
            if let Some(t) = &mut m.tuning {
                t.crop = Crop::default();
                t.effects = Effects::default();
                t.grain = Grain::default();
                t.lens = Lens::default();
                t.transform = Transform::default();
                t.image_rotation = ImageRotation::default();
                t.calibration = Calibration::default();
                t.retouch.clear();
                t.masks.clear();
                t.ai_provenance_log.clear();
                t.clamp();
            }
            if m.tuning.as_ref().is_some_and(|t| *t == Develop::default()) {
                m.tuning = None;
            }
        }
    }
}

/// Merge an MCP/AI parameter patch (spec §4.3 `parameters`) into a develop state.
/// Returns the new state plus the patch keys that do not exist in the edit graph.
pub fn apply_patch(cur: &Develop, patch: &Value, relative: bool) -> Result<(Develop, Vec<String>), String> {
    let mut base = serde_json::to_value(cur).map_err(es)?;
    let mut p = patch.clone();
    if let Some(o) = p.as_object_mut() {
        // MCP schema names -> edit-graph names
        for (from, to) in [("tone", "basic_tone"), ("point_color_swatches", "point_color"), ("hsl", "color_mixer")] {
            if let Some(v) = o.remove(from) {
                o.insert(to.into(), v);
            }
        }
    }
    let mut unknown = vec![];
    merge(&mut base, &p, relative, "", &mut unknown);
    let mut d: Develop = serde_json::from_value(base).map_err(|e| format!("invalid parameters: {e}"))?;
    d.clamp();
    Ok((d, unknown))
}

fn merge(base: &mut Value, patch: &Value, rel: bool, path: &str, unknown: &mut Vec<String>) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (k, v) in p {
                let key = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                match b.get_mut(k) {
                    Some(slot) => merge(slot, v, rel, &key, unknown),
                    None => unknown.push(key),
                }
            }
        }
        (b @ Value::Number(_), Value::Number(n)) if rel => {
            *b = json!(b.as_f64().unwrap_or(0.0) + n.as_f64().unwrap_or(0.0))
        }
        (b, p) => *b = p.clone(),
    }
}

/// Build a mask from MCP `mask_create_and_edit` operations.
pub fn mask_from_ops(name: &str, ops: &Value, adjustments: &Value) -> Result<(Mask, Vec<String>), String> {
    let mut components = vec![];
    for op in ops.as_array().ok_or("operations must be an array")? {
        let mut o = op.clone();
        if let Some(m) = o.as_object_mut() {
            for (from, to) in [("boolean_op", "op"), ("source_type", "type")] {
                if let Some(v) = m.remove(from) {
                    m.insert(to.into(), v);
                }
            }
        }
        let mut c: MaskComponent = serde_json::from_value(o).map_err(|e| format!("invalid operation: {e}"))?;
        if c.kind == "polygon" && c.name.is_empty() {
            c.name = name.into();
        }
        if !MASK_TYPES.contains(&c.kind.as_str()) {
            return Err(format!(
                "source_type '{}' needs the AI segmentation/depth models, which are not installed in this build. Available: {}",
                c.kind,
                MASK_TYPES.join(", ")
            ));
        }
        if !["add", "subtract", "intersect"].contains(&c.op.as_str()) {
            return Err(format!("boolean_op '{}' must be add, subtract or intersect", c.op));
        }
        if c.kind == "polygon" {
            check_outline(&c.points)?;
        }
        components.push(c);
    }
    let mut base = serde_json::to_value(LocalAdjust::default()).map_err(es)?;
    let mut unknown = vec![];
    merge(&mut base, adjustments, false, "local_adjustments", &mut unknown);
    let adjustments = serde_json::from_value(base).map_err(|e| format!("invalid local_adjustments: {e}"))?;
    let mask = Mask { name: name.into(), components, adjustments, ..Mask::default() };
    Ok((mask, unknown))
}

// ---------------------------------------------------------------------------
// Colour math (working space: linear Rec.2020)
// ---------------------------------------------------------------------------

pub(crate) const M709_2020: M3 = [[0.6274039, 0.3292830, 0.0433131], [0.0690973, 0.9195404, 0.0113623], [0.0163914, 0.0880133, 0.8955953]];
pub(crate) const M2020_709: M3 = [[1.6604910, -0.5876411, -0.0728499], [-0.1245505, 1.1328999, -0.0083494], [-0.0181508, -0.1005789, 1.1187297]];
const M2020_P3: M3 = [[1.3435783, -0.2821797, -0.0613986], [-0.0652975, 1.0757879, -0.0104905], [0.0028218, -0.0195985, 1.0167767]];
const M2020_ADOBE: M3 = [[1.1519784, -0.0975031, -0.0544753], [-0.1245505, 1.1328999, -0.0083494], [-0.0225304, -0.0498065, 1.0723369]];
const MXYZ_2020: M3 = [[1.7166513, -0.3556708, -0.2533663], [-0.6666843, 1.6164812, 0.0157686], [0.0176399, -0.0427706, 0.9421031]];
const M2020_LMS: M3 = [[0.6166884, 0.3601591, 0.0230433], [0.2651402, 0.6358565, 0.0990302], [0.1001506, 0.2040043, 0.6963247]];
const MLMS_2020: M3 = [[2.1401404, -1.2463560, 0.1064317], [-0.8848325, 2.1631727, -0.2783616], [-0.0485791, -0.4544909, 1.5023563]];
const IDENT: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[inline]
pub(crate) fn mul(m: &M3, p: Px) -> Px {
    [
        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2],
        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2],
        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2],
    ]
}
#[inline]
pub fn luma(p: Px) -> f32 {
    0.2627 * p[0] + 0.6780 * p[1] + 0.0593 * p[2]
}
#[inline]
pub(crate) fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
#[inline]
fn lerp(a: Px, b: Px, t: f32) -> Px {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
pub fn srgb_eotf(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
pub fn srgb_oetf(l: f32) -> f32 {
    if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}
fn pq(l: f32) -> f32 {
    // SMPTE ST 2084 with SDR white (1.0) at 203 nits
    let y = (l * 203.0 / 10000.0).clamp(0.0, 1.0);
    let p = y.powf(0.159_301_76);
    ((0.835_937_5 + 18.851_563 * p) / (1.0 + 18.6875 * p)).powf(78.84375)
}
#[inline]
pub(crate) fn oklab(p: Px) -> Px {
    let l = mul(&M2020_LMS, p).map(f32::cbrt);
    [
        0.2104542553 * l[0] + 0.7936177850 * l[1] - 0.0040720468 * l[2],
        1.9779984951 * l[0] - 2.4285922050 * l[1] + 0.4505937099 * l[2],
        0.0259040371 * l[0] + 0.7827717662 * l[1] - 0.8086757660 * l[2],
    ]
}
#[inline]
pub(crate) fn from_oklab(c: Px) -> Px {
    let l = c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2];
    let m = c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2];
    let s = c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2];
    mul(&MLMS_2020, [l * l * l, m * m * m, s * s * s])
}
/// sRGB-encoded (0–1) colour -> linear Rec.2020.
pub(crate) fn srgb_to_working(c: Px) -> Px {
    mul(&M709_2020, c.map(srgb_eotf))
}
/// Fully saturated sRGB colour for an HSL hue in degrees (matches the UI colour wheels).
fn hue_rgb(h: f32) -> Px {
    let f = |n: f32| {
        let k = (n + h / 30.0).rem_euclid(12.0);
        0.5 - 0.5 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    [f(0.0), f(8.0), f(4.0)]
}

/// Planckian locus white (Kim et al. cubic fit) in linear Rec.2020.
fn kelvin_rgb(t: f32) -> Px {
    let t = t.clamp(1667.0, 25000.0) as f64;
    let (t2, t3) = (t * t, t * t * t);
    let x = if t <= 4000.0 {
        -0.2661239e9 / t3 - 0.2343589e6 / t2 + 0.8776956e3 / t + 0.179910
    } else {
        -3.0258469e9 / t3 + 2.1070379e6 / t2 + 0.2226347e3 / t + 0.240390
    };
    let (x2, x3) = (x * x, x * x * x);
    let y = if t <= 2222.0 {
        -1.1063814 * x3 - 1.34811020 * x2 + 2.18555832 * x - 0.20219683
    } else if t <= 4000.0 {
        -0.9549476 * x3 - 1.37418593 * x2 + 2.09137015 * x - 0.16748867
    } else {
        3.0817580 * x3 - 5.87338670 * x2 + 3.75112997 * x - 0.37001483
    };
    mul(&MXYZ_2020, [(x / y) as f32, 1.0, ((1.0 - x - y) / y) as f32])
}

/// Per-channel gains for a temperature/tint relative to the 5500K reference.
fn wb_gains(temperature_k: f32, tint: f32) -> Px {
    let (r, t) = (kelvin_rgb(5500.0), kelvin_rgb(temperature_k));
    let mut g = [r[0] / t[0], r[1] / t[1], r[2] / t[2]];
    g[1] *= (-tint / 150.0 * 0.4).exp2(); // + = magenta
    let n = luma(g);
    g.map(|v| v / n)
}

// ---------------------------------------------------------------------------
// Image I/O
// ---------------------------------------------------------------------------

/// Working image: linear Rec.2020, row-major.
#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Px>,
}

pub const RAW_EXTS: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "srw", "3fr", "iiq",
    "erf", "mef", "mos", "kdc", "dcr", "rwl",
];
pub const IMG_EXTS: &[&str] = &["jpg", "jpeg", "png", "tif", "tiff", "webp"];

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}
pub fn is_raw(path: &Path) -> bool {
    RAW_EXTS.contains(&ext(path).as_str())
}
pub fn is_supported(path: &Path) -> bool {
    let e = ext(path);
    RAW_EXTS.contains(&e.as_str()) || IMG_EXTS.contains(&e.as_str())
}

/// Float images (32-bit TIFF, the Enhanced masters, and decoded RAWs) hold linear light.
pub fn is_float(img: &DynamicImage) -> bool {
    matches!(img, DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_))
}

/// What a decoded file looks like on screen: float (linear) images get the sRGB curve.
pub fn displayable(img: DynamicImage) -> DynamicImage {
    if !is_float(&img) {
        return img;
    }
    let buf = img.to_rgb32f();
    let rgb = buf.as_raw().par_iter().map(|&v| oetf8(v)).collect();
    DynamicImage::ImageRgb8(ImageBuffer::from_raw(buf.width(), buf.height(), rgb).expect("rgb8 buffer"))
}

/// Decode any supported file into the working space, optionally downscaled.
pub fn load(path: &Path, max_edge: Option<u32>) -> Result<Img, String> {
    let img = if is_raw(path) { decode_raw(path)? } else { decode_std(path)? };
    let linear = is_float(&img);
    let img = match max_edge {
        Some(m) if img.width().max(img.height()) > m => img.resize(m, m, FilterType::Triangle),
        _ => img,
    };
    let buf = img.to_rgb32f();
    let (w, h) = (buf.width() as usize, buf.height() as usize);
    let px = buf
        .as_raw()
        .par_chunks_exact(3)
        .map(|c| if linear { mul(&M709_2020, [c[0], c[1], c[2]]) } else { srgb_to_working([c[0], c[1], c[2]]) })
        .collect();
    Ok(Img { w, h, px })
}

/// JPEG/PNG/TIFF/WebP with EXIF orientation applied.
/// ponytail: embedded ICC profiles are ignored (files are read as sRGB); add moxcms
/// transforms when wide-gamut JPEGs (e.g. Display P3 phone shots) need to be exact.
pub fn decode_std(path: &Path) -> Result<DynamicImage, String> {
    let mut dec =
        image::ImageReader::open(path).map_err(es)?.with_guessed_format().map_err(es)?.into_decoder().map_err(es)?;
    let o = dec.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(dec).map_err(es)?;
    img.apply_orientation(o);
    Ok(img)
}

/// Full RAW develop via rawler (demosaic, as-shot WB, camera matrix) -> linear sRGB.
fn decode_raw(path: &Path) -> Result<DynamicImage, String> {
    use rawler::imgop::develop::{Intermediate, ProcessingStep::*, RawDevelop};
    let path = path.to_path_buf();
    // rawler still has todo!()/panic!() paths for exotic sensors
    std::panic::catch_unwind(move || {
        let raw = rawler::decode_file(&path).map_err(es)?;
        let dev =
            RawDevelop::new_with(&[Rescale, Demosaic, FujiRotate, CropActiveArea, WhiteBalance, Calibrate, CropDefault]);
        let (w, h, data): (usize, usize, Vec<f32>) = match dev.develop_intermediate(&raw).map_err(es)? {
            Intermediate::ThreeColor(p) => (p.width, p.height, p.data.into_iter().flatten().collect()),
            Intermediate::Monochrome(p) => (p.width, p.height, p.data.iter().flat_map(|&v| [v, v, v]).collect()),
            Intermediate::FourColor(_) => return Err("unsupported 4-colour sensor".to_string()),
        };
        let buf = ImageBuffer::from_raw(w as u32, h as u32, data).ok_or_else(|| "bad RAW buffer".to_string())?;
        let mut img = DynamicImage::ImageRgb32F(buf);
        if let Some(o) = Orientation::from_exif(raw_orientation(&raw.orientation)) {
            img.apply_orientation(o);
        }
        Ok(img)
    })
    .unwrap_or_else(|_| Err("RAW decoder crashed on this file (unsupported camera?)".into()))
}

fn raw_orientation(o: &rawler::Orientation) -> u8 {
    use rawler::Orientation::*;
    match o {
        Normal | Unknown => 1,
        HorizontalFlip => 2,
        Rotate180 => 3,
        VerticalFlip => 4,
        Transpose => 5,
        Rotate90 => 6,
        Transverse => 7,
        Rotate270 => 8,
    }
}

/// Camera-rendered preview embedded in a RAW file (fast thumbnails on import).
pub fn raw_preview(path: &Path) -> Option<DynamicImage> {
    let path = path.to_path_buf();
    std::panic::catch_unwind(move || {
        let src = rawler::rawsource::RawSource::new(&path).ok()?;
        let dec = rawler::get_decoder(&src).ok()?;
        let p = rawler::decoders::RawDecodeParams::default();
        dec.preview_image(&src, &p).ok().flatten().or_else(|| dec.thumbnail_image(&src, &p).ok().flatten())
    })
    .ok()
    .flatten()
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
pub struct Opts {
    /// Crop tool active: straighten but keep the full frame.
    pub skip_crop: bool,
    /// No geometry (distortion, transform, straighten, crop): the frame mask and retouch coordinates refer to.
    pub full_frame: bool,
    /// Tint this mask's coverage red.
    pub overlay_mask: Option<usize>,
}

pub fn render(src: &Img, d: &Develop, o: &Opts) -> Img {
    let (w, h) = (src.w, src.h);
    let (mut px, wts) = tones(&src.px, w, h, d, o.overlay_mask);
    // 8. mask tuning: the mask's own settings on top of the result, inside the mask only
    for (m, wt) in d.masks.iter().zip(&wts) {
        if let (true, Some(t), Some(wt)) = (m.enabled, &m.tuning, wt) {
            let (tp, _) = tones(&px, w, h, t, None);
            px.par_iter_mut().zip(tp).zip(wt).for_each(|((p, q), &a)| *p = lerp(*p, q, a));
        }
    }
    if let Some(m) = o.overlay_mask.and_then(|i| wts.get(i)).and_then(Option::as_ref) {
        px.par_iter_mut().zip(m).for_each(|(p, &m)| *p = lerp(p.map(|c| c.clamp(0.0, 1.0)), [0.9, 0.05, 0.05], m * 0.6));
    }
    // 9. geometry (lens distortion, transform, straighten, crop), then post-crop effects
    let mut img = if o.full_frame { Img { w, h, px } } else { geometry(Img { w, h, px }, d, !o.skip_crop) };
    vignette(&mut img, &d.effects);
    grain(&mut img, &d.grain);
    img
}

/// Stages 1–7 (everything before geometry), plus each mask's coverage (None for a disabled
/// mask that is not the overlay).
fn tones(src: &[Px], w: usize, h: usize, d: &Develop, overlay: Option<usize>) -> (Vec<Px>, Vec<Option<Vec<f32>>>) {
    // 1. white balance + exposure
    let wb = wb_gains(d.white_balance.temperature_k, d.white_balance.tint);
    let g = wb.map(|v| v * d.basic_tone.exposure.exp2());
    let mut px: Vec<Px> = src.par_iter().map(|p| [p[0] * g[0], p[1] * g[1], p[2] * g[2]]).collect();
    // 1a. lens corrections on the full frame, camera calibration
    lens(&mut px, w, h, &d.lens);
    calibrate(&mut px, &d.calibration);
    // 1b. object removal
    for r in &d.retouch {
        heal(&mut px, w, h, r);
    }
    // 2. dehaze
    if d.presence.dehaze != 0.0 {
        dehaze(&mut px, w, h, d.presence.dehaze / 100.0);
    }
    // 3. global tone
    let tone = Tone::new(&d.basic_tone);
    if !tone.is_identity() {
        px.par_iter_mut().for_each(|p| *p = tone.apply(*p));
    }
    // 4. presence: clarity / texture
    if d.presence.clarity != 0.0 || d.presence.texture != 0.0 {
        local_contrast(&mut px, w, h, d.presence.clarity / 100.0, d.presence.texture / 100.0);
    }
    // 5. local masks
    let wts = apply_masks(&mut px, w, h, &d.masks, overlay);
    // 6. curves + colour
    let curves = Curves::new(&d.tone_curve);
    let color = ColorOps::new(d);
    if curves.is_some() || color.active {
        px.par_iter_mut().for_each(|p| {
            if let Some(c) = &curves {
                *p = c.apply(*p);
            }
            if color.active {
                *p = color.apply(*p);
            }
        });
    }
    // 7. detail
    if d.detail.luminance_noise_reduction > 0.0 {
        luma_nr(&mut px, w, h, &d.detail);
    }
    if d.detail.color_noise_reduction > 0.0 {
        color_nr(&mut px, w, h, d.detail.color_noise_reduction / 100.0);
    }
    if d.detail.sharpening > 0.0 {
        sharpen(&mut px, w, h, &d.detail);
    }
    // 7b. Pro: Photoshop adjustments on the result (inside the mask, for a mask's tuning)
    let pro = Pro::new(d);
    if d.filters.blur > 0.0 {
        let s = d.filters.blur / 100.0 * 0.02 * w.max(h) as f32;
        let ch: [Vec<f32>; 3] = std::array::from_fn(|c| blur(&px.iter().map(|p| p[c]).collect::<Vec<_>>(), w, h, s));
        px.par_iter_mut().enumerate().for_each(|(i, p)| *p = [ch[0][i], ch[1][i], ch[2][i]]);
    }
    if pro.active {
        px.par_iter_mut().for_each(|p| *p = pro.apply(*p));
    }
    (px, wts)
}

/// Click marker for the AI: a red ring with a white rim around `at` (normalised).
pub fn mark(img: &mut Img, at: [f32; 2]) {
    let (cx, cy, r) = (at[0] * img.w as f32, at[1] * img.h as f32, 0.015 * img.w.max(img.h) as f32);
    let span = |c: f32, n: usize| (c - r - 4.0).max(0.0) as usize..((c + r + 4.0).max(0.0) as usize).min(n);
    for y in span(cy, img.h) {
        for x in span(cx, img.w) {
            let d = ((x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) - r).abs();
            if d < 3.5 {
                img.px[y * img.w + x] = if d < 2.0 { [1.0, 0.0, 0.0] } else { [1.0; 3] };
            }
        }
    }
}

struct Tone {
    a_sh: f32,
    a_hi: f32,
    bp: f32,
    wp: f32,
    k: f32,
}
impl Tone {
    fn new(t: &BasicTone) -> Self {
        Tone {
            a_sh: 1.8 * t.shadows / 100.0,
            a_hi: 1.2 * t.highlights / 100.0,
            bp: -0.15 * t.blacks / 100.0,
            wp: 1.0 - 0.15 * t.whites / 100.0,
            k: t.contrast / 100.0,
        }
    }
    fn is_identity(&self) -> bool {
        self.a_sh == 0.0 && self.a_hi == 0.0 && self.bp == 0.0 && self.wp == 1.0 && self.k == 0.0
    }
    /// Shadows/highlights are hue-preserving gains keyed on max(R,G,B) (monotone for the
    /// slider ranges used here); whites/blacks/contrast act per channel in a sqrt domain.
    /// ponytail: global curves; Lightroom's edge-aware local tone mapping would need a
    /// bilateral/local-Laplacian base layer.
    #[inline]
    fn apply(&self, mut p: Px) -> Px {
        if self.a_sh != 0.0 || self.a_hi != 0.0 {
            let v = p[0].max(p[1]).max(p[2]).max(0.0).sqrt();
            let ws = 1.0 - smooth(0.0, 0.5, v);
            let wh = smooth(0.35, 1.0, v);
            let g = (self.a_sh * ws + self.a_hi * wh).exp2();
            p = p.map(|c| c * g);
        }
        p.map(|c| {
            let mut v = (c.max(0.0).sqrt() - self.bp) / (self.wp - self.bp);
            if v > 0.0 && v < 1.0 {
                v += self.k * (v * v * (3.0 - 2.0 * v) - v);
            }
            let v = v.max(0.0);
            v * v
        })
    }
}

/// Separable 3-pass box blur ≈ Gaussian.
pub fn blur(src: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let r = ((sigma * 0.9).round() as usize).max(1);
    let mut a = src.to_vec();
    for _ in 0..3 {
        a = box_rows(&a, w, r);
    }
    let mut t = transpose(&a, w, h);
    for _ in 0..3 {
        t = box_rows(&t, h, r);
    }
    transpose(&t, h, w)
}

fn box_rows(src: &[f32], w: usize, r: usize) -> Vec<f32> {
    let mut out = vec![0.0; src.len()];
    out.par_chunks_mut(w).zip(src.par_chunks(w)).for_each(|(o, s)| {
        let last = s.len() - 1;
        let inv = 1.0 / (2 * r + 1) as f32;
        let mut acc = s[0] * r as f32;
        for k in 0..=r {
            acc += s[k.min(last)];
        }
        for i in 0..s.len() {
            o[i] = acc * inv;
            acc += s[(i + r + 1).min(last)] - s[i.saturating_sub(r)];
        }
    });
    out
}

pub(crate) fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0; src.len()];
    out.par_chunks_mut(h).enumerate().for_each(|(x, col)| {
        for (y, v) in col.iter_mut().enumerate() {
            *v = src[y * w + x];
        }
    });
    out
}

fn local_contrast(px: &mut [Px], w: usize, h: usize, clarity: f32, texture: f32) {
    let s = w.max(h) as f32;
    let v: Vec<f32> = px.par_iter().map(|p| luma(*p).max(0.0).sqrt()).collect();
    let bc = (clarity != 0.0).then(|| blur(&v, w, h, (s * 0.01).max(2.0)));
    let bt = (texture != 0.0).then(|| blur(&v, w, h, (s * 0.0015).max(1.0)));
    px.par_iter_mut().enumerate().for_each(|(i, p)| {
        let v0 = v[i];
        if v0 < 1e-4 {
            return;
        }
        let mut d = 0.0;
        if let Some(b) = &bc {
            d += clarity * 0.8 * (v0 - b[i]) * (4.0 * v0 * (1.0 - v0)).clamp(0.0, 1.0);
        }
        if let Some(b) = &bt {
            d += texture * (v0 - b[i]);
        }
        let r = ((v0 + d).max(0.0) / v0).powi(2).min(8.0);
        *p = p.map(|c| c * r);
    });
}

/// Dark-channel-prior dehaze. ponytail: blurred dark channel instead of min-filter +
/// guided-filter refinement; good enough at proxy scale, may halo on hard skylines.
fn dehaze(px: &mut [Px], w: usize, h: usize, amount: f32) {
    let dc: Vec<f32> = px.par_iter().map(|p| p[0].min(p[1]).min(p[2]).max(0.0)).collect();
    let dcb = blur(&dc, w, h, w.max(h) as f32 * 0.02);
    let mut idx: Vec<usize> = (0..dcb.len()).step_by(7).collect();
    idx.sort_by(|a, b| dcb[*b].total_cmp(&dcb[*a]));
    let top = &idx[..(idx.len() / 200).max(1)];
    let mut a = [0.0f32; 3];
    for &i in top {
        for c in 0..3 {
            a[c] += px[i][c] / top.len() as f32;
        }
    }
    let a = a.map(|v| v.max(1e-3));
    let amax = a[0].max(a[1]).max(a[2]);
    px.par_iter_mut().zip(&dcb).for_each(|(p, &d)| {
        let haze = (d / amax).min(1.0);
        if amount > 0.0 {
            let t = (1.0 - 0.9 * amount * haze).max(0.15);
            *p = [0, 1, 2].map(|c| ((p[c] - a[c]) / t + a[c]).max(0.0));
        } else {
            *p = lerp(*p, a, -amount * 0.7 * (0.3 + 0.7 * haze));
        }
    });
}

/// Pixel rectangle inside an image.
#[derive(Clone, Copy)]
struct Win {
    x0: usize,
    y0: usize,
    w: usize,
    h: usize,
}

/// Pixel bounding box (min, max) of normalised points.
fn bounds(points: &[[f32; 2]], w: usize, h: usize) -> ([f32; 2], [f32; 2]) {
    points.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| {
        let q = [p[0] * w as f32, p[1] * h as f32];
        ([lo[0].min(q[0]), lo[1].min(q[1])], [hi[0].max(q[0]), hi[1].max(q[1])])
    })
}

/// Bounding box of normalised points grown by `pad` pixels, clipped to the image (may be empty).
fn win(points: &[[f32; 2]], w: usize, h: usize, pad: f32) -> Win {
    let (lo, hi) = bounds(points, w, h);
    let c = |v: f32, n: usize| v.max(0.0).min(n as f32) as usize;
    let (x0, y0) = (c((lo[0] - pad).floor(), w), c((lo[1] - pad).floor(), h));
    let (x1, y1) = (c((hi[0] + pad).ceil(), w), c((hi[1] + pad).ceil(), h));
    Win { x0, y0, w: x1.saturating_sub(x0), h: y1.saturating_sub(y0) }
}

/// Even-odd fill of a normalised polygon at pixel centres, over window `r` of a w×h image.
fn fill(points: &[[f32; 2]], w: usize, h: usize, r: Win) -> Vec<f32> {
    let mut out = vec![0.0; r.w * r.h];
    if points.len() < 3 || out.is_empty() {
        return out;
    }
    let (fw, fh) = (w as f32, h as f32);
    out.par_chunks_mut(r.w).enumerate().for_each(|(j, row)| {
        let y = (r.y0 + j) as f32 + 0.5;
        let mut xs: Vec<f32> = vec![];
        for (k, a) in points.iter().enumerate() {
            let b = points[(k + 1) % points.len()];
            let (ay, by) = (a[1] * fh, b[1] * fh);
            if (ay <= y) != (by <= y) {
                xs.push((a[0] + (y - ay) / (by - ay) * (b[0] - a[0])) * fw);
            }
        }
        xs.sort_by(f32::total_cmp);
        let col = |x: f32| ((x - 0.5).ceil().max(r.x0 as f32) as usize).min(r.x0 + r.w) - r.x0;
        for s in xs.chunks_exact(2) {
            row[col(s[0])..col(s[1])].fill(1.0);
        }
    });
    out
}

/// Grey-guided filter (He et al.): smooths `p` except across edges in `guide`.
fn guided(guide: &[f32], p: &[f32], w: usize, h: usize, r: f32, eps: f32) -> Vec<f32> {
    let prod = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).collect::<Vec<f32>>();
    let (mi, mp) = (blur(guide, w, h, r), blur(p, w, h, r));
    let (mii, mip) = (blur(&prod(guide, guide), w, h, r), blur(&prod(guide, p), w, h, r));
    let a: Vec<f32> = (0..p.len()).map(|i| (mip[i] - mi[i] * mp[i]) / (mii[i] - mi[i] * mi[i] + eps)).collect();
    let b: Vec<f32> = (0..p.len()).map(|i| mp[i] - a[i] * mi[i]).collect();
    let (ma, mb) = (blur(&a, w, h, r), blur(&b, w, h, r));
    (0..p.len()).map(|i| (ma[i] * guide[i] + mb[i]).clamp(0.0, 1.0)).collect()
}

/// AI object outline -> soft matte. The outline is a prior: within a band around it each pixel
/// goes to whichever side's colours it resembles (Oklab histograms of the confidently inside /
/// outside pixels), then a guided filter snaps the edge to the photo and a contrast curve
/// (width set by feather) drops the faint spill.
/// Outside pixels (`raw < 0.5`) are clamped by a 1px anti-aliased edge (`0.48 * b_fine`) so
/// U-shaped concavities (e.g. a foreground person's head/body carved out of a background castle)
/// never flood across the 0.5 boundary into the background selection.
fn object_matte(points: &[[f32; 2]], px: &[Px], w: usize, h: usize, soft: f32, shift: f32) -> (Win, Vec<f32>) {
    let s = w.max(h) as f32;
    let (lo, hi) = bounds(points, w, h);
    let size = (hi[0] - lo[0]).min(hi[1] - lo[1]).max(1.0);
    let band = (0.08 * size).min(0.008 * s).max(1.0);
    let r = ((0.001 + 0.004 * soft) * s).min(0.1 * size).max(1.0);
    let k = 0.05 + 0.45 * soft;
    let mid = (0.5 - shift / 250.0).clamp(0.1, 0.9);
    let win = win(points, w, h, 3.0 * (band + r));
    let n = win.w * win.h;
    if n == 0 {
        return (win, vec![]);
    }
    let at = |k: usize| px[(win.y0 + k / win.w) * w + win.x0 + k % win.w];
    let raw = fill(points, w, h, win);
    let b_band = blur(&raw, win.w, win.h, band);
    let b_fine = blur(&raw, win.w, win.h, 1.0);
    let prior: Vec<f32> = (0..n)
        .map(|k| {
            if raw[k] >= 0.5 {
                b_band[k].max(0.52 * b_fine[k])
            } else {
                b_band[k].min(0.48 * b_fine[k])
            }
        })
        .collect();
    let bin = |p: Px| {
        let q = oklab(p);
        let f = |v: f32, lo: f32, hi: f32| (((v - lo) / (hi - lo) * 16.0) as usize).min(15);
        f(q[0], 0.0, 1.0) * 256 + f(q[1], -0.25, 0.25) * 16 + f(q[2], -0.25, 0.25)
    };
    let (mut hin, mut hout) = (vec![1f32; 4096], vec![1f32; 4096]);
    for (k, &p) in b_band.iter().enumerate() {
        if p > 0.85 {
            hin[bin(at(k))] += 1.0;
        } else if p < 0.15 {
            hout[bin(at(k))] += 1.0;
        }
    }
    let (nin, nout): (f32, f32) = (hin.iter().sum(), hout.iter().sum());
    let post: Vec<f32> = prior
        .par_iter()
        .enumerate()
        .map(|(k, &p)| {
            if p <= 0.001 || p >= 0.999 {
                return p;
            }
            let b = bin(at(k));
            let (i, o) = (p * hin[b] / nin, (1.0 - p) * hout[b] / nout);
            i / (i + o)
        })
        .collect();
    let guide: Vec<f32> = (0..n).map(|k| luma(at(k)).max(0.0).sqrt()).collect();
    (win, guided(&guide, &post, win.w, win.h, r, 1e-3).into_iter().map(|v| smooth(mid - k, mid + k, v)).collect())
}

/// Heal sizes for an outline in pixels: dilation/feather sigma and colour-correction sigma.
fn heal_sizes(points: &[[f32; 2]], w: usize, h: usize) -> (f32, f32) {
    let (lo, hi) = bounds(points, w, h);
    let m = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0);
    (1.0 + 0.05 * m, 0.5 * m)
}

/// Convex hull (Andrew's monotone chain).
fn hull(mut p: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut h: Vec<[f32; 2]> = vec![];
    for &q in &p {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
            h.pop();
        }
        h.push(q);
    }
    let t = h.len() + 1;
    for &q in p.iter().rev().skip(1) {
        while h.len() >= t && cross(h[h.len() - 2], h[h.len() - 1], q) <= 0.0 {
            h.pop();
        }
        h.push(q);
    }
    h.pop();
    h
}

/// Outlines whose heal areas touch become one heal over their convex hull, so each fill is
/// matched against clean surroundings rather than the neighbouring object.
pub fn group_outlines(objs: Vec<(String, Vec<[f32; 2]>)>, w: usize, h: usize) -> Vec<(String, Vec<[f32; 2]>)> {
    let area = |p: &[[f32; 2]]| {
        let ((lo, hi), g) = (bounds(p, w, h), 3.0 * heal_sizes(p, w, h).0);
        [lo[0] - g, lo[1] - g, hi[0] + g, hi[1] + g]
    };
    let touch = |a: [f32; 4], b: [f32; 4]| a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
    let mut out: Vec<(String, Vec<[f32; 2]>, bool)> = vec![];
    for (mut name, mut pts) in objs {
        let mut merged = false;
        while let Some(i) = out.iter().position(|o| touch(area(&o.1), area(&pts))) {
            let o = out.remove(i);
            name = format!("{} + {name}", o.0);
            pts.extend(o.1);
            merged = true;
        }
        out.push((name, pts, merged));
    }
    out.into_iter().map(|(n, p, merged)| (n, if merged { hull(p) } else { p })).collect()
}

/// Heal coverage: the outline grown by ~σ with a soft edge.
fn heal_alpha(points: &[[f32; 2]], w: usize, h: usize, r: Win, sa: f32) -> Vec<f32> {
    blur(&fill(points, w, h, r), r.w, r.h, sa).into_iter().map(|v| (2.0 * v).min(1.0)).collect()
}

/// Replace the outlined area with the source area plus a smooth correction that matches the
/// surroundings (normalised convolution of the boundary difference).
fn heal(px: &mut [Px], w: usize, h: usize, hl: &Heal) {
    if hl.points.len() < 3 {
        return;
    }
    let (sa, sc) = heal_sizes(&hl.points, w, h);
    let r = win(&hl.points, w, h, 3.0 * (sa + sc));
    let n = r.w * r.h;
    if n == 0 {
        return;
    }
    let alpha = heal_alpha(&hl.points, w, h, r, sa);
    let (dx, dy) = ((hl.source[0] * w as f32).round() as isize, (hl.source[1] * h as f32).round() as isize);
    let idx = |k: usize| (r.y0 + k / r.w) * w + r.x0 + k % r.w;
    let (mut src, mut wt) = (vec![[0.0; 3]; n], vec![0.0; n]);
    for k in 0..n {
        let (x, y) = ((r.x0 + k % r.w) as isize + dx, (r.y0 + k / r.w) as isize + dy);
        let (sx, sy) = (x.clamp(0, w as isize - 1), y.clamp(0, h as isize - 1));
        src[k] = px[sy as usize * w + sx as usize];
        if (sx, sy) != (x, y) {
            continue;
        }
        // a source pixel inside the outline itself is no colour reference
        let (lx, ly) = (sx - r.x0 as isize, sy - r.y0 as isize);
        let a = if lx >= 0 && ly >= 0 && (lx as usize) < r.w && (ly as usize) < r.h { alpha[ly as usize * r.w + lx as usize] } else { 0.0 };
        wt[k] = (1.0 - alpha[k]) * (1.0 - a);
    }
    let den = blur(&wt, r.w, r.h, sc);
    let corr: Vec<Vec<f32>> = (0..3)
        .map(|c| blur(&(0..n).map(|k| (px[idx(k)][c] - src[k][c]) * wt[k]).collect::<Vec<f32>>(), r.w, r.h, sc))
        .collect();
    for k in 0..n {
        if alpha[k] > 0.0 {
            let f = if den[k] > 1e-4 { [0, 1, 2].map(|c| (src[k][c] + corr[c][k] / den[k]).max(0.0)) } else { src[k] };
            let i = idx(k);
            px[i] = lerp(px[i], f, alpha[k]);
        }
    }
}

/// Source offset (normalised) for healing `points`: the nearby area whose surroundings best match
/// the outline's surroundings and that stays clear of the outline and of `avoid`. None if none fits.
pub fn heal_source(img: &Img, points: &[[f32; 2]], avoid: &[&[[f32; 2]]]) -> Option<[f32; 2]> {
    let (w, h) = (img.w, img.h);
    let (sa, _) = heal_sizes(points, w, h);
    let r = win(points, w, h, 9.0 * sa);
    if r.w * r.h == 0 {
        return None;
    }
    let alpha = heal_alpha(points, w, h, r, sa);
    let near = blur(&fill(points, w, h, r), r.w, r.h, 3.0 * sa);
    let pick = |f: &dyn Fn(usize) -> bool| -> Vec<(isize, isize)> {
        let ks: Vec<usize> = (0..alpha.len()).filter(|&k| f(k)).collect();
        let step = (ks.len() / 1500).max(1);
        ks.iter().step_by(step).map(|&k| ((r.x0 + k % r.w) as isize, (r.y0 + k / r.w) as isize)).collect()
    };
    let ring = pick(&|k| alpha[k] < 0.01 && near[k] > 0.01);
    let v = |x: isize, y: isize| img.px[y.clamp(0, h as isize - 1) as usize * w + x.clamp(0, w as isize - 1) as usize].map(|c| c.max(0.0).sqrt());
    let d2 = |a: Px, b: Px| (0..3).map(|c| (a[c] - b[c]).powi(2)).sum::<f32>();
    // area that gets replaced, as a box; the source box must not overlap it or another outline
    let boxed = |p: &[[f32; 2]]| {
        let (lo, hi) = bounds(p, w, h);
        let g = 3.0 * sa;
        [lo[0] - g, lo[1] - g, hi[0] + g, hi[1] + g]
    };
    let dst = boxed(points);
    let others: Vec<[f32; 4]> = avoid.iter().map(|p| boxed(p)).collect();
    let m = (dst[2] - dst[0]).max(dst[3] - dst[1]);
    let valid = |dx: isize, dy: isize| {
        let b = [dst[0] + dx as f32, dst[1] + dy as f32, dst[2] + dx as f32, dst[3] + dy as f32];
        let apart = |o: &[f32; 4]| b[2] <= o[0] || o[2] <= b[0] || b[3] <= o[1] || o[3] <= b[1];
        b[0] >= 0.0 && b[1] >= 0.0 && b[2] <= w as f32 && b[3] <= h as f32 && apart(&dst) && others.iter().all(apart)
    };
    // outlined pixels are no reference on either side: they are about to be healed
    let outlined = |x: isize, y: isize, bs: &[[f32; 4]]| {
        let (x, y) = (x as f32, y as f32);
        bs.iter().any(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3])
    };
    let ring: Vec<(isize, isize)> = ring.into_iter().filter(|&(x, y)| !outlined(x, y, &others)).collect();
    let all: Vec<[f32; 4]> = others.iter().copied().chain([dst]).collect();
    // ponytail: only the surroundings are compared, so an unlisted object inside the source area gets copied
    let score = |(dx, dy): (isize, isize)| {
        let (sum, n) = ring
            .iter()
            .filter(|&&(x, y)| !outlined(x + dx, y + dy, &all))
            .fold((0.0, 0), |(s, n), &(x, y)| (s + d2(v(x, y), v(x + dx, y + dy)), n + 1));
        if n == 0 || n * 2 < ring.len() {
            return f32::INFINITY;
        }
        sum / n as f32 * (1.0 + 0.1 * (dx as f32).hypot(dy as f32) / m)
    };
    let best = |c: Vec<(isize, isize)>| {
        c.into_par_iter()
            .filter(|&(dx, dy)| valid(dx, dy))
            .map(|d| (score(d), d))
            .filter(|s| s.0.is_finite())
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|b| b.1)
    };
    let grid = |c: (isize, isize), rad: isize, step: usize| -> Vec<(isize, isize)> {
        (-rad..=rad).step_by(step).flat_map(|y| (-rad..=rad).step_by(step).map(move |x| (c.0 + x, c.1 + y))).collect()
    };
    let step = (m / 8.0).max(1.0) as usize;
    let rad = (3.0 * m).max(0.03 * w.max(h) as f32) as isize;
    let coarse = best(grid((0, 0), rad, step))?;
    let (dx, dy) = best(grid(coarse, step as isize, 1)).unwrap_or(coarse);
    Some([dx as f32 / w as f32, dy as f32 / h as f32])
}

struct Comp {
    c: MaskComponent,
    target: Px,
    matte: Option<(Win, Vec<f32>)>,
}

impl Comp {
    #[inline]
    fn eval(&self, xi: usize, yi: usize, w: usize, h: usize, p: Px) -> f32 {
        let c = &self.c;
        let (x, y, w, h) = ((xi as f32 + 0.5) / w as f32, (yi as f32 + 0.5) / h as f32, w as f32, h as f32);
        let soft = 0.5 + c.feather / 200.0; // 0..1
        let v = match c.kind.as_str() {
            "linear_gradient" => {
                let (dx, dy) = ((c.end[0] - c.start[0]) * w, (c.end[1] - c.start[1]) * h);
                let len2 = (dx * dx + dy * dy).max(1e-6);
                let t = ((x - c.start[0]) * w * dx + (y - c.start[1]) * h * dy) / len2;
                let f = 0.1 + 1.8 * soft;
                1.0 - smooth(0.0, 1.0, 0.5 + (t - 0.5) / f)
            }
            "radial_gradient" => {
                let dx = (x - c.center[0]) / c.radius[0].max(1e-3);
                let dy = (y - c.center[1]) / c.radius[1].max(1e-3);
                1.0 - smooth(1.0 - soft.max(0.01), 1.0, (dx * dx + dy * dy).sqrt())
            }
            "luminance_range" => {
                let l = luma(p).max(0.0).sqrt();
                let s = 0.02 + soft * 0.2;
                smooth(c.range[0] - s, c.range[0], l) * (1.0 - smooth(c.range[1], c.range[1] + s, l))
            }
            "color_range" => {
                let q = oklab(p);
                let t = &self.target;
                let d = ((q[0] - t[0]) * 0.5).hypot(q[1] - t[1]).hypot(q[2] - t[2]);
                let tol = 0.02 + c.tolerance / 100.0 * 0.2;
                1.0 - smooth(tol * (0.3 + 0.5 * (1.0 - soft)), tol, d)
            }
            "polygon" => match &self.matte {
                Some((r, v)) if xi >= r.x0 && yi >= r.y0 && xi < r.x0 + r.w && yi < r.y0 + r.h => v[(yi - r.y0) * r.w + xi - r.x0],
                _ => 0.0,
            },
            _ => 0.0,
        };
        let v = if c.invert { 1.0 - v } else { v };
        if c.kind == "polygon" || v <= 0.0 || v >= 1.0 {
            v.clamp(0.0, 1.0)
        } else {
            (v + c.edge_shift / 200.0).clamp(0.0, 1.0)
        }
    }
}

fn mask_weights(m: &Mask, w: usize, h: usize, px: &[Px]) -> Vec<f32> {
    let comps: Vec<Comp> = m
        .components
        .iter()
        .map(|c| Comp {
            target: oklab(srgb_to_working(c.color)),
            matte: (c.kind == "polygon").then(|| object_matte(&c.points, px, w, h, 0.5 + c.feather / 200.0, c.edge_shift)),
            c: c.clone(),
        })
        .collect();
    px.par_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut acc: Option<f32> = None;
            for c in &comps {
                let v = c.eval(i % w, i / w, w, h, *p);
                acc = Some(match (acc, c.c.op.as_str()) {
                    (None, "subtract") => 1.0 - v,
                    (None, _) => v,
                    (Some(a), "subtract") => a * (1.0 - v),
                    (Some(a), "intersect") => a * v,
                    (Some(a), _) => a.max(v),
                });
            }
            acc.unwrap_or(0.0)
        })
        .collect()
}

fn is_poly_object(m: &Mask) -> bool {
    m.components.first().is_some_and(|c| c.kind == "polygon" && c.op != "subtract" && !c.invert)
}

fn apply_masks(px: &mut [Px], w: usize, h: usize, masks: &[Mask], overlay: Option<usize>) -> Vec<Option<Vec<f32>>> {
    let raw: Vec<Option<Vec<f32>>> = masks
        .iter()
        .enumerate()
        .map(|(i, m)| (m.enabled || overlay == Some(i)).then(|| mask_weights(m, w, h, px)))
        .collect();
    let areas: Vec<f32> = raw
        .iter()
        .map(|w| w.as_ref().map_or(0.0, |v| v.iter().copied().sum::<f32>()))
        .collect();
    let mut out = Vec::with_capacity(masks.len());
    for (i, m) in masks.iter().enumerate() {
        let Some(mut wts) = raw[i].clone() else {
            out.push(None);
            continue;
        };
        // Foreground object protection: a smaller polygon object (e.g. a person in front of a castle)
        // is automatically excluded from any larger background polygon selection so they never mix.
        if is_poly_object(m) && areas[i] > 0.0 {
            for (j, mj) in masks.iter().enumerate() {
                if i != j
                    && is_poly_object(mj)
                    && areas[j] > 0.0
                    && areas[j] < 0.65 * areas[i]
                    && let Some(wj) = &raw[j]
                {
                    wts.par_iter_mut().zip(wj).for_each(|(a, &b)| *a *= 1.0 - b);
                }
            }
        }
        if m.enabled && m.adjustments != LocalAdjust::default() {
            let a = &m.adjustments;
            let tone = Tone::new(&BasicTone {
                exposure: 0.0,
                contrast: a.contrast,
                highlights: a.highlights,
                shadows: a.shadows,
                whites: a.whites,
                blacks: a.blacks,
            });
            let g = wb_gains(5500.0 * (a.temperature_shift / 100.0).exp2(), a.tint_shift).map(|v| v * a.exposure.exp2());
            let sat = 1.0 + a.saturation / 100.0;
            px.par_iter_mut().zip(&wts).for_each(|(p, &m)| {
                if m <= 0.0 {
                    return;
                }
                let mut q = tone.apply([p[0] * g[0], p[1] * g[1], p[2] * g[2]]);
                if sat != 1.0 {
                    let y = luma(q);
                    q = q.map(|c| y + (c - y) * sat);
                }
                *p = lerp(*p, q, m);
            });
        }
        out.push(Some(wts));
    }
    out
}

/// Monotone cubic (Fritsch–Carlson) through curve points, in 0–1 units.
struct Spline {
    x: Vec<f32>,
    y: Vec<f32>,
    m: Vec<f32>,
}
impl Spline {
    fn new(pts: &[[f32; 2]]) -> Option<Self> {
        let mut p: Vec<[f32; 2]> = pts.iter().map(|q| [q[0] / 255.0, q[1] / 255.0]).collect();
        p.sort_by(|a, b| a[0].total_cmp(&b[0]));
        p.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-4);
        if p.len() < 2 || p.iter().all(|q| (q[0] - q[1]).abs() < 1e-4) {
            return None;
        }
        let n = p.len();
        let (x, y): (Vec<f32>, Vec<f32>) = p.iter().map(|q| (q[0], q[1])).unzip();
        let d: Vec<f32> = (0..n - 1).map(|k| (y[k + 1] - y[k]) / (x[k + 1] - x[k])).collect();
        let mut m = vec![0.0; n];
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for k in 1..n - 1 {
            m[k] = if d[k - 1] * d[k] <= 0.0 { 0.0 } else { (d[k - 1] + d[k]) / 2.0 };
        }
        for k in 0..n - 1 {
            if d[k] == 0.0 {
                m[k] = 0.0;
                m[k + 1] = 0.0;
            } else {
                let (a, b) = (m[k] / d[k], m[k + 1] / d[k]);
                let s = a * a + b * b;
                if s > 9.0 {
                    let t = 3.0 / s.sqrt();
                    m[k] = t * a * d[k];
                    m[k + 1] = t * b * d[k];
                }
            }
        }
        Some(Spline { x, y, m })
    }
    fn eval(&self, v: f32) -> f32 {
        let n = self.x.len();
        if v <= self.x[0] {
            return self.y[0];
        }
        if v >= self.x[n - 1] {
            return self.y[n - 1];
        }
        let k = self.x.partition_point(|&q| q <= v) - 1;
        let hh = self.x[k + 1] - self.x[k];
        let t = (v - self.x[k]) / hh;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * self.y[k]
            + (t3 - 2.0 * t2 + t) * hh * self.m[k]
            + (-2.0 * t3 + 3.0 * t2) * self.y[k + 1]
            + (t3 - t2) * hh * self.m[k + 1]
    }
}

fn parametric(p: &Parametric, x: f32) -> f32 {
    let s = [0.0, p.splits[0] / 100.0, p.splits[1] / 100.0, p.splits[2] / 100.0, 1.0];
    let amounts = [p.shadows, p.darks, p.lights, p.highlights];
    let mut y = x;
    for i in 0..4 {
        let (a, b) = (s[i], s[i + 1]);
        let d = (x - (a + b) / 2.0) / (b - a).max(0.05);
        if d.abs() < 1.0 {
            y += amounts[i] / 100.0 * 0.08 * (d * PI / 2.0).cos().powi(2);
        }
    }
    y.clamp(0.0, 1.0)
}

const CURVE_N: usize = 1024;

/// Tone curves composed into one LUT per channel over a sqrt-encoded domain; curve
/// math happens on sRGB display values so points match the histogram the user sees.
struct Curves {
    lut: [Vec<f32>; 3],
}
impl Curves {
    fn new(tc: &ToneCurve) -> Option<Self> {
        let para = tc.parametric != Parametric { splits: tc.parametric.splits, ..Parametric::default() };
        let luma = Spline::new(&tc.point_luma);
        let ch = [Spline::new(&tc.point_red), Spline::new(&tc.point_green), Spline::new(&tc.point_blue)];
        if !para && luma.is_none() && ch.iter().all(Option::is_none) {
            return None;
        }
        let lut = [0, 1, 2].map(|c| {
            let mut lut: Vec<f32> = (0..=CURVE_N)
                .map(|i| {
                    let v = i as f32 / CURVE_N as f32;
                    let mut x = srgb_oetf(v * v);
                    if para {
                        x = parametric(&tc.parametric, x);
                    }
                    if let Some(s) = &luma {
                        x = s.eval(x);
                    }
                    if let Some(s) = &ch[c] {
                        x = s.eval(x);
                    }
                    srgb_eotf(x.clamp(0.0, 1.0)).sqrt()
                })
                .collect();
            for i in 1..lut.len() {
                lut[i] = lut[i].max(lut[i - 1]);
            }
            lut
        });
        Some(Curves { lut })
    }
    #[inline]
    fn apply(&self, p: Px) -> Px {
        [0, 1, 2].map(|c| {
            let v = p[c].max(0.0).sqrt();
            let lut = &self.lut[c];
            let o = if v >= 1.0 {
                lut[CURVE_N] + (v - 1.0)
            } else {
                let f = v * CURVE_N as f32;
                let i = f as usize;
                lut[i] + (lut[i + 1] - lut[i]) * (f - i as f32)
            };
            o * o
        })
    }
}

/// Oklch hue centres (degrees) of the 8 colour-mixer bands.
const BANDS: [f32; 8] = [29.0, 55.0, 100.0, 142.0, 195.0, 264.0, 295.0, 330.0];

struct Sw {
    h: f32,
    c: f32,
    l: f32,
    rh: f32,
    rc: f32,
    rl: f32,
    inner: f32,
    outer: f32,
    shift: Px,
}

struct Grade {
    off: [Px; 3],
    global: Px,
    bal: f32,
    k: f32,
}

struct ColorOps {
    active: bool,
    mono: bool,
    sat: f32,
    vib: f32,
    mixer: Option<[Px; 8]>,
    grade: Option<Grade>,
    swatches: Vec<Sw>,
}

impl ColorOps {
    fn new(d: &Develop) -> Self {
        let m = &d.color_mixer;
        let mixer = (m != &ColorMixer::default()).then(|| {
            std::array::from_fn(|i| {
                [m.hue[i] / 100.0 * 30f32.to_radians(), m.saturation[i] / 100.0, m.luminance[i] / 100.0 * 0.2]
            })
        });
        let g = &d.color_grading_3way;
        let wheel = |w: &Wheel| -> Px {
            if w.sat == 0.0 && w.lum == 0.0 {
                return [0.0; 3];
            }
            let q = oklab(srgb_to_working(hue_rgb(w.hue)));
            let n = q[1].hypot(q[2]).max(1e-6);
            let s = w.sat / 100.0 * 0.06;
            [w.lum / 100.0 * 0.12, q[1] / n * s, q[2] / n * s]
        };
        let grade = [&g.shadows, &g.midtones, &g.highlights, &g.global]
            .iter()
            .any(|w| w.sat != 0.0 || w.lum != 0.0)
            .then(|| Grade {
                off: [wheel(&g.shadows), wheel(&g.midtones), wheel(&g.highlights)],
                global: wheel(&g.global),
                bal: (-g.balance / 100.0).exp2(),
                k: 1.0 + (50.0 - g.blending) / 50.0,
            });
        let swatches: Vec<Sw> = d
            .point_color
            .iter()
            .filter(|s| s.shift_hsl != [0.0; 3])
            .map(|s| Sw {
                h: s.src_hsl[0].to_radians(),
                c: s.src_hsl[1] / 250.0,
                l: s.src_hsl[2] / 100.0,
                rh: (5.0 + s.range_hsl[0] / 100.0 * 55.0).to_radians(),
                rc: 0.01 + s.range_hsl[1] / 100.0 * 0.15,
                rl: 0.02 + s.range_hsl[2] / 100.0 * 0.4,
                inner: (1.0 - s.falloff / 100.0).max(0.0),
                outer: 1.0 + s.falloff / 100.0,
                shift: [s.shift_hsl[0] / 100.0 * 30f32.to_radians(), s.shift_hsl[1] / 100.0, s.shift_hsl[2] / 100.0 * 0.2],
            })
            .collect();
        let mono = d.profile.eq_ignore_ascii_case("monochrome");
        let (sat, vib) = (d.presence.saturation / 100.0, d.presence.vibrance / 100.0);
        ColorOps {
            active: mono || sat != 0.0 || vib != 0.0 || mixer.is_some() || grade.is_some() || !swatches.is_empty(),
            mono,
            sat,
            vib,
            mixer,
            grade,
            swatches,
        }
    }

    #[inline]
    fn apply(&self, p: Px) -> Px {
        let [mut l, a, b] = oklab(p);
        let mut c = a.hypot(b);
        let mut h = b.atan2(a);
        if self.sat != 0.0 || self.vib != 0.0 {
            let hd = h.to_degrees().rem_euclid(360.0);
            let skin = 1.0 - 0.6 * smooth(25.0, 0.0, (hd - 62.0).abs()); // protect skin hues
            let vib = self.vib * (1.0 - (c / 0.25).min(1.0)) * if self.vib > 0.0 { skin } else { 1.0 };
            c *= ((1.0 + self.sat) * (1.0 + vib)).max(0.0);
        }
        if let Some(mx) = &self.mixer {
            let hd = h.to_degrees().rem_euclid(360.0);
            let i = BANDS.iter().rposition(|&b| b <= hd).unwrap_or(7);
            let j = (i + 1) % 8;
            let span = (BANDS[j] - BANDS[i]).rem_euclid(360.0);
            let t = smooth(0.0, 1.0, (hd - BANDS[i]).rem_euclid(360.0) / span);
            let gate = smooth(0.0, 0.06, c);
            let s = [0, 1, 2].map(|k| (mx[i][k] * (1.0 - t) + mx[j][k] * t) * gate);
            h += s[0];
            c *= (1.0 + s[1]).max(0.0);
            l += s[2];
        }
        for s in &self.swatches {
            let dh = (h - s.h + PI).rem_euclid(2.0 * PI) - PI;
            let d = ((dh / s.rh).powi(2) + ((c - s.c) / s.rc).powi(2) + ((l - s.l) / s.rl).powi(2)).sqrt();
            let wgt = 1.0 - smooth(s.inner, s.outer, d);
            if wgt > 0.0 {
                h += s.shift[0] * wgt;
                c *= (1.0 + s.shift[1] * wgt).max(0.0);
                l += s.shift[2] * wgt;
            }
        }
        if self.mono {
            c = 0.0;
        }
        let (mut a, mut b) = (c * h.cos(), c * h.sin());
        if let Some(g) = &self.grade {
            let mut lb = l.clamp(0.0, 1.0);
            if g.bal != 1.0 {
                lb = lb.powf(g.bal);
            }
            let mut w = [(1.0 - lb).powi(2), 2.0 * lb * (1.0 - lb), lb * lb];
            if g.k != 1.0 {
                w = w.map(|v| v.powf(g.k));
                let n = w[0] + w[1] + w[2];
                w = w.map(|v| v / n);
            }
            for z in 0..3 {
                l += w[z] * g.off[z][0];
                a += w[z] * g.off[z][1];
                b += w[z] * g.off[z][2];
            }
            l += g.global[0];
            a += g.global[1];
            b += g.global[2];
        }
        from_oklab([l, a, b])
    }
}

/// Pro: Photoshop's adjustment layers as one fixed stack, after Gaussian blur: Levels, Hue/Saturation,
/// Selective Color, Channel Mixer, Gradient Map, Invert, Posterize, Threshold.
/// ponytail: the RGB ones work on display sRGB values clipped to 0-1, like a Photoshop sRGB document,
/// so wide-gamut colours and HDR highlights clip while one is on; give them extended-range Rec.2020
/// values if wide-gamut exports need those colours.
struct Pro {
    active: bool,
    levels: Option<[Vec<f32>; 3]>,
    /// hue turn (sin, cos), chroma gain, lightness toward white (+) / black (-), colorize [a, b]
    hue_sat: Option<((f32, f32), f32, f32, Option<[f32; 2]>)>,
    /// non-zero families as (index, [c, m, y, k] / 100), absolute
    selective: Option<(Vec<(usize, [f32; 4])>, bool)>,
    mixer: Option<[[f32; 4]; 3]>,
    /// Oklab ends and amount
    gradient: Option<(Px, Px, f32)>,
    invert: bool,
    /// levels - 1, threshold 0-1
    posterize: Option<f32>,
    threshold: Option<f32>,
}

const LEVELS_N: usize = 1024;

/// sRGB display value of a working-space colour, clipped to 0-1, and back.
fn enc(p: Px) -> Px {
    mul(&M2020_709, p).map(|v| srgb_oetf(v.clamp(0.0, 1.0)))
}
fn dec(e: Px) -> Px {
    srgb_to_working(e)
}

impl Pro {
    fn new(d: &Develop) -> Self {
        let lv = &d.levels;
        let levels = (*lv != Levels::default())
            .then(|| [&lv.red, &lv.green, &lv.blue].map(|l| (0..=LEVELS_N).map(|i| l.eval(lv.rgb.eval(i as f32 / LEVELS_N as f32))).collect()));
        let hs = &d.hue_saturation;
        let hue_sat = (hs.colorize || hs.hue != 0.0 || hs.saturation != 0.0 || hs.lightness != 0.0).then(|| {
            let tint = hs.colorize.then(|| {
                let q = oklab(srgb_to_working(hue_rgb(hs.colorize_hue)));
                let k = hs.colorize_saturation / 100.0 * 0.2 / q[1].hypot(q[2]).max(1e-6);
                [q[1] * k, q[2] * k]
            });
            (hs.hue.to_radians().sin_cos(), 1.0 + hs.saturation / 100.0, hs.lightness / 100.0, tint)
        });
        let s = &d.selective_color;
        let fams: Vec<(usize, [f32; 4])> = [s.reds, s.yellows, s.greens, s.cyans, s.blues, s.magentas, s.whites, s.neutrals, s.blacks]
            .into_iter()
            .enumerate()
            .filter(|(_, f)| *f != [0.0; 4])
            .map(|(i, f)| (i, f.map(|v| v / 100.0)))
            .collect();
        let selective = (!fams.is_empty()).then_some((fams, s.absolute));
        let (m, id) = (&d.channel_mixer, ChannelMixer::default());
        let mixer = (m.monochrome || [m.red, m.green, m.blue] != [id.red, id.green, id.blue])
            .then(|| if m.monochrome { [m.gray; 3] } else { [m.red, m.green, m.blue] }.map(|r| r.map(|v| v / 100.0)));
        let g = &d.gradient_map;
        let gradient = (g.amount > 0.0).then(|| (oklab(srgb_to_working(g.shadows)), oklab(srgb_to_working(g.highlights)), g.amount / 100.0));
        let f = &d.filters;
        let posterize = (f.posterize >= 2.0).then(|| f.posterize.round() - 1.0);
        let threshold = (f.threshold > 0.0).then(|| f.threshold / 255.0);
        Pro {
            active: levels.is_some() || hue_sat.is_some() || selective.is_some() || mixer.is_some() || gradient.is_some() || f.invert || posterize.is_some() || threshold.is_some(),
            levels,
            hue_sat,
            selective,
            mixer,
            gradient,
            invert: f.invert,
            posterize,
            threshold,
        }
    }

    #[inline]
    fn apply(&self, mut p: Px) -> Px {
        if let Some(lut) = &self.levels {
            let e = enc(p);
            p = dec([0, 1, 2].map(|c| {
                let f = e[c] * LEVELS_N as f32;
                let i = (f as usize).min(LEVELS_N - 1);
                lut[c][i] + (lut[c][i + 1] - lut[c][i]) * (f - i as f32)
            }));
        }
        if let Some(((sin, cos), sat, light, tint)) = self.hue_sat {
            let [mut l, mut a, mut b] = oklab(p);
            if let Some([ta, tb]) = tint {
                // like HSL colorize: the tint fades toward black and white
                let k = (4.0 * l * (1.0 - l)).clamp(0.0, 1.0);
                (a, b) = (ta * k, tb * k);
            } else {
                (a, b) = ((a * cos - b * sin) * sat, (a * sin + b * cos) * sat);
            }
            if light > 0.0 {
                l += (1.0 - l) * light;
            } else {
                l *= 1.0 + light;
            }
            let k = 1.0 - light.abs();
            p = from_oklab([l, a * k, b * k]);
        }
        if let Some((fams, absolute)) = &self.selective {
            // family weights (Photoshop's model): how far the max channel leads (primaries) or the min
            // channel trails (secondaries), and the distance from mid grey for whites / neutrals / blacks
            let e = enc(p);
            let [r, g, b] = e;
            let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
            let md = r + g + b - mx - mn;
            let lead = |c: f32| if c == mx { mx - md } else { 0.0 };
            let trail = |c: f32| if c == mn { md - mn } else { 0.0 };
            let wts = [
                lead(r),
                trail(b),
                lead(g),
                trail(r),
                lead(b),
                trail(g),
                ((mn - 0.5) * 2.0).max(0.0),
                1.0 - (mx - 0.5).abs() - (mn - 0.5).abs(),
                ((0.5 - mx) * 2.0).max(0.0),
            ];
            let mut o = e;
            for &(i, f) in fams {
                if wts[i] > 0.0 {
                    for c in 0..3 {
                        // ink (1 - value) grows by the colour's ink and then by black
                        let ink = f[c] + f[3] * (1.0 + f[c]);
                        o[c] -= wts[i] * ink * if *absolute { 1.0 } else { 1.0 - e[c] };
                    }
                }
            }
            p = dec(o.map(|v| v.clamp(0.0, 1.0)));
        }
        if let Some(m) = &self.mixer {
            let e = enc(p);
            p = dec(m.map(|r| (r[0] * e[0] + r[1] * e[1] + r[2] * e[2] + r[3]).clamp(0.0, 1.0)));
        }
        if let Some((s, h, amt)) = self.gradient {
            let q = oklab(p);
            p = from_oklab(lerp(q, lerp(s, h, q[0].clamp(0.0, 1.0)), amt));
        }
        if self.invert || self.posterize.is_some() || self.threshold.is_some() {
            let mut e = enc(p);
            if self.invert {
                e = e.map(|v| 1.0 - v);
            }
            if let Some(n) = self.posterize {
                e = e.map(|v| (v * n).round() / n);
            }
            if let Some(t) = self.threshold {
                e = [if srgb_oetf(luma(dec(e)).clamp(0.0, 1.0)) >= t { 1.0 } else { 0.0 }; 3];
            }
            p = dec(e);
        }
        p
    }
}

fn color_nr(px: &mut [Px], w: usize, h: usize, amount: f32) {
    let lab: Vec<Px> = px.par_iter().map(|p| oklab(*p)).collect();
    let sigma = 1.0 + amount * w.max(h) as f32 * 0.004;
    let a = blur(&lab.iter().map(|q| q[1]).collect::<Vec<_>>(), w, h, sigma);
    let b = blur(&lab.iter().map(|q| q[2]).collect::<Vec<_>>(), w, h, sigma);
    px.par_iter_mut().enumerate().for_each(|(i, p)| *p = from_oklab([lab[i][0], a[i], b[i]]));
}

/// Edge-preserving luminance smoothing: a self-guided filter on sqrt luma, where `luminance_detail`
/// lowers the edge contrast it smooths across. Sized relative to the frame like the other stages.
fn luma_nr(px: &mut [Px], w: usize, h: usize, d: &Detail) {
    let amt = d.luminance_noise_reduction / 100.0;
    let r = (0.5 + 2.5 * amt) * w.max(h) as f32 / 1600.0;
    let eps = amt * amt * (0.02 * (1.0 - d.luminance_detail / 100.0) + 0.0005);
    let v: Vec<f32> = px.par_iter().map(|p| luma(*p).max(0.0).sqrt()).collect();
    let mean = blur(&v, w, h, r);
    let sq = blur(&v.iter().map(|x| x * x).collect::<Vec<f32>>(), w, h, r);
    let a: Vec<f32> = sq.iter().zip(&mean).map(|(s, m)| (s - m * m).max(0.0)).map(|var| var / (var + eps)).collect();
    let b: Vec<f32> = a.iter().zip(&mean).map(|(a, m)| m * (1.0 - a)).collect();
    let (ma, mb) = (blur(&a, w, h, r), blur(&b, w, h, r));
    px.par_iter_mut().enumerate().for_each(|(i, p)| {
        if v[i] > 1e-4 {
            let k = ((ma[i] * v[i] + mb[i]).max(0.0) / v[i]).powi(2).min(8.0);
            *p = p.map(|c| c * k);
        }
    });
}

/// Lens corrections on the full frame: lateral chromatic aberration (red and blue rescaled about the
/// centre), lens vignetting, then defringe (purple / green fringes along high-contrast edges go grey).
fn lens(px: &mut Vec<Px>, w: usize, h: usize, l: &Lens) {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let rn = 4.0 / (w * w + h * h) as f32;
    if l.ca_red_cyan != 0.0 || l.ca_blue_yellow != 0.0 {
        // in linear sRGB: the channels a JPEG's fringes are in (Rec.2020 would mix them)
        let s = [l.ca_red_cyan, 0.0, l.ca_blue_yellow].map(|v| 1.0 + v / 100.0 * CA_MAX);
        let src = Img { w, h, px: px.par_iter().map(|p| mul(&M2020_709, *p)).collect() };
        let (mx, my) = ((w - 1) as f32, (h - 1) as f32);
        *px = (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = ((i % w) as f32 + 0.5 - cx, (i / w) as f32 + 0.5 - cy);
                let at = |c: usize| sample(&src, (cx + x * s[c] - 0.5).clamp(0.0, mx), (cy + y * s[c] - 0.5).clamp(0.0, my))[c];
                mul(&M709_2020, [at(0), src.px[i][1], at(2)])
            })
            .collect();
    }
    if l.vignetting != 0.0 {
        let (v, e) = (l.vignetting / 100.0 * 1.5, 1.0 + l.vignetting_midpoint / 25.0);
        px.par_iter_mut().enumerate().for_each(|(i, p)| {
            let (x, y) = ((i % w) as f32 + 0.5 - cx, (i / w) as f32 + 0.5 - cy);
            let g = (v * ((x * x + y * y) * rn).powf(e / 2.0)).exp2();
            *p = p.map(|c| c * g);
        });
    }
    if l.defringe_purple > 0.0 || l.defringe_green > 0.0 {
        let v: Vec<f32> = px.par_iter().map(|p| luma(*p).max(0.0).sqrt()).collect();
        let grad: Vec<f32> = (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let dx = v[y * w + (x + 1).min(w - 1)] - v[y * w + x.saturating_sub(1)];
                let dy = v[(y + 1).min(h - 1) * w + x] - v[y.saturating_sub(1) * w + x];
                dx.hypot(dy) / 2.0
            })
            .collect();
        // edge contrast near each pixel: blurred |∇| × σ is about the step height at any resolution
        let sigma = (1.0 + l.defringe_purple.max(l.defringe_green) / 20.0 * 3.0) * w.max(h) as f32 / 1600.0;
        let edge = blur(&grad, w, h, sigma);
        let k = sigma.max(1.0) * 2.5;
        // Oklch hue windows: purple 240°-360° (blue to magenta), green 90°-200° (yellow to cyan)
        let bands = [(l.defringe_purple, 240.0, 1.2, l.defringe_purple_hue), (l.defringe_green, 90.0, 1.1, l.defringe_green_hue)];
        px.par_iter_mut().zip(&edge).for_each(|(p, &e)| {
            let e = smooth(0.05, 0.2, e * k);
            if e <= 0.0 {
                return;
            }
            let [lum, a, b] = oklab(*p);
            if a.hypot(b) < 0.01 {
                return;
            }
            let hd = b.atan2(a).to_degrees().rem_euclid(360.0);
            let mut f = 0.0f32;
            for (amount, base, span, r) in bands {
                if amount > 0.0 {
                    let (lo, hi) = (base + r[0] * span, base + r[1] * span);
                    let off = ((hd - (lo + hi) / 2.0 + 180.0).rem_euclid(360.0) - 180.0).abs();
                    f = f.max((1.0 - smooth((hi - lo) / 2.0, (hi - lo) / 2.0 + 10.0, off)) * (amount / 8.0).min(1.0));
                }
            }
            if f > 0.0 {
                let s = 1.0 - f * e;
                *p = from_oklab([lum, a * s, b * s]);
            }
        });
    }
}

/// Camera calibration as a white-preserving matrix (each primary moved toward its neighbour and
/// away from / toward grey at constant luminance), plus a green-magenta tint in the shadows.
fn calibrate(px: &mut [Px], c: &Calibration) {
    if *c == Calibration::default() {
        return;
    }
    let (hue, sat) = ([c.red_hue, c.green_hue, c.blue_hue], [c.red_saturation, c.green_saturation, c.blue_saturation]);
    let cols: [Px; 3] = std::array::from_fn(|k| {
        let mut p = IDENT[k];
        let t = hue[k] / 100.0 * 0.35;
        p[if t > 0.0 { (k + 1) % 3 } else { (k + 2) % 3 }] += t.abs();
        let y = luma(p);
        let p = p.map(|v| y + (v - y) * (1.0 + sat[k] / 100.0 * 0.6));
        let n = luma(IDENT[k]) / luma(p).max(1e-6);
        p.map(|v| v * n)
    });
    let mut m: M3 = std::array::from_fn(|i| std::array::from_fn(|k| cols[k][i]));
    for row in &mut m {
        let s: f32 = row.iter().sum();
        *row = row.map(|v| v / s.max(1e-3));
    }
    let tint = c.shadows_tint / 100.0 * 0.3;
    px.par_iter_mut().for_each(|p| {
        let mut q = mul(&m, *p);
        if tint != 0.0 {
            let y = luma(q);
            q[1] *= (-tint * (1.0 - smooth(0.0, 0.4, y.max(0.0).sqrt()))).exp2();
            let n = y / luma(q).max(1e-9);
            q = q.map(|v| v * n);
        }
        *p = q;
    });
}

fn sharpen(px: &mut [Px], w: usize, h: usize, d: &Detail) {
    let v: Vec<f32> = px.par_iter().map(|p| luma(*p).max(0.0).sqrt()).collect();
    let b = blur(&v, w, h, d.sharpen_radius);
    let (amt, m) = (d.sharpening / 100.0, d.sharpen_masking / 100.0);
    px.par_iter_mut().enumerate().for_each(|(i, p)| {
        let detail = v[i] - b[i];
        let gate = if m > 0.0 { smooth(m * 0.01, m * 0.03, detail.abs()) } else { 1.0 };
        if v[i] > 1e-4 {
            let r = ((v[i] + amt * detail * gate).max(0.0) / v[i]).powi(2).min(8.0);
            *p = p.map(|c| c * r);
        }
    });
}

/// Focal length of the virtual camera as a fraction of the long edge (~32mm on full frame).
/// ponytail: fixed; EXIF focal length and sensor size would make Upright's aspect exact.
const FOCAL: f32 = 0.9;
/// Transform vertical / horizontal at ±100 tilt or pan the virtual camera this many degrees.
const TILT_MAX: f32 = 40.0;
/// Lateral CA at ±100 scales the red / blue channel by this fraction.
const CA_MAX: f32 = 0.002;

fn mat(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// Virtual camera turn: pan (yaw), then tilt (pitch), then roll; radians.
fn rotation(pitch: f32, yaw: f32, roll: f32) -> M3 {
    let ((sp, cp), (sy, cy), (sr, cr)) = (pitch.sin_cos(), yaw.sin_cos(), roll.sin_cos());
    let rx = [[1.0, 0.0, 0.0], [0.0, cp, -sp], [0.0, sp, cp]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cr, -sr, 0.0], [sr, cr, 0.0], [0.0, 0.0, 1.0]];
    mat(&rz, &mat(&rx, &ry))
}

/// Lens distortion and Transform as pixel mappings about the frame centre (stage 9, before
/// straighten and crop). Resolution independent, so the proxy and the export agree.
struct Geo {
    c: [f32; 2],
    f: f32,
    /// inverse camera turn
    rt: M3,
    /// aspect × scale per axis, offset in pixels
    a: [f32; 2],
    o: [f32; 2],
    /// where the turn moves the frame centre (undone, so the centre stays put)
    shift: [f32; 2],
    /// radial distortion r_src = r (1 + k r²) in half-diagonal units
    k: f32,
    rn: f32,
    active: bool,
}

impl Geo {
    fn new(d: &Develop, w: usize, h: usize) -> Self {
        let t = &d.transform;
        let (fw, fh) = (w as f32, h as f32);
        let r = rotation((-t.vertical / 100.0 * TILT_MAX).to_radians(), (-t.horizontal / 100.0 * TILT_MAX).to_radians(), (-t.rotate).to_radians());
        let (s, a, f) = (t.scale / 100.0, (t.aspect / 200.0).exp2(), FOCAL * fw.max(fh));
        // the turn magnifies the centre (1/cos²); scale back to keep its area, as Lightroom does
        let fwd = |u: f32, v: f32| {
            let q = mul(&r, [u, v, f]);
            [f * q[0] / q[2], f * q[1] / q[2]]
        };
        let (c0, e) = (fwd(0.0, 0.0), 0.01 * f);
        let (jx, jy) = (fwd(e, 0.0), fwd(0.0, e));
        let det = ((jx[0] - c0[0]) * (jy[1] - c0[1]) - (jx[1] - c0[1]) * (jy[0] - c0[0])) / (e * e);
        let n = 1.0 / det.max(1e-3).sqrt();
        Geo {
            c: [fw / 2.0, fh / 2.0],
            f,
            rt: std::array::from_fn(|i| std::array::from_fn(|j| r[j][i])),
            a: [s * a * n, s / a * n],
            o: [t.offset_x / 100.0 * fw / 2.0, t.offset_y / 100.0 * fh / 2.0],
            shift: c0,
            k: -d.lens.distortion / 100.0 * 0.25,
            rn: 4.0 / (fw * fw + fh * fh),
            active: d.lens.distortion != 0.0 || !t.is_identity(),
        }
    }

    /// Transformed-frame point -> source position; None where the virtual camera sees nothing.
    #[inline]
    fn to_source(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        let (u, v) = ((x - self.c[0] - self.o[0]) / self.a[0] + self.shift[0], (y - self.c[1] - self.o[1]) / self.a[1] + self.shift[1]);
        let q = mul(&self.rt, [u, v, self.f]);
        if q[2] < 1e-3 * self.f {
            return None;
        }
        let (u, v) = (self.f * q[0] / q[2], self.f * q[1] / q[2]);
        let g = 1.0 + self.k * (u * u + v * v) * self.rn;
        (g > 0.05).then(|| [self.c[0] + u * g, self.c[1] + v * g])
    }

    /// Source position -> lens-corrected position about the centre (fixed-point inverse of the distortion).
    fn undistort(&self, x: f32, y: f32) -> [f32; 2] {
        let (dx, dy) = (x - self.c[0], y - self.c[1]);
        let mut g = 1.0f32;
        for _ in 0..12 {
            let (u, v) = (dx / g, dy / g);
            g = (1.0 + self.k * (u * u + v * v) * self.rn).max(0.5);
        }
        [dx / g, dy / g]
    }
}

/// Lens distortion, transform, straighten, image rotation, then the crop rectangle, in one bilinear
/// resample. With `constrain_crop` the crop shrinks about its centre until no empty area shows.
fn geometry(img: Img, d: &Develop, apply_crop: bool) -> Img {
    let (c, r) = (&d.crop, &d.image_rotation);
    let geo = Geo::new(d, img.w, img.h);
    let quarter = (r.angle / 90.0).round() as i32 & 3;
    let turned = quarter != 0 || r.flip_h || r.flip_v;
    if !geo.active && !turned && (c.is_identity() || (!apply_crop && c.angle == 0.0)) {
        return img;
    }
    let (fw, fh) = (img.w as f32, img.h as f32);
    // the rotated frame, which the crop rectangle is in
    let (gw, gh) = if quarter % 2 == 1 { (fh, fw) } else { (fw, fh) };
    let (mut x0, mut y0, mut cw, mut ch) = if apply_crop { (c.x * gw, c.y * gh, c.w * gw, c.h * gh) } else { (0.0, 0.0, gw, gh) };
    let (sin, cos) = c.angle.to_radians().sin_cos();
    let (cx, cy) = (fw / 2.0, fh / 2.0);
    // rotated-frame point -> straightened-frame point -> source position
    let src = |x: f32, y: f32| {
        let (x, y) = (if r.flip_h { gw - x } else { x }, if r.flip_v { gh - y } else { y });
        let (x, y) = match quarter {
            1 => (y, fh - x),
            2 => (fw - x, fh - y),
            3 => (fw - y, x),
            _ => (x, y),
        };
        let (u, v) = (x - cx, y - cy);
        geo.to_source(cx + u * cos - v * sin, cy + u * sin + v * cos)
    };
    if apply_crop && d.transform.constrain_crop && (c.angle != 0.0 || geo.active) {
        // the crop outline (9 points per side: distortion bends the sides) must land inside the source
        let inside = |s: f32| {
            let (mx, my) = (x0 + cw / 2.0, y0 + ch / 2.0);
            (0..=8).all(|i| {
                let t = i as f32 / 8.0 - 0.5;
                [(t, -0.5), (t, 0.5), (-0.5, t), (0.5, t)].iter().all(|&(a, b)| {
                    src(mx + a * cw * s, my + b * ch * s).is_some_and(|p| p[0] >= -0.01 && p[1] >= -0.01 && p[0] <= fw + 0.01 && p[1] <= fh + 0.01)
                })
            })
        };
        if !inside(1.0) {
            let (mut lo, mut hi) = (0.05f32, 1.0f32);
            for _ in 0..24 {
                let m = (lo + hi) / 2.0;
                if inside(m) { lo = m } else { hi = m }
            }
            (x0, y0, cw, ch) = (x0 + cw * (1.0 - lo) / 2.0, y0 + ch * (1.0 - lo) / 2.0, cw * lo, ch * lo);
        }
    }
    let (ow, oh) = ((cw.round() as usize).max(1), (ch.round() as usize).max(1));
    let px = (0..ow * oh)
        .into_par_iter()
        .map(|i| match src(x0 + (i % ow) as f32 + 0.5, y0 + (i / ow) as f32 + 0.5) {
            Some(p) => sample(&img, p[0] - 0.5, p[1] - 0.5),
            None => [0.0; 3],
        })
        .collect();
    Img { w: ow, h: oh, px }
}

fn sample(img: &Img, x: f32, y: f32) -> Px {
    if x < -0.5 || y < -0.5 || x > img.w as f32 - 0.5 || y > img.h as f32 - 0.5 {
        return [0.0; 3];
    }
    let (x, y) = (x.clamp(0.0, (img.w - 1) as f32), y.clamp(0.0, (img.h - 1) as f32));
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| img.px[y * img.w + x];
    lerp(lerp(at(x0, y0), at(x1, y0), tx), lerp(at(x0, y1), at(x1, y1), tx), ty)
}

fn vignette(img: &mut Img, e: &Effects) {
    if e.vignette == 0.0 {
        return;
    }
    let amt = e.vignette / 100.0;
    let mid = 0.2 + e.vignette_midpoint / 100.0 * 0.7;
    let soft = 0.05 + e.vignette_feather / 100.0 * 0.6;
    let (w, fw, fh) = (img.w, img.w as f32, img.h as f32);
    img.px.par_iter_mut().enumerate().for_each(|(i, p)| {
        let x = ((i % w) as f32 + 0.5) / fw * 2.0 - 1.0;
        let y = ((i / w) as f32 + 0.5) / fh * 2.0 - 1.0;
        let f = smooth(mid - soft / 2.0, mid + soft / 2.0, (x * x + y * y).sqrt() / std::f32::consts::SQRT_2);
        let g = if amt < 0.0 { 1.0 + amt * f } else { 1.0 + 1.5 * amt * f };
        *p = p.map(|c| c * g);
    });
}

/// Smooth value noise in [-1, 1] on the integer lattice.
fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (tx, ty) = (x - xi, y - yi);
    let (tx, ty) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let hash = |i: f32, j: f32| {
        let mut k = (i as i32 as u32).wrapping_mul(0x8da6_b343) ^ (j as i32 as u32).wrapping_mul(0xd816_3841);
        k ^= k >> 13;
        k = k.wrapping_mul(0x5bd1_e995);
        k ^= k >> 15;
        k as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    let (a, b, c, d) = (hash(xi, yi), hash(xi + 1.0, yi), hash(xi, yi + 1.0), hash(xi + 1.0, yi + 1.0));
    a + (b - a) * tx + (c - a) * ty + (a - b - c + d) * tx * ty
}

/// Film grain on luminance, strongest in the midtones. Grain size is a fraction of the frame, so the
/// proxy shows the export's grain scaled down.
fn grain(img: &mut Img, g: &Grain) {
    if g.amount <= 0.0 {
        return;
    }
    let cell = (0.0005 + g.size / 100.0 * 0.0025) * img.w.max(img.h) as f32;
    let (amp, rough, w) = (g.amount / 100.0 * 0.1, g.roughness / 100.0, img.w);
    img.px.par_iter_mut().enumerate().for_each(|(i, p)| {
        let (x, y) = ((i % w) as f32 / cell, (i / w) as f32 / cell);
        let z = noise(x, y) * (1.0 - 0.5 * rough) + noise(2.7 * x + 31.0, 2.7 * y + 17.0) * rough;
        let v = luma(*p).max(0.0).sqrt();
        if v > 1e-4 {
            let t = v.min(1.0);
            let k = ((v + amp * z * (0.25 + 3.0 * t * (1.0 - t))).max(0.0) / v).powi(2);
            *p = p.map(|c| c * k);
        }
    });
}

// ---------------------------------------------------------------------------
// Upright and auto chromatic aberration (measured on the full-frame proxy)
// ---------------------------------------------------------------------------

/// A straight edge for Upright: lens-corrected endpoints about the centre (pixels), weight, and
/// whether it should become vertical (else horizontal).
type Seg = ([f32; 2], [f32; 2], f32, bool);

/// How far `s` is from vertical / horizontal after the camera turn `r` (sine of the angle).
fn seg_err(r: &M3, f: f32, s: &Seg) -> f32 {
    let proj = |a: [f32; 2]| {
        let q = mul(r, [a[0], a[1], f]);
        (q[2] > 1e-3 * f).then(|| [q[0] / q[2], q[1] / q[2]])
    };
    match (proj(s.0), proj(s.1)) {
        (Some(a), Some(b)) => {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            (if s.3 { dx } else { dy }) / dx.hypot(dy).max(1e-9)
        }
        _ => 2.0,
    }
}

/// Pitch, yaw and roll (radians) that turn `segs` vertical or horizontal: damped Gauss-Newton over
/// the angles allowed by `dof`, with `reg` pulling them toward zero.
fn solve_upright(segs: &[Seg], f: f32, dof: [bool; 3], reg: f32) -> [f32; 3] {
    let lim = [TILT_MAX.to_radians(), TILT_MAX.to_radians(), 10f32.to_radians()];
    let res = |p: [f32; 3]| -> Vec<f32> {
        let r = rotation(p[0], p[1], p[2]);
        segs.iter().map(|s| s.2.sqrt() * seg_err(&r, f, s)).chain(p.iter().map(|v| reg * v)).collect()
    };
    let cost = |r: &[f32]| r.iter().map(|v| v * v).sum::<f32>();
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    let det = |m: &M3| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let (mut p, mut mu) = ([0.0f32; 3], 1e-3f32);
    let mut r0 = res(p);
    for _ in 0..100 {
        let jac: Vec<Vec<f32>> = (0..3)
            .map(|k| {
                if !dof[k] {
                    return vec![0.0; r0.len()];
                }
                let mut q = p;
                q[k] += 1e-3;
                res(q).iter().zip(&r0).map(|(a, b)| (a - b) / 1e-3).collect()
            })
            .collect();
        let mut a: M3 = std::array::from_fn(|i| std::array::from_fn(|j| dot(&jac[i], &jac[j])));
        for i in 0..3 {
            a[i][i] = if dof[i] { a[i][i] * (1.0 + mu) + 1e-6 } else { 1.0 };
        }
        let g: Px = std::array::from_fn(|i| -dot(&jac[i], &r0));
        let dt = det(&a);
        if dt.abs() < 1e-20 {
            break;
        }
        // Cramer's rule for the 3×3 step
        let q: [f32; 3] = std::array::from_fn(|k| {
            let mut m = a;
            for i in 0..3 {
                m[i][k] = g[i];
            }
            (p[k] + det(&m) / dt).clamp(-lim[k], lim[k])
        });
        let r1 = res(q);
        if cost(&r1) < cost(&r0) {
            (p, r0, mu) = (q, r1, (mu * 0.3).max(1e-6));
        } else {
            mu *= 10.0;
            if mu > 1e4 {
                break;
            }
        }
    }
    p
}

/// Straight edges for Auto Upright: a Hough transform over thinned near-vertical and near-horizontal
/// edges at ~800px, each peak cut to its longest well-supported run. Normalised endpoints + length.
fn find_lines(img: &Img) -> Vec<([[f32; 2]; 2], f32)> {
    let k = (img.w.max(img.h) / 800).max(1);
    let (w, h) = (img.w / k, img.h / k);
    if w < 16 || h < 16 {
        return vec![];
    }
    let v: Vec<f32> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % w) * k, (i / w) * k);
            (0..k * k).map(|j| luma(img.px[(y + j / k) * img.w + x + j % k]).max(0.0).sqrt()).sum::<f32>() / (k * k) as f32
        })
        .collect();
    let v = blur(&v, w, h, 1.0);
    let sobel = |i: usize| {
        let at = |o: isize| v[(i as isize + o) as usize];
        let w = w as isize;
        let gx = at(1 - w) + 2.0 * at(1) + at(1 + w) - at(-1 - w) - 2.0 * at(-1) - at(w - 1);
        let gy = at(w - 1) + 2.0 * at(w) + at(w + 1) - at(-w - 1) - 2.0 * at(-w) - at(1 - w);
        (gx / 8.0, gy / 8.0)
    };
    let inner = |i: usize| (1..w - 1).contains(&(i % w)) && (1..h - 1).contains(&(i / w));
    let mag: Vec<f32> = (0..w * h).into_par_iter().map(|i| if inner(i) { let (gx, gy) = sobel(i); gx.hypot(gy) } else { 0.0 }).collect();
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    // edge pixels about the centre with their normal angle folded into [-45°, 135°)
    let edges: Vec<(f32, f32, f32)> = (0..w * h)
        .into_par_iter()
        .filter_map(|i| {
            if mag[i] < 0.02 || !inner(i) {
                return None;
            }
            let (gx, gy) = sobel(i);
            let o = if gx.abs() > gy.abs() { 1 } else { w };
            if mag[i] < mag[i - o] || mag[i] < mag[i + o] {
                return None; // not the ridge of the edge
            }
            let t = (gy.atan2(gx) + PI / 4.0).rem_euclid(PI) - PI / 4.0;
            let off = t.abs().min((t - PI / 2.0).abs());
            (off < 35f32.to_radians()).then_some(((i % w) as f32 + 0.5 - cx, (i / w) as f32 + 0.5 - cy, t))
        })
        .collect();
    const STEP: f32 = 0.5;
    let nt = (180.0 / STEP) as usize;
    let dmax = cx.hypot(cy).ceil() as usize;
    let nr = 2 * dmax + 1;
    let trig: Vec<(f32, f32)> = (0..nt).map(|j| (j as f32 * STEP - 45.0).to_radians().sin_cos()).collect();
    let mut acc = vec![0u32; nt * nr];
    for &(x, y, t) in &edges {
        let j0 = ((t.to_degrees() + 45.0) / STEP).round() as usize;
        for j in j0.saturating_sub(6)..(j0 + 7).min(nt) {
            let (s, c) = trig[j];
            acc[j * nr + ((x * c + y * s).round() + dmax as f32) as usize] += 1;
        }
    }
    let minv = (0.05 * w.min(h) as f32) as u32;
    let mut peaks: Vec<(u32, usize, usize)> = (0..nt * nr)
        .into_par_iter()
        .filter_map(|i| {
            let (a, j, r) = (acc[i], i / nr, i % nr);
            if a < minv {
                return None;
            }
            for jj in j.saturating_sub(2)..(j + 3).min(nt) {
                for rr in r.saturating_sub(3)..(r + 4).min(nr) {
                    let b = acc[jj * nr + rr];
                    if b > a || (b == a && (jj, rr) < (j, r)) {
                        return None;
                    }
                }
            }
            Some((a, j, r))
        })
        .collect();
    peaks.sort_by(|a, b| b.0.cmp(&a.0));
    peaks.truncate(80);
    // collinear edges across gaps (a column of windows) are one line; sparse support is not
    let (gap, min_len) = (0.08 * w.max(h) as f32 + 2.0, 0.08 * w.min(h) as f32);
    peaks
        .into_iter()
        .filter_map(|(_, j, r)| {
            let ((s, c), theta, rho) = (trig[j], (j as f32 * STEP - 45.0).to_radians(), r as f32 - dmax as f32);
            let mut ts: Vec<f32> = edges
                .iter()
                .filter(|&&(x, y, t)| (x * c + y * s - rho).abs() <= 1.5 && (t - theta).abs() <= 4f32.to_radians())
                .map(|&(x, y, _)| y * c - x * s)
                .collect();
            ts.sort_by(f32::total_cmp);
            // longest run of support without a big gap
            let (mut best, mut start) = ((0, 0), 0);
            for i in 1..=ts.len() {
                if i == ts.len() || ts[i] - ts[i - 1] > gap {
                    if best.1 == 0 || ts[i - 1] - ts[start] > ts[best.1 - 1] - ts[best.0] {
                        best = (start, i);
                    }
                    start = i;
                }
            }
            let (t0, t1) = (*ts.get(best.0)?, *ts.get(best.1.checked_sub(1)?)?);
            let len = t1 - t0;
            if len < min_len || ((best.1 - best.0) as f32) < 0.3 * len {
                return None;
            }
            let at = |t: f32| [(rho * c - t * s + cx) / w as f32, (rho * s + t * c + cy) / h as f32];
            Some(([at(t0), at(t1)], len * k as f32))
        })
        .collect()
}

/// Upright: the Transform that `mode` (off | auto | level | vertical | full | guided) gives `d`,
/// solved from straight lines in `img` (the full-frame proxy) or from the guides.
pub fn upright(img: &Img, d: &Develop, mode: &str) -> Result<Transform, String> {
    let mut t = Transform { upright: mode.into(), vertical: 0.0, horizontal: 0.0, rotate: 0.0, ..d.transform.clone() };
    if mode == "off" {
        return Ok(t);
    }
    let (w, h) = (img.w as f32, img.h as f32);
    let geo = Geo::new(&Develop { lens: d.lens.clone(), ..Develop::default() }, img.w, img.h);
    let guided = mode == "guided";
    let lines = if guided { d.transform.guides.iter().map(|g| (*g, 1.0)).collect() } else { find_lines(img) };
    // a guide is whichever it is closer to; a found line must be within 25° of it
    let tan = if guided { 1.0 } else { 25f32.to_radians().tan() };
    let mut segs: Vec<Seg> = lines
        .iter()
        .filter_map(|&([p, q], wt)| {
            let (a, b) = (geo.undistort(p[0] * w, p[1] * h), geo.undistort(q[0] * w, q[1] * h));
            let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
            if dx <= dy * tan {
                Some((a, b, wt, true))
            } else if dy <= dx * tan {
                Some((a, b, wt, false))
            } else {
                None
            }
        })
        .collect();
    if guided && segs.len() < 2 {
        return Err("draw at least two guides".into());
    }
    if segs.is_empty() {
        return Err("no straight edges found in this photo".into());
    }
    let norm = |segs: &mut Vec<Seg>| {
        let total: f32 = segs.iter().map(|s| s.2).sum();
        segs.iter_mut().for_each(|s| s.2 /= total);
    };
    norm(&mut segs);
    let (dof, reg) = match mode {
        "level" => ([false, false, true], 0.01),
        "vertical" => ([true, false, true], 0.01),
        "full" => ([true, true, true], 0.01),
        "guided" => ([segs.iter().any(|s| s.3), segs.iter().any(|s| !s.3), true], 0.001),
        _ => ([true, true, true], 0.08), // auto: balanced, smaller corrections
    };
    let f = FOCAL * w.max(h);
    let mut p = solve_upright(&segs, f, dof, reg);
    if !guided {
        // drop the lines the first solution disagrees with (diagonals, curves) and solve again
        let r = rotation(p[0], p[1], p[2]);
        let errs: Vec<f32> = segs.iter().map(|s| seg_err(&r, f, s).abs()).collect();
        let mut sorted = errs.clone();
        sorted.sort_by(f32::total_cmp);
        let cut = (3.0 * sorted[sorted.len() / 2]).max(0.03);
        let mut keep: Vec<Seg> = segs.iter().zip(&errs).filter(|(_, e)| **e <= cut).map(|(s, _)| *s).collect();
        if !keep.is_empty() && keep.len() < segs.len() {
            norm(&mut keep);
            p = solve_upright(&keep, f, dof, reg);
        }
    }
    t.vertical = (-p[0].to_degrees() / TILT_MAX * 100.0).clamp(-100.0, 100.0);
    t.horizontal = (-p[1].to_degrees() / TILT_MAX * 100.0).clamp(-100.0, 100.0);
    t.rotate = (-p[2].to_degrees()).clamp(-10.0, 10.0);
    Ok(t)
}

/// Auto lateral chromatic aberration: the red and blue scales about the centre that best line their
/// edges up with green in linear sRGB, as [ca_red_cyan, ca_blue_yellow]. Short radial profiles across strong edges
/// are correlated with green's after removing each profile's mean (so the colours on either side of
/// an edge don't matter), weighted by contrast; the best scale is refined with a parabola.
pub fn estimate_ca(img: &Img) -> [f32; 2] {
    let (w, h) = (img.w, img.h);
    let (cx, cy, rn) = (w as f32 / 2.0, h as f32 / 2.0, 4.0 / (w * w + h * h) as f32);
    let (mx, my) = ((w - 1) as f32, (h - 1) as f32);
    // softened planes: bilinear sampling of sharp edges would lock the match to whole pixels
    let planes: Vec<Vec<f32>> = (0..3).map(|c| blur(&img.px.par_iter().map(|p| mul(&M2020_709, *p)[c].max(0.0).sqrt()).collect::<Vec<f32>>(), w, h, 1.0)).collect();
    let at = |c: usize, x: f32, y: f32| {
        let (x, y) = ((cx + x - 0.5).clamp(0.0, mx), (cy + y - 0.5).clamp(0.0, my));
        let (x0, y0, v) = (x as usize, y as usize, &planes[c]);
        let (x1, y1, tx) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1), x - x0 as f32);
        let row = |y: usize| v[y * w + x0] + (v[y * w + x1] - v[y * w + x0]) * tx;
        row(y0) + (row(y1) - row(y0)) * (y - y0 as f32)
    };
    const K: [f32; 5] = [-2.0, -1.0, 0.0, 1.0, 2.0];
    let g = &planes[1];
    // strong radial edges in green away from the centre: position, radial unit vector, green profile
    let mut pts: Vec<([f32; 2], [f32; 2], [f32; 5])> = (0..w * h)
        .into_par_iter()
        .filter_map(|i| {
            let (x, y) = (i % w, i / w);
            if x < 3 || y < 3 || x + 3 >= w || y + 3 >= h {
                return None;
            }
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let r = dx.hypot(dy);
            let radial = ((g[i + 1] - g[i - 1]) * dx + (g[i + w] - g[i - w]) * dy).abs() / r.max(1e-3);
            if radial < 0.05 || r * r * rn < 0.1 {
                return None;
            }
            let u = [dx / r, dy / r];
            Some(([dx, dy], u, K.map(|k| at(1, dx + k * u[0], dy + k * u[1]))))
        })
        .collect();
    if pts.len() < 200 {
        return [0.0; 2];
    }
    let step = (pts.len() / 30_000).max(1);
    pts = pts.into_iter().step_by(step).collect();
    // centred cross and auto products of two profiles
    let moments = |a: &[f32; 5], b: &[f32; 5]| {
        let (ma, mb) = (a.iter().sum::<f32>() / 5.0, b.iter().sum::<f32>() / 5.0);
        (0..5).fold([0.0f64; 3], |m, k| {
            let (x, y) = ((a[k] - ma) as f64, (b[k] - mb) as f64);
            [m[0] + x * y, m[1] + x * x, m[2] + y * y]
        })
    };
    let best = |c: usize| {
        let scores: Vec<f32> = (-20..=20)
            .map(|i| {
                let s = 1.0 + i as f32 / 20.0 * CA_MAX;
                let m = pts
                    .par_iter()
                    .map(|(p, u, gp)| moments(&K.map(|k| at(c, (p[0] + k * u[0]) * s, (p[1] + k * u[1]) * s)), gp))
                    .reduce(|| [0.0; 3], |a, b| [a[0] + b[0], a[1] + b[1], a[2] + b[2]]);
                (m[0] / (m[1] * m[2]).sqrt().max(1e-12)) as f32
            })
            .collect();
        let i = (0..scores.len()).max_by(|&a, &b| scores[a].total_cmp(&scores[b])).unwrap_or(20);
        // parabola through the best score and its neighbours
        let off = match (scores.get(i.wrapping_sub(1)), scores.get(i + 1)) {
            (Some(&l), Some(&r)) if l + r - 2.0 * scores[i] < 0.0 => (0.5 * (l - r) / (l + r - 2.0 * scores[i])).clamp(-0.5, 0.5),
            _ => 0.0,
        };
        ((i as f32 - 20.0 + off) * 5.0).clamp(-100.0, 100.0)
    };
    [best(0), best(2)]
}

// ---------------------------------------------------------------------------
// Auto: white balance, tone and profile looks (measured on the full-frame proxy)
// ---------------------------------------------------------------------------

/// Profiles the Auto tab tunes to: auto white balance and tone, then a look.
pub const PROFILES: &[&str] = &["natural", "neutral", "vivid", "portrait", "landscape", "bw", "film", "moody"];

/// A profile's look on top of the auto result: numbers add to it, everything else is set.
fn look(profile: &str) -> Option<Value> {
    Some(match profile {
        "natural" => json!({ "presence": { "vibrance": 10 } }),
        "neutral" => json!({ "basic_tone": { "contrast": -20, "highlights": -15, "shadows": 15 }, "presence": { "vibrance": -10 } }),
        "vivid" => json!({ "basic_tone": { "contrast": 15 }, "presence": { "clarity": 10, "vibrance": 35, "saturation": 5 } }),
        "portrait" => json!({
            "white_balance": { "temperature_k": 200 },
            "basic_tone": { "exposure": 0.1, "contrast": -5, "highlights": -10, "shadows": 10 },
            "presence": { "texture": -15, "clarity": -10, "vibrance": 5 }
        }),
        "landscape" => json!({ "basic_tone": { "contrast": 10, "highlights": -25, "shadows": 15 }, "presence": { "clarity": 15, "dehaze": 10, "vibrance": 25 } }),
        "bw" => json!({ "profile": "Monochrome", "basic_tone": { "contrast": 20 }, "presence": { "clarity": 10 } }),
        "film" => json!({
            "basic_tone": { "contrast": -10 },
            "presence": { "saturation": -10 },
            "tone_curve": { "point_luma": [[0, 22], [64, 68], [192, 194], [255, 240]] },
            "color_grading_3way": { "shadows": { "hue": 200, "sat": 15 }, "highlights": { "hue": 40, "sat": 15 } },
            "grain": { "amount": 20 }
        }),
        "moody" => json!({
            "basic_tone": { "exposure": -0.3, "contrast": 15, "highlights": -20 },
            "presence": { "clarity": 10, "dehaze": 5, "saturation": -20 },
            "effects": { "vignette": -25 }
        }),
        _ => return None,
    })
}

/// Auto fine tune: white balance and tone measured on the photo, then `profile`'s look. White balance,
/// Basic, presence, colour / B&W and the panels the look uses start over, so a profile applied twice
/// changes nothing; the rest of the edit stays.
pub fn auto(img: &Img, cur: &Develop, profile: &str) -> Result<Develop, String> {
    look(profile).ok_or_else(|| format!("unknown profile '{profile}': use {}", PROFILES.join(", ")))?;
    let white_balance = auto_wb(img);
    let basic_tone = auto_tone(img, &white_balance);
    look_on(&Develop { white_balance, basic_tone, ..cur.clone() }, profile)
}

/// `profile`'s look on `d`: presence, colour / B&W and the panels the look uses start over, then its
/// numbers add to the rest (e.g. Portrait warms the white balance).
pub fn look_on(d: &Develop, profile: &str) -> Result<Develop, String> {
    let look = look(profile).ok_or_else(|| format!("unknown profile '{profile}': use {}", PROFILES.join(", ")))?;
    let d = Develop { presence: Presence::default(), profile: String::new(), ..d.clone() };
    let (mut v, fresh) = (serde_json::to_value(d).map_err(es)?, serde_json::to_value(Develop::default()).map_err(es)?);
    for k in look.as_object().into_iter().flat_map(|o| o.keys()).filter(|k| !["white_balance", "basic_tone"].contains(&k.as_str())) {
        v[k.as_str()] = fresh[k.as_str()].clone();
    }
    let d: Develop = serde_json::from_value(v).map_err(es)?;
    Ok(apply_patch(&d, &look, true)?.0)
}

/// About 40k pixels spread over the frame.
fn subsample(img: &Img) -> impl Iterator<Item = &Px> {
    img.px.iter().step_by((img.px.len() / 40_000).max(1))
}

/// Auto white balance: the average colour (grey world), refined to the pixels that are near grey under
/// it (ignoring very dark and clipped ones), then the temperature and tint whose gains make it grey. The
/// correction counts only as far as it turns a good share of the photo grey (colourful scenes such as
/// flowers keep their colour) and as the brightest pixels agree (white patch: a purple night sky lit by
/// white lightning keeps its purple); tint moves half as far, since real light varies mostly blue to yellow.
/// ponytail: grey-pixel heuristics; a learned illuminant estimate would handle scenes neither cue reads.
pub fn auto_wb(img: &Img) -> WhiteBalance {
    let px: Vec<Px> = subsample(img).copied().filter(|p| (0.02..0.95).contains(&p[0].max(p[1]).max(p[2]))).collect();
    let sat = |p: &Px, e: &Px| {
        let q = [p[0] / e[0], p[1] / e[1], p[2] / e[2]];
        1.0 - q[0].min(q[1]).min(q[2]).max(0.0) / q[0].max(q[1]).max(q[2]).max(1e-6)
    };
    let mut e = [1.0f32; 3];
    for pass in 0..3 {
        let mut acc = [0f64; 4];
        for p in &px {
            let w = if pass == 0 { 1.0 } else { (1.0 - sat(p, &e) / 0.3).max(0.0).powi(2) * luma(*p).max(0.0).sqrt() };
            for c in 0..3 {
                acc[c] += (w * p[c]) as f64;
            }
            acc[3] += w as f64;
        }
        if acc[3] <= 0.0 {
            break;
        }
        let m = [0, 1, 2].map(|c| (acc[c] / acc[3]) as f32);
        e = m.map(|v| (v / luma(m).max(1e-6)).max(1e-3));
    }
    let share = px.iter().filter(|p| sat(p, &e) < 0.12).count() as f32 / px.len().max(1) as f32;
    let mut bright: Vec<&Px> = px.iter().collect();
    bright.sort_by(|a, b| luma(**b).total_cmp(&luma(**a)));
    let top = &bright[..(bright.len() / 50).max(1).min(bright.len())];
    let w = [0, 1, 2].map(|c| top.iter().map(|p| p[c]).sum::<f32>().max(1e-6));
    let apart = ((e[0] / e[1]) / (w[0] / w[1])).ln().hypot(((e[2] / e[1]) / (w[2] / w[1])).ln());
    let e = e.map(|v| v.powf(smooth(0.05, 0.3, share) * (1.0 - smooth(0.08, 0.25, apart))));
    let err = |t: f32, tint: f32| {
        let g = wb_gains(t, tint);
        let q = [g[0] * e[0], g[1] * e[1], g[2] * e[2]];
        (q[0] / q[1]).ln().powi(2) + (q[2] / q[1]).ln().powi(2)
    };
    let mut best = (5500.0, 0.0, f32::MAX);
    for i in 0..=180 {
        let t = 2500.0 * (15000.0f32 / 2500.0).powf(i as f32 / 180.0);
        for j in -40..=40 {
            let e = err(t, j as f32 * 1.5);
            if e < best.2 {
                best = (t, j as f32 * 1.5, e);
            }
        }
    }
    WhiteBalance { temperature_k: (best.0 / 10.0).round() * 10.0, tint: (best.1 / 2.0).round() }
}

/// Auto tone, solved from the photo's percentiles with Tone's own maths: exposure moves a median outside
/// the normal range into it (so a well exposed photo keeps its key), highlights recover clipping or
/// whites stretch toward the brightest channel, blacks set the low end, shadows lift a dark low end and
/// contrast evens out the midtone spread; all damped like Lightroom's Auto.
pub fn auto_tone(img: &Img, wb: &WhiteBalance) -> BasicTone {
    let g = wb_gains(wb.temperature_k, wb.tint);
    let px: Vec<Px> = subsample(img).map(|p| [p[0] * g[0], p[1] * g[1], p[2] * g[2]]).collect();
    let pct = |mut v: Vec<f32>, qs: &[f32]| -> Vec<f32> {
        v.sort_by(f32::total_cmp);
        qs.iter().map(|q| v.get(((v.len().max(1) - 1) as f32 * q) as usize).copied().unwrap_or(0.0)).collect()
    };
    let lum: Vec<f32> = px.iter().map(|p| luma(*p).max(0.0)).collect();
    let med = pct(lum.clone(), &[0.5])[0].max(1e-4);
    let exposure = ((0.8 * (med.clamp(0.08, 0.35) / med).log2()).clamp(-0.75, 2.0) * 20.0).round() / 20.0;
    let k = exposure.exp2();
    // Tone works on sqrt values: the white end from the brightest channel, the rest from luma
    let hi = pct(px.iter().map(|p| (p[0].max(p[1]).max(p[2]) * k).max(0.0).sqrt()).collect(), &[0.997])[0];
    let v = pct(lum.iter().map(|l| (l * k).sqrt()).collect(), &[0.003, 0.1, 0.25, 0.75]);
    let mut t = BasicTone { exposure, ..Default::default() };
    if hi > 0.98 {
        t.highlights = (-167.0 * (hi / 0.98).log2()).max(-100.0);
    } else {
        t.whites = (0.6 * (1.0 - hi / 0.98) / 0.15 * 100.0).min(40.0);
    }
    let wp = 1.0 - 0.15 * t.whites / 100.0;
    t.blacks = (-0.6 * (v[0] - 0.02 * wp) / 0.98 / 0.15 * 100.0).clamp(-40.0, 10.0);
    let bp = -0.15 * t.blacks / 100.0;
    let at = |x: f32| (x - bp) / (wp - bp);
    let p10 = at(v[1]).max(1e-3);
    if p10 < 0.12 {
        t.shadows = (0.6 * 200.0 * (0.12 / p10).log2() / (1.8 * (1.0 - smooth(0.0, 0.5, p10)).max(0.1))).min(25.0);
    }
    t.contrast = (50.0 * (0.4 / (at(v[3]) - at(v[2])).max(0.05) - 1.0)).clamp(-15.0, 25.0);
    for s in [&mut t.contrast, &mut t.highlights, &mut t.shadows, &mut t.whites, &mut t.blacks] {
        *s = s.round();
    }
    // the end points and contrast moved the midtones: put the median back where exposure set it
    let tone = Tone::new(&t);
    let after = pct(px.iter().map(|p| luma(tone.apply(p.map(|c| c * k))).max(0.0)).collect(), &[0.5])[0].max(1e-4);
    t.exposure = ((exposure + 0.8 * (med * k / after).log2()).clamp(-0.75, 2.0) * 20.0).round() / 20.0;
    t
}

// ---------------------------------------------------------------------------
// Output: display buffers, histogram/telemetry, export
// ---------------------------------------------------------------------------

const LUT_N: usize = 16384;
static OETF_LUT: OnceLock<Vec<u8>> = OnceLock::new();

#[inline]
fn oetf8(l: f32) -> u8 {
    let lut = OETF_LUT
        .get_or_init(|| (0..=LUT_N).map(|i| (srgb_oetf(i as f32 / LUT_N as f32) * 255.0 + 0.5) as u8).collect());
    lut[(l.clamp(0.0, 1.0) * LUT_N as f32) as usize]
}

/// sRGB RGBA8 for display.
pub fn to_rgba8(img: &Img) -> Vec<u8> {
    img.px
        .par_iter()
        .flat_map_iter(|p| {
            let q = mul(&M2020_709, *p);
            [oetf8(q[0]), oetf8(q[1]), oetf8(q[2]), 255]
        })
        .collect()
}

/// Visualize Spots: high-pass luminance as white on black, so dust and specks stand out.
/// `threshold` 0-100: higher shows fainter detail.
pub fn visualize_spots(img: &mut Img, threshold: f32) {
    let (w, h) = (img.w, img.h);
    let v: Vec<f32> = img.px.par_iter().map(|p| luma(*p).max(0.0).sqrt()).collect();
    let b = blur(&v, w, h, (w.max(h) as f32 * 0.003).max(1.0));
    let t = 0.002 + (1.0 - threshold.clamp(0.0, 100.0) / 100.0) * 0.04;
    img.px.par_iter_mut().enumerate().for_each(|(i, p)| *p = [smooth(t, 2.0 * t, (v[i] - b[i]).abs()); 3]);
}

/// Soft-proofing overlays: colours outside `gamut` (sRGB | DisplayP3 | AdobeRGB) turn violet; with
/// `clipping`, clipped highlights turn red and crushed shadows blue (as the display shows them).
pub fn proof(img: &mut Img, gamut: Option<&str>, clipping: bool) {
    let m = match gamut {
        Some("sRGB") => Some(M2020_709),
        Some("DisplayP3") => Some(M2020_P3),
        Some("AdobeRGB") => Some(M2020_ADOBE),
        _ => None,
    };
    if m.is_none() && !clipping {
        return;
    }
    let paint = |c: Px| mul(&M709_2020, c);
    let (red, blue, violet) = (paint([1.0, 0.0, 0.0]), paint([0.0, 0.25, 1.0]), paint([0.75, 0.0, 1.0]));
    img.px.par_iter_mut().for_each(|p| {
        let s = mul(&M2020_709, *p);
        if clipping && s.iter().any(|&v| v >= 1.0) {
            *p = red;
        } else if clipping && s.iter().all(|&v| v < 1.5e-4) {
            *p = blue;
        } else if let Some(m) = &m {
            let q = mul(m, *p);
            if q.iter().any(|&v| v < -1e-3) || (luma(*p) < 1.0 && q.iter().any(|&v| v > 1.0 + 1e-3)) {
                *p = violet;
            }
        }
    });
}

/// 256-bin R, G, B and luma histograms of an RGBA8 buffer.
pub fn histogram(rgba: &[u8]) -> [[u32; 256]; 4] {
    let mut h = [[0u32; 256]; 4];
    for p in rgba.chunks_exact(4) {
        h[0][p[0] as usize] += 1;
        h[1][p[1] as usize] += 1;
        h[2][p[2] as usize] += 1;
        h[3][((54 * p[0] as u32 + 183 * p[1] as u32 + 19 * p[2] as u32) >> 8) as usize] += 1;
    }
    h
}

/// Spec §4.2 telemetry: histograms, clipping, luminance stats and dominant Oklch colours.
pub fn telemetry(img: &Img) -> Value {
    let rgba = to_rgba8(img);
    let h = histogram(&rgba);
    let n = (img.w * img.h).max(1) as f64;
    let (mut hi, mut lo) = (0u64, 0u64);
    for p in rgba.chunks_exact(4) {
        let mx = p[0].max(p[1]).max(p[2]);
        hi += (p[0] == 255 || p[1] == 255 || p[2] == 255) as u64;
        lo += (mx == 0) as u64;
    }
    let pct = |q: f64| {
        let mut acc = 0.0;
        for (i, c) in h[3].iter().enumerate() {
            acc += *c as f64;
            if acc >= q * n {
                return i;
            }
        }
        255
    };
    let mean = h[3].iter().enumerate().map(|(i, c)| i as f64 * *c as f64).sum::<f64>() / n;
    // dominant colours: 12 Oklch hue bins over chromatic pixels
    let mut bins = [[0f64; 5]; 12]; // count, L, C, cos h, sin h
    let mut total = 0f64;
    for p in img.px.iter().step_by(7) {
        let [l, a, b] = oklab(p.map(|c| c.clamp(0.0, 1.0)));
        let c = a.hypot(b);
        total += 1.0;
        if c < 0.03 {
            continue;
        }
        let hh = b.atan2(a);
        let k = ((hh.to_degrees().rem_euclid(360.0) / 30.0) as usize).min(11);
        for (s, v) in bins[k].iter_mut().zip([1.0, l, c, hh.cos(), hh.sin()]) {
            *s += v as f64;
        }
    }
    let mut dom: Vec<&[f64; 5]> = bins.iter().filter(|b| b[0] > 0.0).collect();
    dom.sort_by(|a, b| b[0].total_cmp(&a[0]));
    let r1 = |v: f64| (v * 10.0).round() / 10.0;
    let dominant: Vec<Value> = dom
        .iter()
        .take(5)
        .map(|b| {
            json!({
                "L": r1(b[1] / b[0] * 100.0),
                "C": (b[2] / b[0] * 1000.0).round() / 1000.0,
                "h": r1(b[4].atan2(b[3]).to_degrees().rem_euclid(360.0)),
                "share_pct": r1(b[0] / total.max(1.0) * 100.0),
            })
        })
        .collect();
    json!({
        "width": img.w,
        "height": img.h,
        "histogram": { "r": h[0].to_vec(), "g": h[1].to_vec(), "b": h[2].to_vec(), "luma": h[3].to_vec() },
        "clipping": { "highlights_pct": r1(hi as f64 / n * 100.0), "shadows_pct": r1(lo as f64 / n * 100.0) },
        "luma": { "mean": r1(mean), "p1": pct(0.01), "p50": pct(0.5), "p99": pct(0.99) },
        "dominant_oklch": dominant,
    })
}

/// sRGB JPEG bytes, downscaled to `max_edge` if larger.
pub fn encode_jpeg(img: &Img, max_edge: u32, quality: u8) -> Result<Vec<u8>, String> {
    let buf = image::RgbaImage::from_raw(img.w as u32, img.h as u32, to_rgba8(img)).ok_or("bad buffer")?;
    let mut d = DynamicImage::ImageRgba8(buf);
    if d.width().max(d.height()) > max_edge {
        d = d.resize(max_edge, max_edge, FilterType::Triangle);
    }
    let mut out = vec![];
    d.to_rgb8().write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)).map_err(es)?;
    Ok(out)
}

pub const EXPORT_FORMATS: &[&str] = &["JPEG", "AVIF", "PNG_16", "TIFF_16"];

pub fn file_ext(format: &str) -> &'static str {
    match format {
        "AVIF" => "avif",
        "PNG_16" => "png",
        "TIFF_16" => "tif",
        _ => "jpg",
    }
}

/// Minimal big-endian EXIF block: IFD0 → Exif IFD → DateTimeOriginal. `taken` is the
/// catalog's "YYYY-MM-DD HH:MM:SS" (local time, as read on import); anything else gives None.
fn exif_taken(taken: &str) -> Option<Vec<u8>> {
    let shape = |(i, c): (usize, &u8)| match i {
        4 | 7 => *c == b'-',
        10 => *c == b' ',
        13 | 16 => *c == b':',
        _ => c.is_ascii_digit(),
    };
    if taken.len() != 19 || !taken.as_bytes().iter().enumerate().all(shape) || taken.starts_with("0000") {
        return None;
    }
    let mut e = b"MM\0\x2a\0\0\0\x08".to_vec(); // TIFF header, IFD0 at 8
    e.extend([0, 1, 0x87, 0x69, 0, 4, 0, 0, 0, 1, 0, 0, 0, 26, 0, 0, 0, 0]); // IFD0: ExifIFDPointer (LONG) → 26
    e.extend([0, 1, 0x90, 0x03, 0, 2, 0, 0, 0, 20, 0, 0, 0, 44, 0, 0, 0, 0]); // Exif IFD: DateTimeOriginal (ASCII[20]) at 44
    e.extend(taken.replacen('-', ":", 2).as_bytes());
    e.push(0);
    Some(e)
}

/// Best effort: an encoder that can't carry ICC or EXIF (TIFF has no EXIF here) still writes the pixels.
fn tagged<E: ImageEncoder>(mut e: E, icc: Option<Vec<u8>>, exif: Option<Vec<u8>>) -> E {
    if let Some(i) = icc {
        let _ = e.set_icc_profile(i);
    }
    if let Some(x) = exif {
        let _ = e.set_exif_metadata(x);
    }
    e
}

/// A master in linear light: 32-bit float TIFF in Rec.709 primaries that keeps values below 0 (wide-gamut
/// colours) and above 1 (highlights); `load` reads float files back as linear.
pub fn export_linear(img: &Img, out: &Path) -> Result<(), String> {
    let data: Vec<f32> = img.px.par_iter().flat_map_iter(|p| mul(&M2020_709, *p)).collect();
    let buf: image::Rgb32FImage = ImageBuffer::from_raw(img.w as u32, img.h as u32, data).ok_or("bad buffer")?;
    let file = std::io::BufWriter::new(std::fs::File::create(out).map_err(es)?);
    DynamicImage::ImageRgb32F(buf).write_with_encoder(image::codecs::tiff::TiffEncoder::new(file)).map_err(es)
}

/// Encode with an embedded ICC profile and, when the capture time is known, an EXIF
/// DateTimeOriginal (JPEG, PNG, AVIF). Rec2020_HDR writes PQ-encoded 16-bit files.
pub fn export(img: &Img, out: &Path, format: &str, color_space: &str, quality: u8, taken: Option<&str>) -> Result<(), String> {
    use image::codecs::{avif::AvifEncoder, jpeg::JpegEncoder, png::PngEncoder, tiff::TiffEncoder};
    use moxcms::ColorProfile;
    let hdr = color_space == "Rec2020_HDR";
    let (m, profile) = match color_space {
        "sRGB" | "" => (M2020_709, ColorProfile::new_srgb()),
        "DisplayP3" => (M2020_P3, ColorProfile::new_display_p3()),
        "AdobeRGB" => (M2020_ADOBE, ColorProfile::new_adobe_rgb()),
        "Rec2020_HDR" => (IDENT, ColorProfile::new_bt2020_pq()),
        other => return Err(format!("unsupported color_space '{other}'")),
    };
    if hdr && !matches!(format, "PNG_16" | "TIFF_16") {
        return Err("Rec2020_HDR export needs PNG_16 or TIFF_16".into());
    }
    let adobe = color_space == "AdobeRGB";
    let enc: Vec<f32> = img
        .px
        .par_iter()
        .flat_map_iter(|p| {
            mul(&m, *p).map(|l| {
                if hdr {
                    pq(l)
                } else if adobe {
                    l.clamp(0.0, 1.0).powf(1.0 / 2.199_219)
                } else {
                    srgb_oetf(l.clamp(0.0, 1.0))
                }
            })
        })
        .collect();
    let (w, h) = (img.w as u32, img.h as u32);
    let icc = profile.encode().ok();
    let exif = taken.and_then(exif_taken);
    let file = std::io::BufWriter::new(std::fs::File::create(out).map_err(es)?);
    let to8 = || ImageBuffer::from_raw(w, h, enc.iter().map(|v| (v * 255.0 + 0.5) as u8).collect()).map(DynamicImage::ImageRgb8);
    let to16 = || ImageBuffer::from_raw(w, h, enc.iter().map(|v| (v * 65535.0 + 0.5) as u16).collect()).map(DynamicImage::ImageRgb16);
    let res = match format {
        "JPEG" => to8().ok_or("buffer")?.write_with_encoder(tagged(JpegEncoder::new_with_quality(file, quality), icc, exif)),
        "AVIF" => to8().ok_or("buffer")?.write_with_encoder(tagged(AvifEncoder::new_with_speed_quality(file, 6, quality), None, exif)),
        "PNG_16" => to16().ok_or("buffer")?.write_with_encoder(tagged(PngEncoder::new(file), icc, exif)),
        "TIFF_16" => to16().ok_or("buffer")?.write_with_encoder(tagged(TiffEncoder::new(file), icc, exif)),
        f => return Err(format!("format '{f}' is not supported in this build ({})", EXPORT_FORMATS.join(", "))),
    };
    res.map_err(es)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> Img {
        let (w, h) = (64, 48);
        let px = (0..w * h)
            .map(|i| srgb_to_working([(i % w) as f32 / w as f32, (i / w) as f32 / h as f32, 0.5]))
            .collect();
        Img { w, h, px }
    }

    #[test]
    fn default_develop_is_identity() {
        let src = gradient();
        let out = render(&src, &Develop::default(), &Opts::default());
        let max_diff = out.px.iter().zip(&src.px).flat_map(|(a, b)| (0..3).map(move |c| (a[c] - b[c]).abs())).fold(0.0, f32::max);
        assert!(max_diff < 1e-5, "default develop changed pixels by {max_diff}");
        // sRGB -> working -> sRGB round-trips 8-bit values
        let rgba = to_rgba8(&out);
        assert!((rgba[4 * 10] as i32 - (10.0 / 64.0 * 255.0) as i32).abs() <= 1);
    }

    #[test]
    fn exports_carry_the_capture_date() {
        let dir = std::env::temp_dir().join(format!("ols-exif-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (format, ext) in [("JPEG", "jpg"), ("PNG_16", "png"), ("AVIF", "avif")] {
            let out = dir.join(format!("taken.{ext}"));
            export(&gradient(), &out, format, "sRGB", 90, Some("2024-05-01 12:34:56")).unwrap();
            let mut file = std::io::BufReader::new(std::fs::File::open(&out).unwrap());
            let ex = exif::Reader::new().read_from_container(&mut file).unwrap();
            let date = ex.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY).map(|f| f.display_value().to_string());
            assert_eq!(date.as_deref(), Some("2024-05-01 12:34:56"), "{format}");
        }
        assert_eq!(exif_taken("0000-00-00 00:00:00"), None);
        assert_eq!(exif_taken("2024:05:01 12:34:56"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tone_is_monotone_at_extremes() {
        for (sh, hi) in [(100.0, -100.0), (-100.0, 100.0), (100.0, 100.0), (-100.0, -100.0)] {
            let t = Tone::new(&BasicTone { shadows: sh, highlights: hi, contrast: 100.0, ..Default::default() });
            let mut prev = -1.0;
            for i in 0..=400 {
                let v = i as f32 / 200.0;
                let y = t.apply([v, v, v])[0];
                assert!(y >= prev - 1e-5, "non-monotone at {v} ({sh},{hi})");
                prev = y;
            }
        }
    }

    #[test]
    fn patch_merges_aliases_clamps_and_reports_unknown() {
        let patch = json!({"tone": {"exposure": 9.0, "shadows": 20}, "hdr_mode": true});
        let (d, unknown) = apply_patch(&Develop::default(), &patch, false).unwrap();
        assert_eq!(d.basic_tone.exposure, 5.0);
        assert_eq!(d.basic_tone.shadows, 20.0);
        assert_eq!(unknown, vec!["hdr_mode".to_string()]);
        let (d2, _) = apply_patch(&d, &json!({"tone": {"shadows": 5}}), true).unwrap();
        assert_eq!(d2.basic_tone.shadows, 25.0);
    }

    #[test]
    fn edits_change_pixels_and_masks_localise() {
        let src = gradient();
        let mut d = Develop::default();
        d.masks.push(Mask {
            components: vec![MaskComponent { kind: "linear_gradient".into(), start: [0.5, 0.0], end: [0.5, 0.3], ..Default::default() }],
            adjustments: LocalAdjust { exposure: 1.0, ..Default::default() },
            ..Default::default()
        });
        let out = render(&src, &d, &Opts::default());
        let (top, bottom) = (5 * src.w + 10, (src.h - 2) * src.w + 10);
        assert!(luma(out.px[top]) > luma(src.px[top]) * 1.5);
        assert!((luma(out.px[bottom]) - luma(src.px[bottom])).abs() < 1e-6);
        assert!(mask_from_ops("sky", &json!([{"boolean_op": "add", "source_type": "ai_sky"}]), &json!({})).is_err());
    }

    #[test]
    fn mask_tuning_applies_inside_only() {
        let src = gradient();
        let mut t = Develop { profile: "Monochrome".into(), ..Default::default() };
        t.basic_tone.exposure = 1.0;
        let mut d = Develop::default();
        d.masks.push(Mask {
            components: vec![MaskComponent { kind: "linear_gradient".into(), start: [0.5, 0.0], end: [0.5, 0.3], ..Default::default() }],
            tuning: Some(t),
            ..Default::default()
        });
        let out = render(&src, &d, &Opts::default());
        let (top, bottom) = (5 * src.w + 10, (src.h - 2) * src.w + 10);
        let chroma = |p: Px| oklab(p)[1].hypot(oklab(p)[2]);
        assert!(luma(out.px[top]) > luma(src.px[top]) * 1.5, "brighter inside");
        assert!(chroma(out.px[top]) < 0.6 * chroma(src.px[top]), "B&W inside");
        assert!((0..3).all(|c| (out.px[bottom][c] - src.px[bottom][c]).abs() < 1e-5), "untouched outside");
        // a tuning keeps no geometry or masks of its own, and default settings are dropped
        d.masks[0].tuning = Some(Develop { masks: d.masks.clone(), crop: Crop { w: 0.5, ..Default::default() }, ..Default::default() });
        d.clamp();
        assert_eq!(d.masks[0].tuning, None);
    }

    #[test]
    fn polygon_mask_snaps_to_the_object() {
        // bright square on a dark ground, outlined 4px too generously
        let (w, h) = (400, 300);
        let px = (0..w * h)
            .map(|i| if (150..250).contains(&(i % w)) && (100..200).contains(&(i / w)) { [0.6, 0.5, 0.3] } else { [0.02, 0.03, 0.08] })
            .collect::<Vec<Px>>();
        let pts = [[146.0, 96.0], [254.0, 96.0], [254.0, 204.0], [146.0, 204.0]].map(|[x, y]| [x / w as f32, y / h as f32]);
        let m = Mask { components: vec![MaskComponent { kind: "polygon".into(), points: pts.to_vec(), ..Default::default() }], ..Default::default() };
        let wts = mask_weights(&m, w, h, &px);
        let at = |x: usize, y: usize| wts[y * w + x];
        assert!(at(200, 150) > 0.99 && at(153, 150) > 0.9, "inside: {} {}", at(200, 150), at(153, 150));
        assert!(at(147, 150) < 0.2 && at(200, 97) < 0.2, "edge not snapped: {} {}", at(147, 150), at(200, 97));
        assert_eq!(at(20, 20), 0.0);
        let op = |p: Value| json!([{"boolean_op": "add", "source_type": "polygon", "points": p}]);
        let (castle, _) = mask_from_ops("castle", &op(json!([[0.1, 0.1], [0.9, 0.1], [0.5, 0.8]])), &json!({})).unwrap();
        assert_eq!(castle.components[0].name, "castle", "an AI region is tagged with what it is");
        assert!(mask_from_ops("castle", &op(json!([[0.1, 0.1], [0.9, 0.1]])), &json!({})).is_err());
        assert!(mask_from_ops("castle", &op(json!([[10, 20], [300, 20], [300, 200]])), &json!({})).is_err());
    }

    #[test]
    fn heal_removes_an_object() {
        // horizontal stripes over a vertical gradient, with a red blob in the middle
        let (w, h) = (320, 240);
        let ground = |y: usize| [0.2 + 0.4 * y as f32 / h as f32 + if (y / 6) % 2 == 0 { 0.05 } else { 0.0 }; 3];
        let blob = |x: usize, y: usize| (x as f32 - 160.0).hypot(y as f32 - 120.0) < 15.0;
        let px = (0..w * h).map(|i| if blob(i % w, i / w) { [0.9, 0.05, 0.05] } else { ground(i / w) }).collect();
        let img = Img { w, h, px };
        let pts: Vec<[f32; 2]> = (0..16)
            .map(|k| {
                let a = k as f32 / 16.0 * 2.0 * PI;
                [(160.0 + 19.0 * a.cos()) / w as f32, (120.0 + 19.0 * a.sin()) / h as f32]
            })
            .collect();
        let source = heal_source(&img, &pts, &[]).expect("a source area");
        let d = Develop { retouch: vec![Heal { name: "blob".into(), points: pts, source }], ..Default::default() };
        let out = render(&img, &d, &Opts::default());
        let err = (100..140)
            .flat_map(|y| (140..180).map(move |x| (x, y)))
            .map(|(x, y)| (0..3).map(|c| (out.px[y * w + x][c] - ground(y)[c]).abs()).fold(0.0, f32::max))
            .fold(0.0, f32::max);
        assert!(err < 0.03, "healed area is off the ground by {err} (source {source:?})");
    }

    #[test]
    fn upright_recovers_a_camera_tilt() {
        // a facade's verticals seen by a camera tilted up 12°: Vertical corrects them back
        let f = FOCAL * 1600.0;
        let cam = rotation(-12f32.to_radians(), 0.0, 0.0);
        let see = |x: f32, y: f32| {
            let q = mul(&cam, [x, y, f]);
            [f * q[0] / q[2], f * q[1] / q[2]]
        };
        let segs: Vec<Seg> = [-500.0, -200.0, 250.0, 600.0].iter().map(|&x| (see(x, -400.0), see(x, 400.0), 0.25, true)).collect();
        let p = solve_upright(&segs, f, [true, false, true], 0.001);
        assert!((p[0].to_degrees() - 12.0).abs() < 0.3 && p[2].to_degrees().abs() < 0.3, "{p:?}");
    }

    #[test]
    fn auto_level_straightens_tilted_lines() {
        // bands tilted 3° clockwise: Level finds their edges and rotates 3° back
        let (w, h) = (900, 600);
        let a = 3f32.to_radians();
        let px = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 - 450.0, (i / w) as f32 - 300.0);
                [0.35 + 0.25 * (8.0 * (PI * (-x * a.sin() + y * a.cos()) / 60.0).sin()).clamp(-1.0, 1.0); 3]
            })
            .collect();
        let img = Img { w, h, px };
        assert!(find_lines(&img).len() >= 4);
        let t = upright(&img, &Develop::default(), "level").unwrap();
        assert!((t.rotate - 3.0).abs() < 0.3 && t.vertical == 0.0 && t.horizontal == 0.0, "{t:?}");
        assert!(upright(&img, &Develop::default(), "guided").is_err(), "guided needs two guides");
    }

    #[test]
    fn transform_constrains_the_crop() {
        let src = Img { w: 200, h: 150, px: vec![[0.5; 3]; 200 * 150] };
        let mut d = Develop::default();
        d.transform.vertical = -60.0;
        d.lens.distortion = 40.0;
        let out = render(&src, &d, &Opts::default());
        assert!(out.w < 200 && out.px.iter().all(|p| p[0] > 0.4), "empty area inside a constrained crop");
        d.transform.constrain_crop = false;
        let out = render(&src, &d, &Opts::default());
        assert!(out.w == 200 && out.px.iter().any(|p| p[0] == 0.0), "an unconstrained keystone shows its empty wedges");
        let full = render(&src, &d, &Opts { full_frame: true, ..Default::default() });
        assert!(full.w == 200 && full.px.iter().all(|p| (p[0] - 0.5).abs() < 1e-4), "masks see the untransformed frame");
    }

    #[test]
    fn auto_ca_measures_lateral_fringes() {
        // a JPEG whose red channel is 0.15% larger than green and blue
        let (w, h) = (800, 600);
        let pattern = |x: f32, y: f32| 0.45 + 0.35 * (x / 6.0).sin() * (y / 6.0).sin();
        let px = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32 + 0.5 - 400.0, (i / w) as f32 + 0.5 - 300.0);
                srgb_to_working([pattern(x / 1.0015, y / 1.0015), pattern(x, y), pattern(x, y)])
            })
            .collect();
        let img = Img { w, h, px };
        let [rc, by] = estimate_ca(&img);
        assert!((rc - 75.0).abs() < 5.0 && by.abs() < 5.0, "{rc} {by}");
        // and the correction lines them up again
        let d = Develop { lens: Lens { ca_red_cyan: 75.0, ..Default::default() }, ..Default::default() };
        let [rc, by] = estimate_ca(&render(&img, &d, &Opts::default()));
        assert!(rc.abs() < 5.0 && by.abs() < 5.0, "after correction {rc} {by}");
    }

    #[test]
    fn defringe_calibration_and_grain() {
        // a purple fringe along a black/white edge goes grey; a purple patch on flat grey keeps its colour
        let (w, h) = (200, 100);
        let purple = srgb_to_working([0.6, 0.2, 0.8]);
        let px = (0..w * h)
            .map(|i| match (i % w, i / w) {
                (x, _) if x < 50 => [0.0; 3],
                (x, _) if x < 52 => purple,
                (x, _) if x < 100 => [0.9; 3],
                (x, y) if (130..170).contains(&x) && (30..70).contains(&y) => purple,
                _ => [0.3; 3],
            })
            .collect();
        let src = Img { w, h, px };
        let chroma = |p: Px| oklab(p)[1].hypot(oklab(p)[2]);
        let mut d = Develop::default();
        d.lens.defringe_purple = 10.0;
        let out = render(&src, &d, &Opts::default());
        assert!(chroma(out.px[50 * w + 51]) < 0.3 * chroma(purple), "fringe kept its colour");
        assert!(chroma(out.px[50 * w + 150]) > 0.95 * chroma(purple), "a purple object lost its colour");
        // calibration keeps neutrals neutral
        let d = Develop { calibration: Calibration { red_hue: 60.0, green_saturation: -40.0, blue_hue: -30.0, ..Default::default() }, ..Default::default() };
        assert!(chroma(render(&src, &d, &Opts::default()).px[50 * w + 190]) < 1e-3);
        // grain is deterministic and changes the pixels
        let d = Develop { grain: Grain { amount: 50.0, ..Default::default() }, ..Default::default() };
        let (a, b) = (render(&src, &d, &Opts::default()), render(&src, &d, &Opts::default()));
        assert!(a.px == b.px && a.px != src.px);
    }

    #[test]
    fn touching_outlines_heal_as_one() {
        let sq = |x: f32, y: f32| vec![[x, y], [x + 0.05, y], [x + 0.05, y + 0.1], [x, y + 0.1]];
        let (a, b) = (sq(0.3, 0.5), sq(0.36, 0.52));
        let g = group_outlines(vec![("a".into(), a.clone()), ("far".into(), sq(0.8, 0.1)), ("b".into(), b.clone())], 1600, 1068);
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].0.as_str(), g[1].0.as_str()), ("far", "a + b"));
        assert_eq!(g[0].1, sq(0.8, 0.1));
        // hull of both squares: the 6 outer corners
        assert_eq!(g[1].1.len(), 6);
        assert!(g[1].1.contains(&a[0]) && g[1].1.contains(&b[2]) && !g[1].1.contains(&b[0]));
    }

    #[test]
    fn pro_adjustments_and_image_rotation() {
        let src = gradient();
        let run = |d: &Develop| render(&src, d, &Opts::default());
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        // levels black 51 / white 204 stretch every display value
        let mut d = Develop::default();
        d.levels.rgb = Level { black: 51.0, white: 204.0, ..Default::default() };
        let out = run(&d);
        for i in [0, 700, 3000] {
            let (a, b) = (enc(src.px[i]), enc(out.px[i]));
            assert!((0..3).all(|c| near(b[c], ((a[c] * 255.0 - 51.0) / 153.0).clamp(0.0, 1.0))), "{a:?} -> {b:?}");
        }
        // saturation -100, a black-to-white gradient map and a monochrome mixer all give greys
        let grey = |p: &Px| {
            let e = enc(*p);
            near(e[0], e[1]) && near(e[1], e[2])
        };
        for d in [
            Develop { hue_saturation: HueSaturation { saturation: -100.0, ..Default::default() }, ..Default::default() },
            Develop { gradient_map: GradientMap { amount: 100.0, ..Default::default() }, ..Default::default() },
            Develop { channel_mixer: ChannelMixer { monochrome: true, ..Default::default() }, ..Default::default() },
        ] {
            assert!(run(&d).px.iter().all(grey));
        }
        // selective color: less cyan in the reds makes a red redder and leaves grey alone
        let d = Develop { selective_color: SelectiveColor { reds: [-100.0, 0.0, 0.0, 0.0], ..Default::default() }, ..Default::default() };
        let img = Img { w: 2, h: 1, px: vec![srgb_to_working([0.8, 0.2, 0.2]), srgb_to_working([0.4; 3])] };
        let out = render(&img, &d, &Opts::default());
        assert!(enc(out.px[0])[0] > 0.9 && near(enc(out.px[1])[0], 0.4));
        // threshold leaves black and white only
        let d = Develop { filters: Filters { threshold: 128.0, ..Default::default() }, ..Default::default() };
        assert!(run(&d).px.iter().all(|p| enc(*p).iter().all(|&v| v < 1e-3 || v > 0.999)));
        // 90° clockwise: the top-left comes from the bottom-left; a flip mirrors; the crop is in the rotated frame
        let same = |a: Px, b: Px| (0..3).all(|c| (a[c] - b[c]).abs() < 1e-4);
        let mut d = Develop { image_rotation: ImageRotation { angle: 90.0, ..Default::default() }, ..Default::default() };
        let out = run(&d);
        assert_eq!((out.w, out.h), (src.h, src.w));
        assert!(same(out.px[0], src.px[(src.h - 1) * src.w]));
        d.crop = Crop { w: 0.5, ..Default::default() };
        assert_eq!((run(&d).w, run(&d).h), (src.h / 2, src.w));
        let d = Develop { image_rotation: ImageRotation { flip_h: true, ..Default::default() }, ..Default::default() };
        assert!(same(run(&d).px[0], src.px[src.w - 1]));
    }

    #[test]
    fn ai_selected_objects_are_separate_masks() {
        let mut tuned = Develop::default();
        tuned.basic_tone.exposure = 1.0;
        let mut d = Develop {
            masks: vec![Mask {
                mask_id: "mask_1".into(),
                name: "castle".into(),
                enabled: true,
                components: vec![
                    MaskComponent { op: "subtract".into(), kind: "polygon".into(), name: "castle".into(), points: vec![[0.1, 0.1], [0.5, 0.1], [0.5, 0.5]], ..Default::default() },
                    MaskComponent { op: "add".into(), kind: "polygon".into(), name: "sky".into(), points: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 0.3]], ..Default::default() },
                ],
                tuning: Some(tuned),
                ..Default::default()
            }],
            ..Default::default()
        };
        d.clamp();
        assert_eq!(d.masks.len(), 2);
        assert_eq!(d.masks[0].name, "castle");
        assert_eq!(d.masks[0].components.len(), 1);
        assert_eq!(d.masks[0].components[0].op, "add");
        assert!(d.masks[0].tuning.is_some());
        assert_eq!(d.masks[1].name, "sky");
        assert_eq!(d.masks[1].components.len(), 1);
        assert_eq!(d.masks[1].components[0].op, "add");
        assert!(d.masks[1].tuning.is_none());
    }

    #[test]
    fn background_polygon_does_not_bleed_into_foreground_subject() {
        let (w, h) = (200, 200);
        let sky = srgb_to_working([0.2, 0.5, 0.85]);
        let warm = srgb_to_working([0.75, 0.58, 0.42]); // shared by castle stone and foreground face
        let px: Vec<Px> = (0..w * h)
            .map(|i| if i / w < 60 { sky } else { warm })
            .collect();
        // Castle polygon with a narrow U-shaped notch carved around a foreground person's head (x: 0.47..0.53, y: 0.55..0.80)
        let castle_pts = vec![
            [0.10, 0.30],
            [0.90, 0.30],
            [0.90, 0.80],
            [0.53, 0.80],
            [0.53, 0.55],
            [0.47, 0.55],
            [0.47, 0.80],
            [0.10, 0.80],
        ];
        let (win, matte) = object_matte(&castle_pts, &px, w, h, 0.5, 0.0);
        // Centre of the person's head inside the U-shaped notch (x = 100, y = 125) must stay unselected (0.0)
        let idx = (125 - win.y0) * win.w + (100 - win.x0);
        assert!(matte[idx] < 0.01, "foreground head inside U-notch bled into castle matte: {}", matte[idx]);
    }

    #[test]
    fn auto_neutralises_a_cast_brightens_and_profiles_repeat() {
        // grey steps over colour patches, underexposed under the warm light a 3200K white balance corrects
        let (w, h) = (120, 80);
        let light = wb_gains(3200.0, 0.0).map(|g| 0.12 / g);
        let colours = [[0.7, 0.2, 0.2], [0.2, 0.6, 0.2], [0.2, 0.3, 0.7], [0.8, 0.7, 0.2], [0.3, 0.6, 0.7], [0.6, 0.3, 0.6]];
        let px = (0..w * h)
            .map(|i| {
                let (k, y) = ((i % w) / 20, i / w);
                let c = if y < 40 { [0.05 + 0.15 * k as f32; 3] } else { srgb_to_working(colours[k]) };
                [c[0] * light[0], c[1] * light[1], c[2] * light[2]]
            })
            .collect();
        let img = Img { w, h, px };
        let d = auto(&img, &Develop::default(), "natural").unwrap();
        assert!((d.white_balance.temperature_k - 3200.0).abs() < 150.0, "{:?}", d.white_balance);
        let out = render(&img, &d, &Opts::default());
        for x in [30, 70, 110] {
            let q = oklab(out.px[20 * w + x]);
            assert!(q[1].hypot(q[2]) < 0.01, "grey patch at {x} kept a cast: {q:?}");
        }
        let mean = |im: &Img| im.px.iter().map(|p| luma(*p)).sum::<f32>() / im.px.len() as f32;
        let median = |im: &Img| {
            let mut l: Vec<f32> = im.px.iter().map(|p| luma(*p)).collect();
            l.sort_by(f32::total_cmp);
            l[l.len() / 2]
        };
        let (m0, m1) = (median(&img), median(&out));
        assert!(d.basic_tone.exposure > 0.5 && m1 > 1.5 * m0 && (0.08..0.3).contains(&m1), "median {m0} -> {m1}, mean {} -> {}, {:?}", mean(&img), mean(&out), d.basic_tone);
        // a profile applied twice changes nothing, and edits outside it stay
        let mut cur = Develop::default();
        cur.levels.rgb.black = 20.0;
        let film = auto(&img, &cur, "film").unwrap();
        assert_eq!(auto(&img, &film, "film").unwrap(), film);
        assert_eq!((film.levels.rgb.black, film.grain.amount), (20.0, 20.0));
        assert_eq!(auto(&img, &film, "natural").unwrap().grain.amount, 20.0, "natural leaves the film grain alone");
        assert!(auto(&img, &cur, "bw").unwrap().profile == "Monochrome" && auto(&img, &cur, "nope").is_err());
    }
}
