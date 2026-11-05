//! # Professional 15-Style Color Grading Engine in Rust
//!
//! This module provides both a detailed colorimetric analysis and a complete,
//! zero-dependency Rust implementation of 15 professional photographic color grading styles:
//!
//! ## Group A: 10 Professional Color Grading Styles
//! 1. **Bleach Bypass**: Silver-retention simulation combining high-contrast luminance with
//!    heavily desaturated channels and cold slate shadows.
//! 2. **Cyberpunk (Magenta & Cyan)**: Dual-tone neon grade shifting greens toward electric
//!    cyan (`185°`) and warm hues toward magenta (`315°`) over deep indigo shadows (`265°`).
//! 3. **Moody Matte & Desaturated Greens**: Lifted black point (`+0.12`), crushed white point,
//!    foliage greens shifted toward olive-drab/yellow, and warm amber skin tones.
//! 4. **Golden Hour / Warm Analog**: Chocolate-brown shadows (`28°`), golden-yellow midtones
//!    (`42°`), creamy peach highlights (`24°`), and gentle highlight roll-off.
//! 5. **Nordic Noir (Cool & Muted)**: Prussian-blue shadows (`215°`), steel-gray midtones,
//!    heavily suppressed warm hues, and protected yet cooled skin luminance.
//! 6. **Kodak Portra 400 Emulation**: Analog film response featuring soft S-curve contrast,
//!    pastel cyan skies, emerald-shifted greens, and warm luminous golden skin tones.
//! 7. **Fuji Velvia (Hyper-Vibrant)**: Slide-film emulation with deep ink blacks, steep
//!    contrast, boosted vibrance, punchy emerald greens, and rich magenta-reds.
//! 8. **Emerald & Gold (Matrix/Fincher)**: Split-complementary grade dominated by forest-green
//!    shadows (`135°`), olive-green midtones (`110°`), and warm gold highlights (`45°`).
//! 9. **Pastel Overexposed (Airy Film)**: High-key exposure with lifted milky shadows,
//!    negative contrast, lavender/mint midtones, and airy pastel saturation.
//! 10. **Cross-Processed (Xpro)**: Simulates slide film processed in C-41 chemistry via
//!     asymmetric per-channel S-curves, cyan-tinted shadows, and yellow-skewed highlights.
//!
//! ## Group B: 5 Variations of Teal & Orange
//! 11. **Muted / Matte Teal & Orange (Indie Film)**: Lifted matte blacks, reduced overall
//!     saturation (`0.78x`), dusty slate-blue shadows (`200°`), and terracotta highlights (`28°`).
//! 12. **Dark & Moody (Cyan & Amber)**: Crushed blacks, darkened blue/aqua luminance, deep
//!     abyssal cyan shadows (`190°`), and fiery bronze-amber highlights (`25°`).
//! 13. **Bright & Airy (Mint & Peach)**: High-key lifted shadows, high-luminance mint/aqua
//!     shadows (`172°`), and soft pastel coral/peach highlights (`20°`).
//! 14. **Golden Brown & Slate (Earthy T&O)**: Desaturated steel-slate shadows (`212°`),
//!     ochre/rust/coffee-brown highlights (`22°`), and suppressed green/magenta channels.
//! 15. **Neon Night (Electric Turquoise & Tangerine)**: True-black shadows for high contrast,
//!     boosted vibrance, luminous turquoise mid-shadows (`184°`), and tangerine highlights (`30°`).

use std::fmt;
use std::fs::File;
use std::io::{self, Write};

// ============================================================================
// 1. CORE COLOR SPACE STRUCTURES (RGB & HSL)
// ============================================================================

/// Normalized floating-point RGB pixel with components nominally in `[0.0, 1.0]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// HSL (Hue, Saturation, Lightness) representation for selective color grading.
/// - `h`: Degrees in `[0.0, 360.0)`
/// - `s`: Saturation in `[0.0, 1.0]`
/// - `l`: Lightness in `[0.0, 1.0]`
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    pub h: f32,
    pub s: f32,
    pub l: f32,
}

impl Rgb {
    /// Creates a new normalized `Rgb` pixel from `[0.0, 1.0]` floats.
    #[inline]
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    /// Creates a normalized `Rgb` pixel from 8-bit `[0, 255]` integers.
    #[inline]
    pub fn from_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
        }
    }

    /// Converts normalized `[0.0, 1.0]` RGB back to 8-bit `(u8, u8, u8)`.
    #[inline]
    pub fn to_u8(self) -> (u8, u8, u8) {
        let clamped = self.clamp();
        (
            (clamped.r * 255.0).round() as u8,
            (clamped.g * 255.0).round() as u8,
            (clamped.b * 255.0).round() as u8,
        )
    }

    /// Clamps all channels safely to `[0.0, 1.0]`.
    #[inline]
    pub fn clamp(self) -> Self {
        Self {
            r: self.r.clamp(0.0, 1.0),
            g: self.g.clamp(0.0, 1.0),
            b: self.b.clamp(0.0, 1.0),
        }
    }

    /// Computes Rec. 709 relative luminance: `Y = 0.2126*R + 0.7152*G + 0.0722*B`.
    #[inline]
    pub fn luminance(self) -> f32 {
        (0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b).clamp(0.0, 1.0)
    }

    /// Linearly interpolates between `self` and `other` by factor `t` in `[0.0, 1.0]`.
    #[inline]
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
        }
    }

    /// Converts `Rgb` to `Hsl`.
    pub fn to_hsl(self) -> Hsl {
        let c = self.clamp();
        let max = c.r.max(c.g).max(c.b);
        let min = c.r.min(c.g).min(c.b);
        let delta = max - min;
        let l = (max + min) * 0.5;

        if delta.abs() < 1e-6 {
            return Hsl { h: 0.0, s: 0.0, l };
        }

        let s = if l > 0.5 {
            delta / (2.0 - max - min)
        } else {
            delta / (max + min)
        };

        let mut h = if (max - c.r).abs() < 1e-6 {
            ((c.g - c.b) / delta) % 6.0
        } else if (max - c.g).abs() < 1e-6 {
            ((c.b - c.r) / delta) + 2.0
        } else {
            ((c.r - c.g) / delta) + 4.0
        };

        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }

        Hsl {
            h,
            s: s.clamp(0.0, 1.0),
            l: l.clamp(0.0, 1.0),
        }
    }
}

impl Hsl {
    /// Converts `Hsl` back to normalized `Rgb`.
    pub fn to_rgb(self) -> Rgb {
        let h = ((self.h % 360.0) + 360.0) % 360.0;
        let s = self.s.clamp(0.0, 1.0);
        let l = self.l.clamp(0.0, 1.0);

        if s.abs() < 1e-6 {
            return Rgb::new(l, l, l);
        }

        let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
        let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
        let m = l - c * 0.5;

        let (r1, g1, b1) = match h as u32 {
            0..=59 => (c, x, 0.0),
            60..=119 => (x, c, 0.0),
            120..=179 => (0.0, c, x),
            180..=239 => (0.0, x, c),
            240..=299 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };

        Rgb::new(r1 + m, g1 + m, b1 + m).clamp()
    }
}

// ============================================================================
// 2. 0-100% ADJUSTMENT BAR CONTROLLER
// ============================================================================

/// Controls how the 0–100% adjustment slider is applied to the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// Interpolates the internal grading parameters from neutral (0%) to preset (100%)
    /// before evaluating the pixel pipeline. Produces smoother tone-curve roll-off.
    ParametricGrade,
    /// Evaluates the full 100% grade and linearly blends the resulting RGB pixel with
    /// the original ungraded pixel (equivalent to a LUT opacity slider in DaVinci/Lightroom).
    LinearLutMix,
}

/// Represents the 0–100% adjustment bar for controlling style intensity.
#[derive(Debug, Clone, Copy)]
pub struct AdjustmentBar {
    /// Percentage value strictly clamped to `[0.0, 100.0]`.
    percentage: f32,
    /// Blending strategy used when applying the grade.
    pub blend_mode: BlendMode,
}

impl AdjustmentBar {
    /// Creates a new `AdjustmentBar` initialized to `percentage` (clamped to `0.0..=100.0`).
    pub fn new(percentage: f32) -> Self {
        Self {
            percentage: percentage.clamp(0.0, 100.0),
            blend_mode: BlendMode::ParametricGrade,
        }
    }

    /// Creates a new `AdjustmentBar` with a specific `BlendMode`.
    pub fn with_mode(percentage: f32, blend_mode: BlendMode) -> Self {
        Self {
            percentage: percentage.clamp(0.0, 100.0),
            blend_mode,
        }
    }

    /// Sets the adjustment bar percentage (`0.0` to `100.0`).
    pub fn set_percentage(&mut self, percentage: f32) {
        self.percentage = percentage.clamp(0.0, 100.0);
    }

    /// Increments or decrements the adjustment bar by `delta_percent`.
    pub fn nudge(&mut self, delta_percent: f32) {
        self.set_percentage(self.percentage + delta_percent);
    }

    /// Returns the raw percentage in `[0.0, 100.0]`.
    #[inline]
    pub fn percentage(&self) -> f32 {
        self.percentage
    }

    /// Returns the normalized factor in `[0.0, 1.0]`.
    #[inline]
    pub fn normalized(&self) -> f32 {
        self.percentage / 100.0
    }

    /// Renders an ASCII visual slider bar for CLI / terminal UI inspection.
    pub fn render_ascii(&self, width: usize) -> String {
        let filled = ((self.normalized() * width as f32).round() as usize).min(width);
        let empty = width.saturating_sub(filled);
        format!(
            "[{}{}] {:>5.1}%",
            "█".repeat(filled),
            "░".repeat(empty),
            self.percentage
        )
    }
}

impl fmt::Display for AdjustmentBar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.render_ascii(20))
    }
}

// ============================================================================
// 3. ENUMERATION OF ALL 15 PHOTO STYLES
// ============================================================================

/// Enumeration of all 15 photographic color grading styles (10 general + 5 Teal & Orange).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoStyle {
    // --- Group A: 10 Professional Color Grading Styles ---
    BleachBypass,
    CyberpunkMagentaCyan,
    MoodyMatteDesaturatedGreens,
    GoldenHourWarmAnalog,
    NordicNoirCoolMuted,
    KodakPortra400,
    FujiVelviaHyperVibrant,
    EmeraldAndGoldMatrix,
    PastelOverexposedAiry,
    CrossProcessedXpro,

    // --- Group B: 5 Variations of Teal & Orange ---
    MutedMatteTealOrange,
    DarkMoodyCyanAmber,
    BrightAiryMintPeach,
    GoldenBrownSlate,
    NeonNightTurquoiseTangerine,
}

impl PhotoStyle {
    /// Returns a slice containing all 15 styles in order.
    pub const fn all() -> &'static [PhotoStyle; 15] {
        &[
            PhotoStyle::BleachBypass,
            PhotoStyle::CyberpunkMagentaCyan,
            PhotoStyle::MoodyMatteDesaturatedGreens,
            PhotoStyle::GoldenHourWarmAnalog,
            PhotoStyle::NordicNoirCoolMuted,
            PhotoStyle::KodakPortra400,
            PhotoStyle::FujiVelviaHyperVibrant,
            PhotoStyle::EmeraldAndGoldMatrix,
            PhotoStyle::PastelOverexposedAiry,
            PhotoStyle::CrossProcessedXpro,
            PhotoStyle::MutedMatteTealOrange,
            PhotoStyle::DarkMoodyCyanAmber,
            PhotoStyle::BrightAiryMintPeach,
            PhotoStyle::GoldenBrownSlate,
            PhotoStyle::NeonNightTurquoiseTangerine,
        ]
    }

    /// Human-readable display title of the style.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::BleachBypass => "1. Bleach Bypass",
            Self::CyberpunkMagentaCyan => "2. Cyberpunk (Magenta & Cyan)",
            Self::MoodyMatteDesaturatedGreens => "3. Moody Matte & Desaturated Greens",
            Self::GoldenHourWarmAnalog => "4. Golden Hour / Warm Analog",
            Self::NordicNoirCoolMuted => "5. Nordic Noir (Cool & Muted)",
            Self::KodakPortra400 => "6. Kodak Portra 400 Emulation",
            Self::FujiVelviaHyperVibrant => "7. Fuji Velvia (Hyper-Vibrant)",
            Self::EmeraldAndGoldMatrix => "8. Emerald & Gold (Matrix/Fincher)",
            Self::PastelOverexposedAiry => "9. Pastel Overexposed (Airy Film)",
            Self::CrossProcessedXpro => "10. Cross-Processed (Xpro)",
            Self::MutedMatteTealOrange => "11. [T&O Var 1] Muted / Matte Teal & Orange",
            Self::DarkMoodyCyanAmber => "12. [T&O Var 2] Dark & Moody (Cyan & Amber)",
            Self::BrightAiryMintPeach => "13. [T&O Var 3] Bright & Airy (Mint & Peach)",
            Self::GoldenBrownSlate => "14. [T&O Var 4] Golden Brown & Slate (Earthy)",
            Self::NeonNightTurquoiseTangerine => {
                "15. [T&O Var 5] Neon Night (Turquoise & Tangerine)"
            }
        }
    }

    /// Technical summary of the color science used by this preset.
    pub const fn technical_analysis(&self) -> &'static str {
        match self {
            Self::BleachBypass => {
                "Blends high-contrast luminance silver layer with low-saturation RGB (0.38x) and cold slate shadows (210°)."
            }
            Self::CyberpunkMagentaCyan => {
                "Rotates green foliage/midtones toward electric cyan (-55°) and warm yellows toward neon magenta (+42°) over violet shadows."
            }
            Self::MoodyMatteDesaturatedGreens => {
                "Lifts black point (+0.11) for matte fade, desaturates greens by 60% while shifting them toward olive (-28°), warming skin tones."
            }
            Self::GoldenHourWarmAnalog => {
                "Applies warm split-toning across all three zones (28°/42°/24°), gently rolls off highlights (0.95), and boosts golden vibrance."
            }
            Self::NordicNoirCoolMuted => {
                "Suppresses warm red/yellow saturation, deepens Prussian blue shadows (215°), and maintains crisp steel-gray midtones."
            }
            Self::KodakPortra400 => {
                "Emulates C-41 print film with soft S-curve, cyan-shifted skies (-14°), emerald greens (+12°), and luminous warm skin protection."
            }
            Self::FujiVelviaHyperVibrant => {
                "Emulates reversal slide film with deep crushed blacks (-0.03), high S-curve (+0.38), high vibrance (+0.40), and rich reds/greens."
            }
            Self::EmeraldAndGoldMatrix => {
                "Tints shadows deep forest green (135°), midtones olive-emerald (112°), and isolates key highlights with warm gold (45°)."
            }
            Self::PastelOverexposedAiry => {
                "Raises exposure (+0.08) and black point (+0.14) with negative contrast (-0.20) and lavender/mint pastel split-toning."
            }
            Self::CrossProcessedXpro => {
                "Applies asymmetric RGB channel curves (boosted blue shadows, S-curved green, clipped red-yellow highlights) for C-41 Xpro grit."
            }
            Self::MutedMatteTealOrange => {
                "Fades the tone curve (black lift +0.12, white point 0.92) and lowers global saturation (0.78x) for dusty slate & terracotta."
            }
            Self::DarkMoodyCyanAmber => {
                "Pulls down exposure (-0.06), crushes shadows (-0.04) with high contrast (+0.42), abyssal cyan shadows (190°), and bronze highlights."
            }
            Self::BrightAiryMintPeach => {
                "High-key lift (+0.07 exposure, +0.09 black lift) pairing high-luminance mint/aqua shadows (172°) with coral-peach highlights (20°)."
            }
            Self::GoldenBrownSlate => {
                "Desaturates greens/magentas strongly while pairing cool steel-slate shadows (212°) with earthy ochre/coffee-brown highlights (22°)."
            }
            Self::NeonNightTurquoiseTangerine => {
                "Anchors true blacks (-0.02) with steep S-curve (+0.36) and high vibrance (+0.35) so electric turquoise (184°) & tangerine (30°) glow."
            }
        }
    }
}

// ============================================================================
// 4. MATHEMATICAL GRADING PARAMETERS & PIPELINE
// ============================================================================

/// A single split-toning color wheel vector defined by Hue (degrees) and Strength (`0.0..=1.0`).
#[derive(Debug, Clone, Copy)]
pub struct ToneWheel {
    pub hue_deg: f32,
    pub strength: f32,
}

impl ToneWheel {
    pub const fn new(hue_deg: f32, strength: f32) -> Self {
        Self { hue_deg, strength }
    }

    /// Converts the wheel angle and strength into a zero-sum RGB tint offset vector
    /// so applying the tint shifts chromaticity without skewing overall luminance.
    pub fn to_rgb_offset(self) -> (f32, f32, f32) {
        if self.strength <= 1e-5 {
            return (0.0, 0.0, 0.0);
        }
        let pure = Hsl {
            h: self.hue_deg,
            s: 1.0,
            l: 0.5,
        }
        .to_rgb();
        // Center around neutral 0.5 gray and scale by strength (max +/- 0.25 shift at 1.0)
        let scale = self.strength * 0.45;
        (
            (pure.r - 0.5) * scale,
            (pure.g - 0.5) * scale,
            (pure.b - 0.5) * scale,
        )
    }
}

/// Complete parametric specification of a photographic color grade.
#[derive(Debug, Clone, Copy)]
pub struct StyleParameters {
    /// Global exposure offset in stops-equivalent (`-0.5` to `+0.5`).
    pub exposure: f32,
    /// S-curve contrast strength (`-0.5` to `+0.8`).
    pub contrast: f32,
    /// Matte black point lift (`-0.1` to `+0.25`).
    pub black_lift: f32,
    /// White point ceiling (`0.80` to `1.05`).
    pub white_point: f32,

    /// 3-Way Split-Toning wheels (Shadows, Midtones, Highlights).
    pub shadow_wheel: ToneWheel,
    pub midtone_wheel: ToneWheel,
    pub highlight_wheel: ToneWheel,

    /// Global saturation multiplier (`0.0` = monochrome, `1.0` = neutral, `1.5` = vivid).
    pub saturation: f32,
    /// Selective vibrance boost for muted colors (`-0.5` to `+0.6`).
    pub vibrance: f32,

    /// Selective HSL hue shifts (in degrees `-60.0` to `+60.0`).
    pub green_hue_shift: f32,
    pub green_sat_mult: f32,
    pub blue_hue_shift: f32,
    pub blue_sat_mult: f32,
    pub warm_hue_shift: f32,
    pub warm_sat_mult: f32,

    /// Per-channel S-curve skew for Cross-Processing (`0.0` = normal, `1.0` = full Xpro).
    pub xpro_channel_skew: f32,
    /// Silver-retention luminance overlay blend (`0.0` = normal, `1.0` = full Bleach Bypass).
    pub bleach_bypass_mix: f32,
    /// Protects skin tones (`15°..=45°` hue) from unnatural cool/green shifts (`0.0..=1.0`).
    pub skin_tone_protection: f32,
}

impl StyleParameters {
    /// Returns the neutral identity parameter set (equivalent to 0% adjustment).
    pub const fn neutral() -> Self {
        Self {
            exposure: 0.0,
            contrast: 0.0,
            black_lift: 0.0,
            white_point: 1.0,
            shadow_wheel: ToneWheel::new(0.0, 0.0),
            midtone_wheel: ToneWheel::new(0.0, 0.0),
            highlight_wheel: ToneWheel::new(0.0, 0.0),
            saturation: 1.0,
            vibrance: 0.0,
            green_hue_shift: 0.0,
            green_sat_mult: 1.0,
            blue_hue_shift: 0.0,
            blue_sat_mult: 1.0,
            warm_hue_shift: 0.0,
            warm_sat_mult: 1.0,
            xpro_channel_skew: 0.0,
            bleach_bypass_mix: 0.0,
            skin_tone_protection: 0.0,
        }
    }

    /// Returns the calibrated `StyleParameters` for any of the 15 `PhotoStyle` presets.
    pub const fn from_style(style: PhotoStyle) -> Self {
        match style {
            // 1. Bleach Bypass
            PhotoStyle::BleachBypass => Self {
                exposure: -0.02,
                contrast: 0.52,
                black_lift: -0.02,
                white_point: 0.98,
                shadow_wheel: ToneWheel::new(210.0, 0.22),
                midtone_wheel: ToneWheel::new(205.0, 0.10),
                highlight_wheel: ToneWheel::new(45.0, 0.12),
                saturation: 0.38,
                vibrance: -0.15,
                green_hue_shift: -10.0,
                green_sat_mult: 0.40,
                blue_hue_shift: -5.0,
                blue_sat_mult: 0.60,
                warm_hue_shift: 0.0,
                warm_sat_mult: 0.55,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.65,
                skin_tone_protection: 0.35,
            },

            // 2. Cyberpunk (Magenta & Cyan)
            PhotoStyle::CyberpunkMagentaCyan => Self {
                exposure: 0.0,
                contrast: 0.35,
                black_lift: 0.0,
                white_point: 1.0,
                shadow_wheel: ToneWheel::new(265.0, 0.58),
                midtone_wheel: ToneWheel::new(192.0, 0.36),
                highlight_wheel: ToneWheel::new(315.0, 0.62),
                saturation: 1.25,
                vibrance: 0.35,
                green_hue_shift: -55.0, // Shift greens toward electric cyan
                green_sat_mult: 1.30,
                blue_hue_shift: -12.0,
                blue_sat_mult: 1.40,
                warm_hue_shift: 42.0, // Shift oranges/reds toward neon magenta/pink
                warm_sat_mult: 1.35,
                xpro_channel_skew: 0.15,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.25,
            },

            // 3. Moody Matte & Desaturated Greens
            PhotoStyle::MoodyMatteDesaturatedGreens => Self {
                exposure: -0.03,
                contrast: 0.12,
                black_lift: 0.11,
                white_point: 0.91,
                shadow_wheel: ToneWheel::new(150.0, 0.14),
                midtone_wheel: ToneWheel::new(35.0, 0.12),
                highlight_wheel: ToneWheel::new(38.0, 0.28),
                saturation: 0.82,
                vibrance: 0.08,
                green_hue_shift: -28.0, // Shift greens toward yellow/olive-drab
                green_sat_mult: 0.38,
                blue_hue_shift: -8.0,
                blue_sat_mult: 0.55,
                warm_hue_shift: -4.0,
                warm_sat_mult: 1.08,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.75,
            },

            // 4. Golden Hour / Warm Analog
            PhotoStyle::GoldenHourWarmAnalog => Self {
                exposure: 0.03,
                contrast: 0.16,
                black_lift: 0.04,
                white_point: 0.95,
                shadow_wheel: ToneWheel::new(28.0, 0.28),
                midtone_wheel: ToneWheel::new(40.0, 0.25),
                highlight_wheel: ToneWheel::new(24.0, 0.42),
                saturation: 1.12,
                vibrance: 0.22,
                green_hue_shift: -18.0,
                green_sat_mult: 0.88,
                blue_hue_shift: -6.0,
                blue_sat_mult: 0.85,
                warm_hue_shift: 4.0,
                warm_sat_mult: 1.20,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.85,
            },

            // 5. Nordic Noir (Cool & Muted)
            PhotoStyle::NordicNoirCoolMuted => Self {
                exposure: -0.04,
                contrast: 0.28,
                black_lift: 0.03,
                white_point: 0.94,
                shadow_wheel: ToneWheel::new(215.0, 0.46),
                midtone_wheel: ToneWheel::new(205.0, 0.22),
                highlight_wheel: ToneWheel::new(200.0, 0.12),
                saturation: 0.65,
                vibrance: -0.10,
                green_hue_shift: 15.0,
                green_sat_mult: 0.45,
                blue_hue_shift: 4.0,
                blue_sat_mult: 1.10,
                warm_hue_shift: -6.0,
                warm_sat_mult: 0.55,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.15,
                skin_tone_protection: 0.60,
            },

            // 6. Kodak Portra 400 Emulation
            PhotoStyle::KodakPortra400 => Self {
                exposure: 0.02,
                contrast: 0.14,
                black_lift: 0.03,
                white_point: 0.97,
                shadow_wheel: ToneWheel::new(22.0, 0.10),
                midtone_wheel: ToneWheel::new(34.0, 0.16),
                highlight_wheel: ToneWheel::new(42.0, 0.26),
                saturation: 1.04,
                vibrance: 0.18,
                green_hue_shift: 12.0, // Classic Portra emerald-mint greens
                green_sat_mult: 0.85,
                blue_hue_shift: -14.0, // Pastel cyan skies
                blue_sat_mult: 0.92,
                warm_hue_shift: 5.0, // Golden skin luminance
                warm_sat_mult: 1.10,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.95,
            },

            // 7. Fuji Velvia (Hyper-Vibrant)
            PhotoStyle::FujiVelviaHyperVibrant => Self {
                exposure: -0.01,
                contrast: 0.38,
                black_lift: -0.03,
                white_point: 1.0,
                shadow_wheel: ToneWheel::new(235.0, 0.18),
                midtone_wheel: ToneWheel::new(140.0, 0.08),
                highlight_wheel: ToneWheel::new(28.0, 0.20),
                saturation: 1.32,
                vibrance: 0.40,
                green_hue_shift: 8.0,
                green_sat_mult: 1.35,
                blue_hue_shift: 4.0,
                blue_sat_mult: 1.30,
                warm_hue_shift: -6.0, // Rich magenta-leaning reds
                warm_sat_mult: 1.30,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.50,
            },

            // 8. Emerald & Gold (Matrix/Fincher)
            PhotoStyle::EmeraldAndGoldMatrix => Self {
                exposure: -0.03,
                contrast: 0.32,
                black_lift: 0.02,
                white_point: 0.96,
                shadow_wheel: ToneWheel::new(135.0, 0.48),
                midtone_wheel: ToneWheel::new(112.0, 0.32),
                highlight_wheel: ToneWheel::new(45.0, 0.50),
                saturation: 0.88,
                vibrance: 0.12,
                green_hue_shift: 10.0,
                green_sat_mult: 1.15,
                blue_hue_shift: -35.0, // Pull blues toward cyan-green
                blue_sat_mult: 0.75,
                warm_hue_shift: 12.0, // Shift oranges toward gold
                warm_sat_mult: 1.05,
                xpro_channel_skew: 0.10,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.40,
            },

            // 9. Pastel Overexposed (Airy Film)
            PhotoStyle::PastelOverexposedAiry => Self {
                exposure: 0.08,
                contrast: -0.20,
                black_lift: 0.14,
                white_point: 1.02,
                shadow_wheel: ToneWheel::new(270.0, 0.16), // Soft lavender shadows
                midtone_wheel: ToneWheel::new(165.0, 0.14), // Mint midtones
                highlight_wheel: ToneWheel::new(35.0, 0.22), // Creamy warm highlights
                saturation: 0.84,
                vibrance: 0.25,
                green_hue_shift: 18.0,
                green_sat_mult: 0.72,
                blue_hue_shift: -16.0,
                blue_sat_mult: 0.78,
                warm_hue_shift: 6.0,
                warm_sat_mult: 0.90,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.85,
            },

            // 10. Cross-Processed (Xpro)
            PhotoStyle::CrossProcessedXpro => Self {
                exposure: 0.01,
                contrast: 0.34,
                black_lift: 0.04,
                white_point: 1.02,
                shadow_wheel: ToneWheel::new(195.0, 0.45), // Cyan-tinted blacks
                midtone_wheel: ToneWheel::new(95.0, 0.24), // Green-yellow midtones
                highlight_wheel: ToneWheel::new(52.0, 0.55), // Blown-out yellow highlights
                saturation: 1.18,
                vibrance: 0.20,
                green_hue_shift: -15.0,
                green_sat_mult: 1.20,
                blue_hue_shift: -20.0,
                blue_sat_mult: 1.15,
                warm_hue_shift: 14.0,
                warm_sat_mult: 1.25,
                xpro_channel_skew: 0.85, // Strong per-channel asymmetric S-curves
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.20,
            },

            // ================================================================
            // GROUP B: 5 VARIATIONS OF TEAL & ORANGE
            // ================================================================

            // 11. Muted / Matte Teal & Orange (Indie Film Look)
            PhotoStyle::MutedMatteTealOrange => Self {
                exposure: 0.0,
                contrast: -0.08,
                black_lift: 0.12,
                white_point: 0.92,
                shadow_wheel: ToneWheel::new(200.0, 0.34), // Dusty slate-blue
                midtone_wheel: ToneWheel::new(195.0, 0.10),
                highlight_wheel: ToneWheel::new(28.0, 0.38), // Soft terracotta/tan
                saturation: 0.78,
                vibrance: 0.10,
                green_hue_shift: 22.0,
                green_sat_mult: 0.55,
                blue_hue_shift: -18.0,
                blue_sat_mult: 0.82,
                warm_hue_shift: -2.0,
                warm_sat_mult: 0.92,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.85,
            },

            // 12. Dark & Moody (Cyan & Amber)
            PhotoStyle::DarkMoodyCyanAmber => Self {
                exposure: -0.06,
                contrast: 0.42,
                black_lift: -0.03,
                white_point: 0.96,
                shadow_wheel: ToneWheel::new(190.0, 0.62), // Abyssal dark cyan
                midtone_wheel: ToneWheel::new(194.0, 0.22),
                highlight_wheel: ToneWheel::new(25.0, 0.66), // Fiery amber/bronze
                saturation: 0.96,
                vibrance: 0.24,
                green_hue_shift: 35.0,
                green_sat_mult: 0.50,
                blue_hue_shift: -22.0,
                blue_sat_mult: 1.15,
                warm_hue_shift: -5.0,
                warm_sat_mult: 1.22,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.12,
                skin_tone_protection: 0.75,
            },

            // 13. Bright & Airy (Mint & Peach)
            PhotoStyle::BrightAiryMintPeach => Self {
                exposure: 0.07,
                contrast: -0.14,
                black_lift: 0.09,
                white_point: 1.01,
                shadow_wheel: ToneWheel::new(172.0, 0.40), // Aqua/mint shadows
                midtone_wheel: ToneWheel::new(178.0, 0.14),
                highlight_wheel: ToneWheel::new(20.0, 0.46), // Pastel peach/coral
                saturation: 1.06,
                vibrance: 0.28,
                green_hue_shift: 32.0,
                green_sat_mult: 0.85,
                blue_hue_shift: -28.0,
                blue_sat_mult: 1.08,
                warm_hue_shift: -4.0,
                warm_sat_mult: 1.08,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.90,
            },

            // 14. Golden Brown & Slate (Earthy Teal & Orange)
            PhotoStyle::GoldenBrownSlate => Self {
                exposure: -0.02,
                contrast: 0.24,
                black_lift: 0.04,
                white_point: 0.95,
                shadow_wheel: ToneWheel::new(212.0, 0.30), // Cool steel-slate
                midtone_wheel: ToneWheel::new(28.0, 0.14),
                highlight_wheel: ToneWheel::new(22.0, 0.50), // Ochre, rust, coffee brown
                saturation: 0.76,
                vibrance: 0.14,
                green_hue_shift: -25.0,
                green_sat_mult: 0.20, // Strongly suppress greens
                blue_hue_shift: -10.0,
                blue_sat_mult: 0.68,
                warm_hue_shift: -6.0,
                warm_sat_mult: 1.15,
                xpro_channel_skew: 0.0,
                bleach_bypass_mix: 0.08,
                skin_tone_protection: 0.80,
            },

            // 15. Neon Night (Electric Turquoise & Tangerine)
            PhotoStyle::NeonNightTurquoiseTangerine => Self {
                exposure: 0.0,
                contrast: 0.36,
                black_lift: -0.02, // Keep true blacks crisp so midtones glow
                white_point: 1.0,
                shadow_wheel: ToneWheel::new(184.0, 0.64), // Electric turquoise
                midtone_wheel: ToneWheel::new(188.0, 0.28),
                highlight_wheel: ToneWheel::new(30.0, 0.70), // Luminous tangerine
                saturation: 1.18,
                vibrance: 0.38,
                green_hue_shift: 42.0,
                green_sat_mult: 1.15,
                blue_hue_shift: -26.0,
                blue_sat_mult: 1.35,
                warm_hue_shift: 2.0,
                warm_sat_mult: 1.32,
                xpro_channel_skew: 0.08,
                bleach_bypass_mix: 0.0,
                skin_tone_protection: 0.70,
            },
        }
    }

    /// Interpolates all parameters smoothly between `neutral()` (`t = 0.0`) and `self` (`t = 1.0`).
    pub fn scale_by_intensity(&self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: f32, b: f32| a + (b - a) * t;

        Self {
            exposure: lerp(0.0, self.exposure),
            contrast: lerp(0.0, self.contrast),
            black_lift: lerp(0.0, self.black_lift),
            white_point: lerp(1.0, self.white_point),
            shadow_wheel: ToneWheel::new(self.shadow_wheel.hue_deg, self.shadow_wheel.strength * t),
            midtone_wheel: ToneWheel::new(
                self.midtone_wheel.hue_deg,
                self.midtone_wheel.strength * t,
            ),
            highlight_wheel: ToneWheel::new(
                self.highlight_wheel.hue_deg,
                self.highlight_wheel.strength * t,
            ),
            saturation: lerp(1.0, self.saturation),
            vibrance: lerp(0.0, self.vibrance),
            green_hue_shift: lerp(0.0, self.green_hue_shift),
            green_sat_mult: lerp(1.0, self.green_sat_mult),
            blue_hue_shift: lerp(0.0, self.blue_hue_shift),
            blue_sat_mult: lerp(1.0, self.blue_sat_mult),
            warm_hue_shift: lerp(0.0, self.warm_hue_shift),
            warm_sat_mult: lerp(1.0, self.warm_sat_mult),
            xpro_channel_skew: lerp(0.0, self.xpro_channel_skew),
            bleach_bypass_mix: lerp(0.0, self.bleach_bypass_mix),
            skin_tone_protection: self.skin_tone_protection,
        }
    }
}

// ============================================================================
// 5. PIXEL COLOR GRADING ENGINE
// ============================================================================

/// Stateful color grading processor combining a `PhotoStyle` preset and a 0–100% `AdjustmentBar`.
#[derive(Debug, Clone)]
pub struct ColorGradingEngine {
    pub style: PhotoStyle,
    pub adjustment_bar: AdjustmentBar,
}

impl ColorGradingEngine {
    /// Creates a new `ColorGradingEngine` with the chosen style and initial 0–100% intensity.
    pub fn new(style: PhotoStyle, adjustment_percent: f32) -> Self {
        Self {
            style,
            adjustment_bar: AdjustmentBar::new(adjustment_percent),
        }
    }

    /// Evaluates a single `Rgb` pixel through the full multi-stage color grading pipeline,
    /// respecting the current `AdjustmentBar` percentage (`0.0%` to `100.0%`).
    pub fn process_pixel(&self, input: Rgb) -> Rgb {
        let t = self.adjustment_bar.normalized();
        if t <= 1e-6 {
            return input.clamp();
        }

        let base_params = StyleParameters::from_style(self.style);

        match self.adjustment_bar.blend_mode {
            BlendMode::ParametricGrade => {
                let scaled_params = base_params.scale_by_intensity(t);
                Self::evaluate_pipeline(input, &scaled_params)
            }
            BlendMode::LinearLutMix => {
                let full_graded = Self::evaluate_pipeline(input, &base_params);
                input.lerp(full_graded, t).clamp()
            }
        }
    }

    /// Processes a slice of `Rgb` pixels in-place or into a new `Vec<Rgb>`.
    pub fn process_buffer(&self, buffer: &[Rgb]) -> Vec<Rgb> {
        buffer.iter().copied().map(|px| self.process_pixel(px)).collect()
    }

    /// Internal 5-stage colorimetric pipeline evaluation for a single pixel.
    fn evaluate_pipeline(input: Rgb, p: &StyleParameters) -> Rgb {
        let mut px = input.clamp();

        // --------------------------------------------------------------------
        // STAGE 1: Exposure & Parametric S-Curve / Matte Tone Curve
        // --------------------------------------------------------------------
        let exp_gain = 2.0_f32.powf(p.exposure);
        px.r = Self::apply_tone_curve(px.r * exp_gain, p.contrast, p.black_lift, p.white_point);
        px.g = Self::apply_tone_curve(px.g * exp_gain, p.contrast, p.black_lift, p.white_point);
        px.b = Self::apply_tone_curve(px.b * exp_gain, p.contrast, p.black_lift, p.white_point);

        // Cross-Processing (Xpro) asymmetric per-channel curves
        if p.xpro_channel_skew > 1e-4 {
            let skew = p.xpro_channel_skew;
            // Boost blue in shadows, clip red/green into yellow in highlights
            let r_xpro = Self::apply_tone_curve(px.r, 0.35 * skew, 0.0, 1.0 + 0.06 * skew);
            let g_xpro = Self::apply_tone_curve(px.g, 0.25 * skew, 0.02 * skew, 1.0);
            let b_xpro = Self::apply_tone_curve(px.b, -0.20 * skew, 0.08 * skew, 0.90);
            px = px.lerp(Rgb::new(r_xpro, g_xpro, b_xpro), skew);
        }

        // --------------------------------------------------------------------
        // STAGE 2: Selective HSL Band Calibration & Skin-Tone Protection
        // --------------------------------------------------------------------
        let mut hsl = px.to_hsl();

        // Compute skin-tone closeness weight (centered around 26° orange-peach hue)
        let skin_dist = Self::angular_distance(hsl.h, 26.0);
        let is_skin = (1.0 - (skin_dist / 28.0).clamp(0.0, 1.0)).powi(2) * p.skin_tone_protection;

        // Band weights based on angular proximity
        let green_weight = (1.0 - (Self::angular_distance(hsl.h, 120.0) / 55.0).clamp(0.0, 1.0))
            * (1.0 - is_skin);
        let blue_weight = (1.0 - (Self::angular_distance(hsl.h, 220.0) / 60.0).clamp(0.0, 1.0))
            * (1.0 - is_skin);
        let warm_weight =
            1.0 - (Self::angular_distance(hsl.h, 30.0) / 50.0).clamp(0.0, 1.0);

        let hue_delta = green_weight * p.green_hue_shift
            + blue_weight * p.blue_hue_shift
            + warm_weight * p.warm_hue_shift * (1.0 - 0.5 * is_skin);

        let sat_factor = 1.0
            + green_weight * (p.green_sat_mult - 1.0)
            + blue_weight * (p.blue_sat_mult - 1.0)
            + warm_weight * (p.warm_sat_mult - 1.0);

        hsl.h = (hsl.h + hue_delta + 360.0) % 360.0;
        hsl.s = (hsl.s * sat_factor).clamp(0.0, 1.0);
        px = hsl.to_rgb();

        // --------------------------------------------------------------------
        // STAGE 3: 3-Way Split-Toning (Shadows / Midtones / Highlights)
        // --------------------------------------------------------------------
        let lum = px.luminance();
        let w_shadow = (1.0 - lum).powi(2);
        let w_highlight = lum.powi(2);
        let w_midtone = (1.0 - w_shadow - w_highlight).max(0.0);

        let (sr, sg, sb) = p.shadow_wheel.to_rgb_offset();
        let (mr, mg, mb) = p.midtone_wheel.to_rgb_offset();
        let (hr, hg, hb) = p.highlight_wheel.to_rgb_offset();

        // Protect skin tones from heavy shadow/midtone cool contamination
        let cool_damp = 1.0 - 0.65 * is_skin;
        px.r += (sr * w_shadow + mr * w_midtone) * cool_damp + hr * w_highlight;
        px.g += (sg * w_shadow + mg * w_midtone) * cool_damp + hg * w_highlight;
        px.b += (sb * w_shadow + mb * w_midtone) * cool_damp + hb * w_highlight;
        px = px.clamp();

        // --------------------------------------------------------------------
        // STAGE 4: Global Saturation & Vibrance
        // --------------------------------------------------------------------
        let mut hsl_post = px.to_hsl();
        // Vibrance boosts less-saturated pixels more than already-saturated pixels
        let vibrance_boost = 1.0 + p.vibrance * (1.0 - hsl_post.s);
        let effective_sat = p.saturation * vibrance_boost;
        // If saturation is heavily reduced (e.g. Nordic Noir / Bleach Bypass), slightly protect skin
        let final_sat_mult =
            effective_sat + is_skin * 0.25 * (1.0 - effective_sat).max(0.0);
        hsl_post.s = (hsl_post.s * final_sat_mult).clamp(0.0, 1.0);
        px = hsl_post.to_rgb();

        // --------------------------------------------------------------------
        // STAGE 5: Bleach Bypass Silver-Retention Overlay
        // --------------------------------------------------------------------
        if p.bleach_bypass_mix > 1e-4 {
            let y = px.luminance();
            // High-contrast silver plate overlay formula
            let overlay = |c: f32, l: f32| {
                if l < 0.5 {
                    2.0 * c * l
                } else {
                    1.0 - 2.0 * (1.0 - c) * (1.0 - l)
                }
            };
            let silver = Rgb::new(overlay(px.r, y), overlay(px.g, y), overlay(px.b, y));
            px = px.lerp(silver, p.bleach_bypass_mix);
        }

        px.clamp()
    }

    /// Evaluates a smooth S-curve and black/white point remapping on a single `[0.0, 1.0]` channel.
    #[inline]
    fn apply_tone_curve(x: f32, contrast: f32, black_lift: f32, white_point: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        // Smooth polynomial S-curve centered at 0.5
        let centered = x - 0.5;
        let s_curved = (x + contrast * centered * (1.0 - (2.0 * centered).powi(2))).clamp(0.0, 1.0);
        // Remap dynamic range between black_lift and white_point
        (black_lift + s_curved * (white_point - black_lift)).clamp(0.0, 1.0)
    }

    /// Shortest angular distance between two hue angles in `[0, 180]`.
    #[inline]
    fn angular_distance(h1: f32, h2: f32) -> f32 {
        let diff = (h1 - h2).abs() % 360.0;
        if diff > 180.0 {
            360.0 - diff
        } else {
            diff
        }
    }
}

// ============================================================================
// 6. SYNTHETIC TEST SCENE & PPM IMAGE EXPORTER
// ============================================================================

/// Generates a multi-zone photographic test image (sky, foliage, portrait skin tone,
/// warm streetlamp highlight, and deep urban shadow) to benchmark all 15 styles.
pub fn generate_test_scene(width: usize, height: usize) -> Vec<Rgb> {
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let ny = y as f32 / height as f32;
        for x in 0..width {
            let nx = x as f32 / width as f32;

            // Zone 1: Dusk sky gradient in upper half
            let sky = Rgb::new(0.22 + 0.25 * ny, 0.38 + 0.20 * ny, 0.68 - 0.15 * ny);
            // Zone 2: Forest foliage band in middle
            let foliage = Rgb::new(0.18, 0.42, 0.20);
            // Zone 3: Deep urban shadow in lower half
            let shadow = Rgb::new(0.08 + 0.10 * nx, 0.09 + 0.10 * nx, 0.11 + 0.10 * nx);

            let mut base = if ny < 0.40 {
                sky
            } else if ny < 0.65 {
                foliage.lerp(sky, (0.65 - ny) * 2.0)
            } else {
                shadow
            };

            // Zone 4: Portrait subject skin-tone circle in center
            let dx_skin = nx - 0.45;
            let dy_skin = ny - 0.55;
            if dx_skin * dx_skin + dy_skin * dy_skin < 0.025 {
                base = Rgb::new(0.78, 0.56, 0.44);
            }

            // Zone 5: Warm streetlamp highlight orb on the right
            let dx_lamp = nx - 0.78;
            let dy_lamp = ny - 0.32;
            let dist_lamp = (dx_lamp * dx_lamp + dy_lamp * dy_lamp).sqrt();
            if dist_lamp < 0.15 {
                let glow = (1.0 - dist_lamp / 0.15).powi(2);
                base = base.lerp(Rgb::new(0.98, 0.85, 0.55), glow);
            }

            pixels.push(base.clamp());
        }
    }
    pixels
}

/// Writes an `Rgb` buffer to a standard Netpbm `.ppm` image file (viewable in any image viewer).
pub fn save_ppm_image(path: &str, width: usize, height: usize, pixels: &[Rgb]) -> io::Result<()> {
    let mut file = File::create(path)?;
    writeln!(file, "P6\n{} {}\n255", width, height)?;
    let mut raw_bytes = Vec::with_capacity(pixels.len() * 3);
    for px in pixels {
        let (r, g, b) = px.to_u8();
        raw_bytes.push(r);
        raw_bytes.push(g);
        raw_bytes.push(b);
    }
    file.write_all(&raw_bytes)?;
    Ok(())
}

// ============================================================================
// 7. DEMONSTRATION RUNNER & UNIT TESTS
// ============================================================================

fn main() {
    println!("================================================================================");
    println!("   RUST 15-STYLE PHOTOGRAPHIC COLOR GRADING ENGINE (WITH 0-100% ADJUSTMENT BAR)");
    println!("================================================================================\n");

    // Representative sample pixels from a photograph:
    let shadow_sample = Rgb::from_u8(32, 36, 42);     // Dark urban shadow
    let foliage_sample = Rgb::from_u8(54, 115, 58);   // Natural green foliage
    let skin_sample = Rgb::from_u8(204, 148, 118);    // Portrait skin midtone
    let highlight_sample = Rgb::from_u8(235, 220, 195); // Warm sunlit highlight

    let bar_steps = [0.0_f32, 25.0, 50.0, 75.0, 100.0];

    for style in PhotoStyle::all() {
        println!("--------------------------------------------------------------------------------");
        println!("STYLE: {}", style.name());
        println!("ANALYSIS: {}", style.technical_analysis());
        println!("--------------------------------------------------------------------------------");

        let mut engine = ColorGradingEngine::new(*style, 0.0);

        for &pct in &bar_steps {
            engine.adjustment_bar.set_percentage(pct);

            let s_out = engine.process_pixel(shadow_sample).to_u8();
            let f_out = engine.process_pixel(foliage_sample).to_u8();
            let k_out = engine.process_pixel(skin_sample).to_u8();
            let h_out = engine.process_pixel(highlight_sample).to_u8();

            println!(
                "  {} | Shadow:{:03?}, Foliage:{:03?}, Skin:{:03?}, Highlight:{:03?}",
                engine.adjustment_bar.render_ascii(12),
                s_out,
                f_out,
                k_out,
                h_out
            );
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_percent_adjustment_is_identity() {
        let sample = Rgb::new(0.42, 0.58, 0.31);
        for style in PhotoStyle::all() {
            let engine = ColorGradingEngine::new(*style, 0.0);
            let out = engine.process_pixel(sample);
            assert!((out.r - sample.r).abs() < 1e-5);
            assert!((out.g - sample.g).abs() < 1e-5);
            assert!((out.b - sample.b).abs() < 1e-5);
        }
    }

    #[test]
    fn test_adjustment_bar_clamping_and_monotonic_progression() {
        let mut bar = AdjustmentBar::new(145.0);
        assert_eq!(bar.percentage(), 100.0);
        bar.set_percentage(-20.0);
        assert_eq!(bar.percentage(), 0.0);

        let sample = Rgb::new(0.25, 0.45, 0.65);
        let engine_50 = ColorGradingEngine::new(PhotoStyle::DarkMoodyCyanAmber, 50.0);
        let engine_100 = ColorGradingEngine::new(PhotoStyle::DarkMoodyCyanAmber, 100.0);

        let out_50 = engine_50.process_pixel(sample);
        let out_100 = engine_100.process_pixel(sample);

        // Ensure 50% grade sits between 0% (sample) and 100% grade
        let dist_50 = (out_50.r - sample.r).hypot(out_50.b - sample.b);
        let dist_100 = (out_100.r - sample.r).hypot(out_100.b - sample.b);
        assert!(dist_50 > 0.0 && dist_50 < dist_100);
    }
}