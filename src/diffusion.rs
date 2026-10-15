//! Optical filter PSFs ported from the pinned experimental diffusion model.
#![allow(clippy::needless_range_loop)]
use crate::{Image, interpolation::linear, params::DiffusionFilter};
use anyhow::{Result, ensure};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use serde_json::Value;
use std::sync::{Arc, Mutex, OnceLock};
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
struct Plans {
    rows: [Arc<dyn Fft<f32>>; 2],
    columns: [Arc<dyn Fft<f32>>; 2],
}
impl Plans {
    fn get(w: usize, h: usize) -> Arc<Self> {
        static CACHE: OnceLock<Mutex<crate::cache::ByteCache<Plans>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(crate::cache::ByteCache::new(4 * 1024 * 1024)));
        let key = format!("{w}x{h}");
        if let Some(p) = cache.lock().unwrap().get(&key) {
            return p;
        }
        let mut planner = FftPlanner::new();
        let plans = Arc::new(Self {
            rows: [planner.plan_fft_forward(w), planner.plan_fft_inverse(w)],
            columns: [planner.plan_fft_forward(h), planner.plan_fft_inverse(h)],
        });
        // Conservative bound for radix-2 twiddle/algorithm storage, independent
        // of image/channel count. Large plans are used without retention.
        cache
            .lock()
            .unwrap()
            .insert(key, 128 * (w + h), plans.clone());
        plans
    }
}
struct Scratch {
    column: Vec<Complex<f32>>,
    fft: Vec<Complex<f32>>,
}
impl Scratch {
    fn new(plans: &Plans, h: usize) -> Self {
        let n = plans
            .rows
            .iter()
            .chain(&plans.columns)
            .map(|p| p.get_inplace_scratch_len())
            .max()
            .unwrap();
        Self {
            column: vec![Complex::default(); h],
            fft: vec![Complex::default(); n],
        }
    }
}
fn fft2(
    values: &mut [Complex<f32>],
    w: usize,
    h: usize,
    inverse: bool,
    plans: &Plans,
    scratch: &mut Scratch,
) {
    let direction = usize::from(inverse);
    for row in values.chunks_exact_mut(w) {
        plans.rows[direction].process_with_scratch(row, &mut scratch.fft);
    }
    for x in 0..w {
        for y in 0..h {
            scratch.column[y] = values[y * w + x];
        }
        plans.columns[direction].process_with_scratch(&mut scratch.column, &mut scratch.fft);
        for y in 0..h {
            values[y * w + x] = scratch.column[y];
        }
    }
}
pub fn apply(image: &Image, p: &DiffusionFilter, pixel_um: f64) -> Result<Image> {
    apply_owned(image.clone(), p, pixel_um)
}
/// Owned no-op calls preserve allocation; active calls overwrite a channel only
/// after its entire reflected source has been copied into the FFT workspace.
pub fn apply_owned(image: Image, p: &DiffusionFilter, pixel_um: f64) -> Result<Image> {
    apply_impl(image, p, pixel_um, true)
}
fn apply_impl(mut image: Image, p: &DiffusionFilter, pixel_um: f64, cached: bool) -> Result<Image> {
    validate(p)?;
    image.validate()?;
    if !p.active || p.strength <= 0. || p.spatial_scale <= 0. {
        return Ok(image);
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
    static PSFS: OnceLock<Mutex<crate::cache::ByteCache<Vec<[f64; 3]>>>> = OnceLock::new();
    let psfs = PSFS.get_or_init(|| Mutex::new(crate::cache::ByteCache::new(16 * 1024 * 1024)));
    let psf_key = serde_json::to_string(&(image.width, image.height, pixel_um.to_bits(), p))?;
    let found = if cached {
        psfs.lock().unwrap().get(&psf_key)
    } else {
        None
    };
    let kernel = if let Some(kernel) = found {
        kernel
    } else {
        let mut kernel = vec![[0.; 3]; side * side];
        for (g, name) in ["core", "halo", "bloom"].iter().enumerate() {
            let group = &cfg[name];
            let center = number(group, "lambda_um") * sizes[g];
            let spread = number(group, "spread");
            let n = group["n_components"].as_u64().unwrap() as usize;
            let lambdas: Vec<_> = (0..n)
                .map(|i| {
                    ((center / spread).ln()
                        + i as f64 / (n - 1) as f64
                            * ((center * spread).ln() - (center / spread).ln()))
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
                            kernel[y * side + x][c] += weights[g] * v * (-r / l).exp()
                                / (2. * std::f64::consts::PI * l * l);
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
        let kernel = Arc::new(kernel);
        if cached {
            psfs.lock()
                .unwrap()
                .insert(psf_key.clone(), kernel.len() * 24, kernel.clone());
        }
        kernel
    };
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
    let plans = Plans::get(fw, fh);
    let mut scratch = Scratch::new(&plans, fh);
    static TRANSFORMS: OnceLock<Mutex<crate::cache::ByteCache<Vec<Complex<f32>>>>> =
        OnceLock::new();
    let transforms =
        TRANSFORMS.get_or_init(|| Mutex::new(crate::cache::ByteCache::new(64 * 1024 * 1024)));
    let mut a = vec![Complex::default(); fw * fh];
    for c in 0..image.channels {
        a.fill(Complex::default());
        for y in 0..ph {
            for x in 0..pw {
                a[y * fw + x].re = image.data[(reflect(y as isize - radius as isize, image.height)
                    * image.width
                    + reflect(x as isize - radius as isize, image.width))
                    * image.channels
                    + c] as f32;
            }
        }
        let transform_key = format!("{psf_key}:{fw}:{fh}:{c}");
        let found = if cached {
            transforms.lock().unwrap().get(&transform_key)
        } else {
            None
        };
        let b = if let Some(b) = found {
            b
        } else {
            let mut b = vec![Complex::default(); fw * fh];
            for y in 0..side {
                for x in 0..side {
                    b[y * fw + x].re = kernel[y * side + x][c] as f32;
                }
            }
            fft2(&mut b, fw, fh, false, &plans, &mut scratch);
            let b = Arc::new(b);
            if cached {
                transforms
                    .lock()
                    .unwrap()
                    .insert(transform_key, b.len() * 8, b.clone());
            }
            b
        };
        fft2(&mut a, fw, fh, false, &plans, &mut scratch);
        for (a, b) in a.iter_mut().zip(b.iter()) {
            *a *= *b;
        }
        fft2(&mut a, fw, fh, true, &plans, &mut scratch);
        for y in 0..image.height {
            for x in 0..image.width {
                let i = (y * image.width + x) * image.channels + c;
                let blurred = a[(y + 2 * radius) * fw + x + 2 * radius].re / (fw * fh) as f32;
                image.data[i] =
                    image.data[i] * (1. - fraction) + (blurred * fraction as f32) as f64;
            }
        }
    }
    Ok(image)
}

#[derive(serde::Serialize)]
pub struct WorkspaceEstimate {
    pub fft_shape: [usize; 2],
    pub psf_bytes: usize,
    /// Two live complex planes plus reusable FFT/column scratch. Excludes image,
    /// allocator overhead, FFT plan internals and separately retained cache data.
    pub live_fft_vector_bytes: usize,
    pub cache_byte_limit: usize,
}
pub fn workspace_estimate(
    width: usize,
    height: usize,
    p: &DiffusionFilter,
    pixel_um: f64,
) -> Result<WorkspaceEstimate> {
    validate(p)?;
    ensure!(
        width > 0 && height > 0 && pixel_um.is_finite() && pixel_um > 0.,
        "Invalid diffusion dimensions/spacing"
    );
    let cfg = &presets()["shapes"][&p.filter_family];
    let total = cfg["w_c"].as_f64().unwrap() * p.core_intensity.max(0.)
        + cfg["w_h"].as_f64().unwrap() * p.halo_intensity.max(0.)
        + cfg["w_b"].as_f64().unwrap() * p.bloom_intensity.max(0.);
    let bloom_size = if total <= 0. {
        1.
    } else {
        p.bloom_size.max(1e-6)
    };
    let lambda = cfg["bloom"]["lambda_um"].as_f64().unwrap()
        * bloom_size
        * cfg["bloom"]["spread"].as_f64().unwrap()
        * p.spatial_scale
        / pixel_um;
    let radius = ((8. * lambda).max(5.).ceil() as usize)
        .min((width.min(height) / 2).saturating_sub(1).max(1));
    let w = (width + 4 * radius)
        .checked_next_power_of_two()
        .ok_or_else(|| anyhow::anyhow!("FFT dimensions overflow"))?;
    let h = (height + 4 * radius)
        .checked_next_power_of_two()
        .ok_or_else(|| anyhow::anyhow!("FFT dimensions overflow"))?;
    let plans = Plans::get(w, h);
    let scratch = plans
        .rows
        .iter()
        .chain(&plans.columns)
        .map(|v| v.get_inplace_scratch_len())
        .max()
        .unwrap();
    Ok(WorkspaceEstimate {
        fft_shape: [h, w],
        psf_bytes: (2 * radius + 1).pow(2) * 24,
        live_fft_vector_bytes: (2 * w * h + h + scratch) * 8,
        cache_byte_limit: (4 + 16 + 64) * 1024 * 1024,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owned_noop_reuses_storage() -> Result<()> {
        let input = Image::new(3, 1, 3, vec![0.2; 9])?;
        let ptr = input.data.as_ptr();
        let output = apply_owned(input, &DiffusionFilter::default(), 10.)?;
        assert_eq!(ptr, output.data.as_ptr());
        Ok(())
    }
    #[test]
    fn all_diffusion_keys_match_uncached_arithmetic() -> Result<()> {
        let input = Image::new(
            11,
            9,
            3,
            (0..297)
                .map(|i| if i == 0 { 16. } else { (i % 23) as f64 / 23. })
                .collect(),
        )?;
        for family in ["black_pro_mist", "pro_mist", "glimmerglass", "cinebloom"] {
            let p = DiffusionFilter {
                active: true,
                filter_family: family.into(),
                ..Default::default()
            };
            for pixel in [35_000. / 256., 1_500. / 256.] {
                let mut variants = vec![p.clone()];
                for field in [
                    "strength",
                    "spatial_scale",
                    "halo_warmth",
                    "core_intensity",
                    "core_size",
                    "halo_intensity",
                    "halo_size",
                    "bloom_intensity",
                    "bloom_size",
                ] {
                    let mut v = serde_json::to_value(&p)?;
                    v[field] = serde_json::json!(v[field].as_f64().unwrap() * 1.2 + 0.1);
                    variants.push(serde_json::from_value(v)?);
                }
                for p in variants {
                    let expected = apply_impl(input.clone(), &p, pixel, false)?;
                    assert_eq!(apply_owned(input.clone(), &p, pixel)?.data, expected.data);
                    assert_eq!(apply_owned(input.clone(), &p, pixel)?.data, expected.data);
                }
            }
        }
        Ok(())
    }
}
