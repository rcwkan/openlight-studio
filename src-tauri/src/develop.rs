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

type M3 = [[f32; 3]; 3];
type Px = [f32; 3];

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
    pub detail: Detail,
    pub effects: Effects,
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
}
impl Default for Detail {
    fn default() -> Self {
        Self { sharpening: 0.0, sharpen_radius: 1.0, sharpen_masking: 0.0, color_noise_reduction: 0.0 }
    }
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
    /// effects, removals and masks of its own are ignored.
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
        let e = &mut self.effects;
        cl(&mut e.vignette, -100.0, 100.0);
        cl(&mut e.vignette_midpoint, 0.0, 100.0);
        cl(&mut e.vignette_feather, 0.0, 100.0);
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

const M709_2020: M3 = [[0.6274039, 0.3292830, 0.0433131], [0.0690973, 0.9195404, 0.0113623], [0.0163914, 0.0880133, 0.8955953]];
const M2020_709: M3 = [[1.6604910, -0.5876411, -0.0728499], [-0.1245505, 1.1328999, -0.0083494], [-0.0181508, -0.1005789, 1.1187297]];
const M2020_P3: M3 = [[1.3435783, -0.2821797, -0.0613986], [-0.0652975, 1.0757879, -0.0104905], [0.0028218, -0.0195985, 1.0167767]];
const M2020_ADOBE: M3 = [[1.1519784, -0.0975031, -0.0544753], [-0.1245505, 1.1328999, -0.0083494], [-0.0225304, -0.0498065, 1.0723369]];
const MXYZ_2020: M3 = [[1.7166513, -0.3556708, -0.2533663], [-0.6666843, 1.6164812, 0.0157686], [0.0176399, -0.0427706, 0.9421031]];
const M2020_LMS: M3 = [[0.6166884, 0.3601591, 0.0230433], [0.2651402, 0.6358565, 0.0990302], [0.1001506, 0.2040043, 0.6963247]];
const MLMS_2020: M3 = [[2.1401404, -1.2463560, 0.1064317], [-0.8848325, 2.1631727, -0.2783616], [-0.0485791, -0.4544909, 1.5023563]];
const IDENT: M3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[inline]
fn mul(m: &M3, p: Px) -> Px {
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
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
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
fn oklab(p: Px) -> Px {
    let l = mul(&M2020_LMS, p).map(f32::cbrt);
    [
        0.2104542553 * l[0] + 0.7936177850 * l[1] - 0.0040720468 * l[2],
        1.9779984951 * l[0] - 2.4285922050 * l[1] + 0.4505937099 * l[2],
        0.0259040371 * l[0] + 0.7827717662 * l[1] - 0.8086757660 * l[2],
    ]
}
#[inline]
fn from_oklab(c: Px) -> Px {
    let l = c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2];
    let m = c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2];
    let s = c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2];
    mul(&MLMS_2020, [l * l * l, m * m * m, s * s * s])
}
/// sRGB-encoded (0–1) colour -> linear Rec.2020.
fn srgb_to_working(c: Px) -> Px {
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

/// Decode any supported file into the working space, optionally downscaled.
pub fn load(path: &Path, max_edge: Option<u32>) -> Result<Img, String> {
    let (img, linear) = if is_raw(path) { (decode_raw(path)?, true) } else { (decode_std(path)?, false) };
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
    /// Neither straighten nor crop: the frame that mask and retouch coordinates refer to.
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
    // 9. geometry, then post-crop effects
    let mut img = if o.full_frame { Img { w, h, px } } else { crop_rotate(Img { w, h, px }, &d.crop, !o.skip_crop) };
    vignette(&mut img, &d.effects);
    img
}

/// Stages 1–7 (everything before geometry), plus each mask's coverage (None for a disabled
/// mask that is not the overlay).
fn tones(src: &[Px], w: usize, h: usize, d: &Develop, overlay: Option<usize>) -> (Vec<Px>, Vec<Option<Vec<f32>>>) {
    // 1. white balance + exposure
    let wb = wb_gains(d.white_balance.temperature_k, d.white_balance.tint);
    let g = wb.map(|v| v * d.basic_tone.exposure.exp2());
    let mut px: Vec<Px> = src.par_iter().map(|p| [p[0] * g[0], p[1] * g[1], p[2] * g[2]]).collect();
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
    if d.detail.color_noise_reduction > 0.0 {
        color_nr(&mut px, w, h, d.detail.color_noise_reduction / 100.0);
    }
    if d.detail.sharpening > 0.0 {
        sharpen(&mut px, w, h, &d.detail);
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

fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
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
/// ponytail: one-shot colour model + grey guide, no graph cut or matting model; edges between
/// similar colours stay where the AI drew them.
fn object_matte(points: &[[f32; 2]], px: &[Px], w: usize, h: usize, soft: f32) -> (Win, Vec<f32>) {
    let s = w.max(h) as f32;
    let (lo, hi) = bounds(points, w, h);
    let size = (hi[0] - lo[0]).min(hi[1] - lo[1]).max(1.0);
    let band = (0.1 * size).min(0.012 * s).max(1.0);
    let r = ((0.001 + 0.004 * soft) * s).min(0.1 * size).max(1.0);
    let k = 0.05 + 0.45 * soft;
    let win = win(points, w, h, 3.0 * (band + r));
    let n = win.w * win.h;
    if n == 0 {
        return (win, vec![]);
    }
    let at = |k: usize| px[(win.y0 + k / win.w) * w + win.x0 + k % win.w];
    let prior = blur(&fill(points, w, h, win), win.w, win.h, band);
    let bin = |p: Px| {
        let q = oklab(p);
        let f = |v: f32, lo: f32, hi: f32| (((v - lo) / (hi - lo) * 16.0) as usize).min(15);
        f(q[0], 0.0, 1.0) * 256 + f(q[1], -0.25, 0.25) * 16 + f(q[2], -0.25, 0.25)
    };
    let (mut hin, mut hout) = (vec![1f32; 4096], vec![1f32; 4096]);
    for (k, &p) in prior.iter().enumerate() {
        if p > 0.9 {
            hin[bin(at(k))] += 1.0;
        } else if p < 0.1 {
            hout[bin(at(k))] += 1.0;
        }
    }
    let (nin, nout): (f32, f32) = (hin.iter().sum(), hout.iter().sum());
    let post: Vec<f32> = prior
        .par_iter()
        .enumerate()
        .map(|(k, &p)| {
            if p <= 0.0 || p >= 1.0 {
                return p;
            }
            let b = bin(at(k));
            let (i, o) = (p * hin[b] / nin, (1.0 - p) * hout[b] / nout);
            i / (i + o)
        })
        .collect();
    let guide: Vec<f32> = (0..n).map(|k| luma(at(k)).max(0.0).sqrt()).collect();
    (win, guided(&guide, &post, win.w, win.h, r, 1e-3).into_iter().map(|v| smooth(0.5 - k, 0.5 + k, v)).collect())
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
        (v + c.edge_shift / 200.0).clamp(0.0, 1.0)
    }
}

fn mask_weights(m: &Mask, w: usize, h: usize, px: &[Px]) -> Vec<f32> {
    let comps: Vec<Comp> = m
        .components
        .iter()
        .map(|c| Comp {
            target: oklab(srgb_to_working(c.color)),
            matte: (c.kind == "polygon").then(|| object_matte(&c.points, px, w, h, 0.5 + c.feather / 200.0)),
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

fn apply_masks(px: &mut [Px], w: usize, h: usize, masks: &[Mask], overlay: Option<usize>) -> Vec<Option<Vec<f32>>> {
    let mut out = vec![];
    for (i, m) in masks.iter().enumerate() {
        if !m.enabled && overlay != Some(i) {
            out.push(None);
            continue;
        }
        let wts = mask_weights(m, w, h, px);
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

fn color_nr(px: &mut [Px], w: usize, h: usize, amount: f32) {
    let lab: Vec<Px> = px.par_iter().map(|p| oklab(*p)).collect();
    let sigma = 1.0 + amount * w.max(h) as f32 * 0.004;
    let a = blur(&lab.iter().map(|q| q[1]).collect::<Vec<_>>(), w, h, sigma);
    let b = blur(&lab.iter().map(|q| q[2]).collect::<Vec<_>>(), w, h, sigma);
    px.par_iter_mut().enumerate().for_each(|(i, p)| *p = from_oklab([lab[i][0], a[i], b[i]]));
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

/// Straighten around the centre, then cut the crop rectangle (bilinear resampling).
fn crop_rotate(img: Img, c: &Crop, apply_crop: bool) -> Img {
    if c.is_identity() || (!apply_crop && c.angle == 0.0) {
        return img;
    }
    let (fw, fh) = (img.w as f32, img.h as f32);
    let (mut x0, mut y0, mut cw, mut ch) = if apply_crop { (c.x * fw, c.y * fh, c.w * fw, c.h * fh) } else { (0.0, 0.0, fw, fh) };
    let (sin, cos) = c.angle.to_radians().sin_cos();
    let (cx, cy) = (fw / 2.0, fh / 2.0);
    if apply_crop && c.angle != 0.0 {
        // Shrink about the crop centre until all corners land inside the source (no black wedges).
        let (qx, qy) = (x0 + cw / 2.0 - cx, y0 + ch / 2.0 - cy);
        let (ax, ay) = (cx + qx * cos - qy * sin, cy + qx * sin + qy * cos);
        let mut s: f32 = 1.0;
        for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let (u, v) = (u * cw / 2.0, v * ch / 2.0);
            for (a, d, lim) in [(ax, u * cos - v * sin, fw), (ay, u * sin + v * cos, fh)] {
                if d > 1e-6 {
                    s = s.min((lim - a) / d);
                } else if d < -1e-6 {
                    s = s.min(a / -d);
                }
            }
        }
        let s = s.clamp(0.05, 1.0);
        (x0, y0, cw, ch) = (x0 + cw * (1.0 - s) / 2.0, y0 + ch * (1.0 - s) / 2.0, cw * s, ch * s);
    }
    let (ow, oh) = ((cw.round() as usize).max(1), (ch.round() as usize).max(1));
    let px = (0..ow * oh)
        .into_par_iter()
        .map(|i| {
            let (u, v) = (x0 + (i % ow) as f32 + 0.5 - cx, y0 + (i / ow) as f32 + 0.5 - cy);
            sample(&img, cx + u * cos - v * sin - 0.5, cy + u * sin + v * cos - 0.5)
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
}
