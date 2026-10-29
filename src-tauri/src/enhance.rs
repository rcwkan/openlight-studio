//! Denoise & Super Resolution (spec §3.4.3) for Enhance, which writes a new master next to the original as
//! Lightroom does. The built-in engine removes noise with noise-adaptive wavelet shrinkage and enlarges
//! with Lanczos-3 refined by back-projection; an external engine (e.g. Real-ESRGAN) plugs in as a command.
//!
//! ponytail: no neural model is bundled, so the built-in engine is classical: it removes noise well but
//! cannot invent detail the way a trained model does; the command engine is where such a model runs.

use crate::{
    ai,
    develop::{self, Img, es},
};
use rayon::prelude::*;
use std::{
    f32::consts::PI,
    io::Read,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// Noise σ of each à trous scale for unit white noise (B3 spline, Starck & Murtagh).
const SCALE_NOISE: [f32; 6] = [0.8908, 0.2007, 0.0856, 0.0413, 0.0205, 0.0103];

/// Reflect an index into 0..n without repeating the edge sample.
fn mirror(i: isize, n: usize) -> usize {
    let n = n as isize;
    let i = if i < 0 { -i } else if i >= n { 2 * (n - 1) - i } else { i };
    i.clamp(0, n - 1) as usize
}

/// One à trous step: the B3 spline [1, 4, 6, 4, 1] / 16 with its taps `step` apart, edges mirrored.
fn atrous(src: &[f32], w: usize, h: usize, step: usize) -> Vec<f32> {
    let s = step as isize;
    let mut tmp = vec![0.0; w * h];
    tmp.par_chunks_mut(w).zip(src.par_chunks(w)).for_each(|(o, r)| {
        for (x, v) in o.iter_mut().enumerate() {
            let at = |d: isize| r[mirror(x as isize + d, w)];
            *v = (at(-2 * s) + at(2 * s) + 4.0 * (at(-s) + at(s)) + 6.0 * at(0)) / 16.0;
        }
    });
    let mut out = vec![0.0; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, o)| {
        let row = |d: isize| {
            let y = mirror(y as isize + d, h);
            &tmp[y * w..(y + 1) * w]
        };
        let (a, b, c, d, e) = (row(-2 * s), row(-s), row(0), row(s), row(2 * s));
        for x in 0..w {
            o[x] = (a[x] + e[x] + 4.0 * (b[x] + d[x]) + 6.0 * c[x]) / 16.0;
        }
    });
    out
}

/// Noise σ as a function of brightness (`level`, a smooth copy of lightness, in 16 bins over lo..hi): the
/// median absolute finest-scale coefficient per bin, since noise in raw data grows with the signal. Bins
/// with few pixels take their neighbours' value.
fn noise_curve(fine: &[f32], level: &[f32], lo: f32, hi: f32) -> [f32; 16] {
    let mut bins: Vec<Vec<f32>> = vec![vec![]; 16];
    for (&d, &l) in fine.iter().zip(level).step_by(3) {
        bins[((((l - lo) / (hi - lo).max(1e-6)) * 16.0).max(0.0) as usize).min(15)].push(d.abs());
    }
    let mut s = [f32::NAN; 16];
    for (k, b) in bins.iter_mut().enumerate().filter(|(_, b)| b.len() >= 200) {
        let m = b.len() / 2;
        s[k] = *b.select_nth_unstable_by(m, f32::total_cmp).1 / 0.6745 / SCALE_NOISE[0];
    }
    for k in 1..16 {
        if s[k].is_nan() {
            s[k] = s[k - 1];
        }
    }
    for k in (0..15).rev() {
        if s[k].is_nan() {
            s[k] = s[k + 1];
        }
    }
    s.map(|v| if v.is_nan() { 0.0 } else { v })
}

/// Wavelet shrinkage of one plane: the coefficients of à trous scale j pass a non-negative garrote at
/// k[j] × that scale's share of the noise σ at the pixel's brightness.
fn shrink(p: &[f32], w: usize, h: usize, level: &[f32], sigma: &(dyn Fn(f32) -> f32 + Sync), k: &[f32]) -> Vec<f32> {
    let mut c = p.to_vec();
    let mut out = vec![0.0; w * h];
    for (j, &kj) in k.iter().enumerate() {
        let next = atrous(&c, w, h, 1 << j);
        out.par_iter_mut().zip(&c).zip(&next).zip(level).for_each(|(((o, &a), &b), &l)| {
            let (d, t) = (a - b, kj * sigma(l) * SCALE_NOISE[j]);
            if d.abs() > t {
                *o += d - t * t / d;
            }
        });
        c = next;
    }
    out.par_iter_mut().zip(&c).for_each(|(o, &v)| *o += v);
    out
}

/// Split into Oklab planes (the image's memory is released), and back.
fn planes(img: Img) -> (usize, usize, [Vec<f32>; 3]) {
    let lab: Vec<[f32; 3]> = img.px.into_par_iter().map(develop::oklab).collect();
    let ch = std::array::from_fn(|c| lab.par_iter().map(|q| q[c]).collect());
    (img.w, img.h, ch)
}
fn join(w: usize, h: usize, ch: [Vec<f32>; 3]) -> Img {
    let px = (0..w * h).into_par_iter().map(|i| develop::from_oklab([ch[0][i].max(0.0), ch[1][i], ch[2][i]])).collect();
    Img { w, h, px }
}

/// Remove noise from Oklab lightness and colour separately, each by à trous wavelet shrinkage driven by
/// the noise measured on this photo at each brightness. `strength` 0-100 sets the lightness thresholds
/// (up to 3σ), `detail` 0-100 keeps the finest texture, `color` 0-100 the colour thresholds; colour noise
/// is blotchy, so its coarser scales get higher thresholds.
pub fn denoise(img: Img, strength: f32, detail: f32, color: f32) -> Img {
    let (w, h, mut ch) = planes(img);
    if w < 8 || h < 8 {
        return join(w, h, ch);
    }
    let level = atrous(&atrous(&ch[0], w, h, 1), w, h, 2);
    let mut sorted: Vec<f32> = level.iter().step_by(7).copied().collect();
    sorted.sort_by(f32::total_cmp);
    let (lo, hi) = (sorted[sorted.len() / 1000], sorted[sorted.len() - 1 - sorted.len() / 1000]);
    for (c, plane) in ch.iter_mut().enumerate() {
        let fine: Vec<f32> = plane.par_iter().zip(atrous(plane, w, h, 1)).map(|(a, b)| a - b).collect();
        let curve = noise_curve(&fine, &level, lo, hi);
        drop(fine);
        let sigma = move |l: f32| {
            let t = ((l - lo) / (hi - lo).max(1e-6) * 16.0 - 0.5).clamp(0.0, 15.0);
            let k = (t as usize).min(14);
            curve[k] + (curve[k + 1] - curve[k]) * (t - k as f32)
        };
        let k: Vec<f32> = if c == 0 {
            let base = 3.0 * strength.clamp(0.0, 100.0) / 100.0;
            vec![base * (1.5 - detail.clamp(0.0, 100.0) / 100.0), base, base, 0.9 * base]
        } else {
            let base = 0.5 + 3.5 * color.clamp(0.0, 100.0) / 100.0;
            (0..5).map(|j| base * (1.0 + 0.5 * j as f32)).collect()
        };
        *plane = shrink(plane, w, h, &level, &sigma, &k);
    }
    join(w, h, ch)
}

fn lanczos3(x: f32) -> f32 {
    if x.abs() < 1e-6 {
        1.0
    } else if x.abs() >= 3.0 {
        0.0
    } else {
        let p = PI * x;
        3.0 * p.sin() * (p / 3.0).sin() / (p * p)
    }
}
/// A sensor pixel's footprint: what shrinking averages over.
fn boxed(x: f32) -> f32 {
    if x.abs() < 0.5 { 1.0 } else { 0.0 }
}

/// Taps per output sample, sample centres aligned; the kernel widens when shrinking (antialiasing) and is
/// renormalised where it is cut by an edge.
fn taps(n: usize, nn: usize, kernel: fn(f32) -> f32, support: f32) -> Vec<(usize, Vec<f32>)> {
    let scale = nn as f32 / n as f32;
    let fs = if scale < 1.0 { 1.0 / scale } else { 1.0 };
    (0..nn)
        .map(|o| {
            let c = (o as f32 + 0.5) / scale - 0.5;
            let a = ((c - support * fs).floor().max(0.0) as usize).min(n - 1);
            let b = ((c + support * fs).ceil().max(0.0) as usize).min(n - 1);
            let mut ws: Vec<f32> = (a..=b).map(|i| kernel((i as f32 - c) / fs)).collect();
            let s: f32 = ws.iter().sum();
            if s.abs() < 1e-6 {
                let k = (c.round().max(0.0) as usize).min(n - 1);
                return (k, vec![1.0]);
            }
            ws.iter_mut().for_each(|v| *v /= s);
            (a, ws)
        })
        .collect()
}

/// Separable resampling of a plane to nw × nh.
fn resample(src: &[f32], w: usize, h: usize, nw: usize, nh: usize, kernel: fn(f32) -> f32, support: f32) -> Vec<f32> {
    let (tx, ty) = (taps(w, nw, kernel, support), taps(h, nh, kernel, support));
    let mut tmp = vec![0.0; nw * h];
    tmp.par_chunks_mut(nw).zip(src.par_chunks(w)).for_each(|(o, r)| {
        for (v, (a, ws)) in o.iter_mut().zip(&tx) {
            *v = ws.iter().zip(&r[*a..]).map(|(k, s)| k * s).sum();
        }
    });
    let mut out = vec![0.0; nw * nh];
    out.par_chunks_mut(nw).zip(&ty).for_each(|(o, (a, ws))| {
        for (k, wk) in ws.iter().enumerate() {
            let row = &tmp[(a + k) * nw..(a + k + 1) * nw];
            o.iter_mut().zip(row).for_each(|(v, s)| *v += wk * s);
        }
    });
    out
}

/// Enlarge by `scale`: colour by Lanczos-3; lightness by Lanczos-3 refined by back-projection (the result,
/// averaged again over each original pixel's footprint, must give the original back), then `detail` 0-100
/// of sharpening at the new pixel size, gated to edges so flat areas and noise are left alone.
pub fn upscale(img: Img, scale: f32, detail: f32) -> Img {
    let (w, h, ch) = planes(img);
    let (nw, nh) = (((w as f32 * scale).round() as usize).max(1), ((h as f32 * scale).round() as usize).max(1));
    let up = |p: &[f32]| resample(p, w, h, nw, nh, lanczos3, 3.0);
    let [l0, a, b] = ch;
    let (a, b) = (up(&a), up(&b));
    let mut l = up(&l0);
    for _ in 0..3 {
        let down = resample(&l, nw, nh, w, h, boxed, 0.5);
        let err: Vec<f32> = l0.par_iter().zip(&down).map(|(o, d)| o - d).collect();
        l.par_iter_mut().zip(up(&err)).for_each(|(v, e)| *v += e);
    }
    if detail > 0.0 {
        let soft = develop::blur(&l, nw, nh, 0.6 * scale.max(1.0));
        let amount = 0.8 * detail.clamp(0.0, 100.0) / 100.0;
        l.par_iter_mut().zip(&soft).for_each(|(v, s)| {
            let d = *v - s;
            *v += amount * d * develop::smooth(0.002, 0.01, d.abs());
        });
    }
    join(nw, nh, [l, a, b])
}

/// Run an external engine: `template` is split like a shell (no shell runs) and {{INPUT}}, {{OUTPUT}},
/// {{SCALE}} (1-4), {{STRENGTH}} (0-100) and {{NOISE}} (0-3) are filled in; it must write `output`.
pub fn run_command(template: &str, input: &Path, output: &Path, scale: u32, strength: f32, timeout: Duration) -> Result<(), String> {
    let vars = [
        ("INPUT", input.display().to_string()),
        ("OUTPUT", output.display().to_string()),
        ("SCALE", scale.to_string()),
        ("STRENGTH", format!("{}", strength.round())),
        ("NOISE", format!("{}", (strength / 100.0 * 3.0).round().clamp(0.0, 3.0))),
    ];
    let argv: Vec<String> = shlex::split(template)
        .ok_or("the engine command has unbalanced quotes")?
        .into_iter()
        .map(|w| vars.iter().fold(w, |w, (k, v)| w.replace(&format!("{{{{{k}}}}}"), v)))
        .collect();
    let (prog, args) = argv.split_first().ok_or("no engine command set")?;
    let path = ai::find_binary(prog);
    let mut child = Command::new(&path)
        .args(args)
        .env("PATH", ai::user_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start '{prog}' ({}): {e}", path.display()))?;
    let mut err = child.stderr.take();
    let drain = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_string(&mut s);
        }
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(es)? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(format!("{prog} took longer than {}s", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let stderr = drain.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("{prog} exited with {status}: {}", stderr.trim().chars().take(400).collect::<String>()));
    }
    if !output.is_file() {
        return Err(format!("{prog} finished without writing {}", output.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle(x: f32) -> f32 {
        (1.0 - x.abs()).max(0.0)
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 24) as f32
        }
        fn gauss(&mut self) -> f32 {
            (-2.0 * (self.next() + 1e-7).ln()).sqrt() * (2.0 * PI * self.next()).cos()
        }
    }

    /// A smooth gradient with discs and stripes, in linear light.
    fn scene(w: usize, h: usize) -> Img {
        let px = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                let base = 0.05 + 0.4 * x / w as f32;
                let disc = if (x - w as f32 * 0.6).hypot(y - h as f32 * 0.5) < h as f32 * 0.25 { 0.3 } else { 0.0 };
                let stripes = if ((y / 6.0) as usize).is_multiple_of(2) && x < w as f32 * 0.3 { 0.15 } else { 0.0 };
                let v = base + disc + stripes;
                [v, v * 0.9, v * 0.7]
            })
            .collect();
        Img { w, h, px }
    }

    /// PSNR in dB on display values.
    fn psnr(a: &Img, b: &Img) -> f32 {
        let (x, y) = (develop::to_rgba8(a), develop::to_rgba8(b));
        let mse = x.iter().zip(&y).map(|(p, q)| (*p as f32 - *q as f32).powi(2)).sum::<f32>() / x.len() as f32;
        10.0 * (255.0f32 * 255.0 / mse.max(1e-6)).log10()
    }

    #[test]
    fn denoise_removes_shot_noise_and_keeps_a_clean_photo() {
        let (w, h) = (320, 240);
        let clean = scene(w, h);
        let mut r = Rng(3);
        // shot noise: grows with the signal
        let noisy = Img { w, h, px: clean.px.iter().map(|p| p.map(|v| (v + (0.0004 + 0.002 * v).sqrt() * r.gauss()).max(0.0))).collect() };
        let before = psnr(&clean, &noisy);
        let after = psnr(&clean, &denoise(noisy, 60.0, 50.0, 60.0));
        assert!(after > before + 5.0, "PSNR {before:.1} -> {after:.1} dB");
        let untouched = psnr(&clean, &denoise(clean.clone(), 60.0, 50.0, 60.0));
        assert!(untouched > 40.0, "a clean photo changed: {untouched:.1} dB");
    }

    #[test]
    fn upscale_beats_plain_interpolation() {
        let (w, h) = (240, 160);
        let full = scene(w, h);
        // shrink by 2 over each pixel's footprint, then enlarge again
        let shrink = |img: &Img| -> Img {
            let (w2, h2) = (w / 2, h / 2);
            let px = (0..w2 * h2)
                .map(|i| {
                    let (x, y) = (2 * (i % w2), 2 * (i / w2));
                    let q = [img.px[y * w + x], img.px[y * w + x + 1], img.px[(y + 1) * w + x], img.px[(y + 1) * w + x + 1]];
                    [0, 1, 2].map(|c| q.iter().map(|p| p[c]).sum::<f32>() / 4.0)
                })
                .collect();
            Img { w: w2, h: h2, px }
        };
        let small = shrink(&full);
        let sr = upscale(small.clone(), 2.0, 0.0);
        assert_eq!((sr.w, sr.h), (w, h));
        let lanczos = {
            let ch: [Vec<f32>; 3] = std::array::from_fn(|c| resample(&small.px.iter().map(|p| p[c]).collect::<Vec<_>>(), w / 2, h / 2, w, h, lanczos3, 3.0));
            Img { w, h, px: (0..w * h).map(|i| [ch[0][i], ch[1][i], ch[2][i]]).collect() }
        };
        let plain = {
            let ch: [Vec<f32>; 3] = std::array::from_fn(|c| resample(&small.px.iter().map(|p| p[c]).collect::<Vec<_>>(), w / 2, h / 2, w, h, triangle, 1.0));
            Img { w, h, px: (0..w * h).map(|i| [ch[0][i], ch[1][i], ch[2][i]]).collect() }
        };
        let (a, b, c) = (psnr(&full, &sr), psnr(&full, &plain), psnr(&full, &lanczos));
        assert!(a > b + 1.0 && a > c + 0.5, "super resolution {a:.1} dB vs bilinear {b:.1} dB, Lanczos {c:.1} dB");
        // consistent with its input: shrinking the result gives the input back
        assert!(psnr(&small, &shrink(&sr)) > 38.0, "{}", psnr(&small, &shrink(&sr)));
    }

    #[test]
    fn resampling_keeps_flat_areas_and_edges_in_place() {
        let flat = vec![0.5f32; 30 * 20];
        for (nw, nh) in [(60, 40), (13, 9), (30, 20)] {
            assert!(resample(&flat, 30, 20, nw, nh, lanczos3, 3.0).iter().all(|v| (v - 0.5).abs() < 1e-5));
        }
        // a step at x = 15 lands at x = 30 after doubling
        let step: Vec<f32> = (0..30 * 4).map(|i| if i % 30 < 15 { 0.0 } else { 1.0 }).collect();
        let up = resample(&step, 30, 4, 60, 8, lanczos3, 3.0);
        assert!(up[29] < 0.5 && up[30] > 0.5, "{} {}", up[29], up[30]);
    }

    #[test]
    #[cfg(unix)]
    fn command_engine_fills_in_the_template() {
        let dir = std::env::temp_dir().join(format!("ols-enhance-test-{}", crate::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, output) = (dir.join("in.png"), dir.join("out.png"));
        std::fs::write(&input, b"pixels").unwrap();
        run_command("cp {{INPUT}} {{OUTPUT}}", &input, &output, 2, 50.0, Duration::from_secs(10)).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), b"pixels");
        let e = run_command("sh -c 'echo broken model >&2; exit 3'", &input, &dir.join("x.png"), 2, 50.0, Duration::from_secs(10)).unwrap_err();
        assert!(e.contains("broken model"), "{e}");
        assert!(run_command("true", &input, &dir.join("never.png"), 2, 50.0, Duration::from_secs(10)).unwrap_err().contains("without writing"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
