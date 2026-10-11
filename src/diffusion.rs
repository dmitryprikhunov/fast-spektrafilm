//! Optical filter PSFs ported from the pinned experimental diffusion model.
#![allow(clippy::needless_range_loop)]
use crate::{Image, interpolation::linear, params::DiffusionFilter};
use anyhow::{Result, ensure};
use rustfft::{FftPlanner, num_complex::Complex};
use serde_json::Value;
use std::sync::OnceLock;
fn presets() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| serde_json::from_str(include_str!("../data/diffusion.json")).unwrap())
}
pub fn validate(p: &DiffusionFilter) -> Result<()> {
    ensure!(
        presets()["shapes"].get(&p.filter_family).is_some(),
        "Unknown diffusion filter family: {}",
        p.filter_family
    );
    ensure!(
        [
            p.strength,
            p.spatial_scale,
            p.halo_warmth,
            p.core_intensity,
            p.core_size,
            p.halo_intensity,
            p.halo_size,
            p.bloom_intensity,
            p.bloom_size
        ]
        .iter()
        .all(|v| v.is_finite()),
        "Diffusion parameters must be finite"
    );
    Ok(())
}
fn reflect(i: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let period = 2 * (n - 1) as isize;
    let j = i.rem_euclid(period);
    if j < n as isize {
        j as usize
    } else {
        (period - j) as usize
    }
}
fn fft2(
    values: &mut [Complex<f32>],
    w: usize,
    h: usize,
    inverse: bool,
    planner: &mut FftPlanner<f32>,
) {
    let row = if inverse {
        planner.plan_fft_inverse(w)
    } else {
        planner.plan_fft_forward(w)
    };
    for r in values.chunks_exact_mut(w) {
        row.process(r);
    }
    let column = if inverse {
        planner.plan_fft_inverse(h)
    } else {
        planner.plan_fft_forward(h)
    };
    let mut scratch = vec![Complex::default(); h];
    for x in 0..w {
        for y in 0..h {
            scratch[y] = values[y * w + x];
        }
        column.process(&mut scratch);
        for y in 0..h {
            values[y * w + x] = scratch[y];
        }
    }
}
pub fn apply(image: &Image, p: &DiffusionFilter, pixel_um: f64) -> Result<Image> {
    validate(p)?;
    if !p.active || p.strength <= 0. || p.spatial_scale <= 0. {
        return Ok(image.clone());
    }
    ensure!(
        pixel_um.is_finite() && pixel_um > 0.,
        "Invalid diffusion pixel size"
    );
    let cfg = &presets()["shapes"][&p.filter_family];
    let number = |v: &Value, key: &str| v[key].as_f64().unwrap();
    let mut weights = [
        number(cfg, "w_c") * p.core_intensity.max(0.),
        number(cfg, "w_h") * p.halo_intensity.max(0.),
        number(cfg, "w_b") * p.bloom_intensity.max(0.),
    ];
    let total: f64 = weights.iter().sum();
    let sizes = if total <= 0. {
        weights = [number(cfg, "w_c"), number(cfg, "w_h"), number(cfg, "w_b")];
        [1.; 3]
    } else {
        for v in &mut weights {
            *v /= total;
        }
        [p.core_size, p.halo_size, p.bloom_size].map(|v| v.max(1e-6))
    };
    let max_lambda = number(&cfg["bloom"], "lambda_um")
        * sizes[2]
        * number(&cfg["bloom"], "spread")
        * p.spatial_scale
        / pixel_um;
    let radius = ((8. * max_lambda).max(5.).ceil() as usize)
        .min((image.width.min(image.height) / 2).saturating_sub(1).max(1));
    let side = 2 * radius + 1;
    let warmth = (number(cfg, "halo_warmth_base") + p.halo_warmth).clamp(-1.5, 1.5);
    let mut kernel = vec![[0.; 3]; side * side];
    for (g, name) in ["core", "halo", "bloom"].iter().enumerate() {
        let group = &cfg[name];
        let center = number(group, "lambda_um") * sizes[g];
        let spread = number(group, "spread");
        let n = group["n_components"].as_u64().unwrap() as usize;
        let lambdas: Vec<_> = (0..n)
            .map(|i| {
                ((center / spread).ln()
                    + i as f64 / (n - 1) as f64 * ((center * spread).ln() - (center / spread).ln()))
                .exp()
            })
            .collect();
        let mut inner: Vec<_> = lambdas
            .iter()
            .map(|v| {
                if g == 2 {
                    v.powf(2. - number(group, "alpha"))
                } else {
                    1.
                }
            })
            .collect();
        let sum: f64 = inner.iter().sum();
        for v in &mut inner {
            *v /= sum;
        }
        let mean: f64 = inner
            .iter()
            .enumerate()
            .map(|(i, v)| v * (-1. + 2. * i as f64 / (n - 1) as f64))
            .sum();
        for c in 0..3 {
            let mut cw = inner.clone();
            if g == 1 {
                for (i, v) in cw.iter_mut().enumerate() {
                    *v *= (1.
                        + warmth
                            * [1.30, 0.15, -1.45][c]
                            * (-1. + 2. * i as f64 / (n - 1) as f64 - mean))
                        .max(0.);
                }
                let sum: f64 = cw.iter().sum();
                for v in &mut cw {
                    *v /= sum;
                }
            }
            for y in 0..side {
                for x in 0..side {
                    let r = ((x as f64 - radius as f64).powi(2)
                        + (y as f64 - radius as f64).powi(2))
                    .sqrt();
                    for (l, v) in lambdas.iter().zip(&cw) {
                        let l = (l * p.spatial_scale / pixel_um).max(1e-6);
                        kernel[y * side + x][c] +=
                            weights[g] * v * (-r / l).exp() / (2. * std::f64::consts::PI * l * l);
                    }
                }
            }
        }
    }
    for c in 0..3 {
        let sum: f64 = kernel.iter().map(|v| v[c]).sum();
        for v in &mut kernel {
            v[c] /= sum;
        }
    }
    let breaks: Vec<_> = presets()["breakpoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap().log2())
        .collect();
    let fractions: Vec<_> = presets()["fractions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let fraction = (linear(p.strength.max(1e-6).log2(), &breaks, &fractions)
        * presets()["gains"][&p.filter_family].as_f64().unwrap())
    .clamp(0., 0.99);
    let pw = image.width + 2 * radius;
    let ph = image.height + 2 * radius;
    let fw = (pw + side - 1).next_power_of_two();
    let fh = (ph + side - 1).next_power_of_two();
    let mut planner = FftPlanner::new();
    let mut output = image.clone();
    for c in 0..image.channels {
        let mut a = vec![Complex::default(); fw * fh];
        let mut b = a.clone();
        for y in 0..ph {
            for x in 0..pw {
                a[y * fw + x].re = image.data[(reflect(y as isize - radius as isize, image.height)
                    * image.width
                    + reflect(x as isize - radius as isize, image.width))
                    * image.channels
                    + c] as f32;
            }
        }
        for y in 0..side {
            for x in 0..side {
                b[y * fw + x].re = kernel[y * side + x][c] as f32;
            }
        }
        fft2(&mut a, fw, fh, false, &mut planner);
        fft2(&mut b, fw, fh, false, &mut planner);
        for (a, b) in a.iter_mut().zip(b) {
            *a *= b;
        }
        fft2(&mut a, fw, fh, true, &mut planner);
        for y in 0..image.height {
            for x in 0..image.width {
                let i = (y * image.width + x) * image.channels + c;
                let blurred = a[(y + 2 * radius) * fw + x + 2 * radius].re / (fw * fh) as f32;
                output.data[i] =
                    image.data[i] * (1. - fraction) + (blurred * fraction as f32) as f64;
            }
        }
    }
    Ok(output)
}
