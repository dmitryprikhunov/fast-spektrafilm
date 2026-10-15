// Indexing preserves explicit wavelength/channel axes in spectral sums.
#![allow(clippy::needless_range_loop)]
use crate::{
    color::{Tables, knee},
    image::read_npy,
    interpolation::{Lut2, tri_to_quad},
    params::Params,
    profile::Profile,
};
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use std::path::Path;

pub struct FilmResponse {
    pub lut: Lut2,
    pub projection_white: [f64; 2],
}
impl FilmResponse {
    pub(crate) fn shared(
        data: &Path,
        t: &Tables,
        film: &Profile,
        p: &Params,
    ) -> Result<std::sync::Arc<Self>> {
        use std::sync::{Arc, Mutex, OnceLock};
        static CACHE: OnceLock<Mutex<crate::cache::ByteCache<FilmResponse>>> = OnceLock::new();
        let key = crate::cache::data_key(data)?
            + &serde_json::to_string(&(
                &film.sensitivity,
                &film.window,
                &film.surface,
                &film.info.reference_illuminant,
                &p.settings.rgb_to_raw_method,
                p.settings.apply_hanatos2025_adaptation_window,
                p.settings.apply_hanatos2025_adaptation_surface,
                p.settings.spectral_gaussian_blur,
                &p.camera.color_filter,
                &p.io.input_gamut_compress,
            ))?;
        let cache =
            CACHE.get_or_init(|| Mutex::new(crate::cache::ByteCache::new(64 * 1024 * 1024)));
        if let Some(value) = cache.lock().unwrap().get(&key) {
            return Ok(value);
        }
        let response = Arc::new(Self::new(data, t, film, p)?);
        cache
            .lock()
            .unwrap()
            .insert(key, response.lut.data.len() * 8, response.clone());
        Ok(response)
    }
    pub fn new(data: &Path, t: &Tables, film: &Profile, p: &Params) -> Result<Self> {
        let method = &p.settings.rgb_to_raw_method;
        ensure!(
            [
                "hanatos2025",
                "arctic2026beta04",
                "jakob2019",
                "mallett2019",
                "otsu2018",
                "gauss-lasers"
            ]
            .contains(&method.as_str()),
            "Unknown spectral upsampling method {method}"
        );
        let file_method = method.replace('-', "_");
        let descriptor: toml::Value = toml::from_str(&std::fs::read_to_string(
            data.join("luts/spectral_upsampling").join(format!(
                "{file_method}_{}",
                if method == "hanatos2025" {
                    "irradiance_xy_tc.toml"
                } else {
                    "reflectance_xy_tc.toml"
                }
            )),
        )?)?;
        let file = descriptor
            .get("file")
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{method}_{}_xy_tc.npy",
                    if method == "hanatos2025" {
                        "irradiance"
                    } else {
                        "reflectance"
                    }
                )
            });
        let (shape, spectra) = read_npy(&data.join("luts/spectral_upsampling").join(file))?;
        ensure!(
            shape.len() == 3 && shape[0] == shape[1] && shape[2] >= 81,
            "Unsupported spectral LUT shape"
        );
        let size = shape[0];
        let wavelengths = shape[2];
        let channels = film.channels();
        let mut spectra = Lut2 {
            size,
            channels: wavelengths,
            data: spectra,
        };
        ensure!(
            p.settings.spectral_gaussian_blur.is_finite()
                && p.settings.spectral_gaussian_blur >= 0.0,
            "Invalid spectral smoothing width"
        );
        if method == "hanatos2025" && p.settings.spectral_gaussian_blur > 0.0 {
            spectra
                .data
                .par_chunks_exact_mut(wavelengths)
                .for_each(|spectrum| {
                    let smoothed =
                        crate::base::gaussian(spectrum, p.settings.spectral_gaussian_blur);
                    spectrum.copy_from_slice(&smoothed);
                });
        }
        let reference = t.illuminant(&film.info.reference_illuminant)?;
        let scene_name = descriptor
            .get("reflectance")
            .and_then(|v| v.get("scene_illuminant"))
            .and_then(toml::Value::as_str)
            .unwrap_or("D65");
        let projection_white = if method == "hanatos2025" {
            reference.xy
        } else {
            t.illuminant(scene_name)?.xy
        };
        let mut sensitivity = film.sensitivity.clone();
        if let Some(filter) = t
            .camera_filters
            .get(&p.camera.color_filter)
            .with_context(|| format!("Unknown camera filter {}", p.camera.color_filter))?
        {
            for (w, row) in sensitivity.iter_mut().enumerate() {
                for v in row {
                    *v *= filter[w];
                }
            }
        }
        if method == "hanatos2025"
            && p.settings.apply_hanatos2025_adaptation_window
            && let Some(window) = &film.window
        {
            ensure!(window.len() == 4, "Unsupported adaptation window");
            let weights = film
                .wavelengths
                .iter()
                .map(|wl| {
                    0.5 * (1.0
                        + libm::erf((wl - window[0]) / (window[1] * std::f64::consts::SQRT_2)))
                        * 0.5
                        * (1.0
                            - libm::erf((wl - window[2]) / (window[3] * std::f64::consts::SQRT_2)))
                })
                .collect::<Vec<_>>();
            let neutral = spectra.cubic(tri_to_quad(reference.xy));
            for c in 0..channels {
                let original: f64 = (0..81).map(|w| sensitivity[w][c] * neutral[w]).sum();
                let filtered: f64 = (0..81)
                    .map(|w| sensitivity[w][c] * neutral[w] * weights[w])
                    .sum();
                for w in 0..81 {
                    sensitivity[w][c] *= weights[w] / (filtered / original);
                }
            }
        }
        let mut normalization = vec![1.0; channels];
        if method != "hanatos2025" {
            let neutral = spectra.cubic(tri_to_quad(projection_white));
            for c in 0..channels {
                normalization[c] = (0..81)
                    .map(|w| neutral[w] * reference.spectrum[w] * sensitivity[w][c])
                    .sum();
            }
            for (w, row) in sensitivity.iter_mut().enumerate() {
                for v in row {
                    *v *= reference.spectrum[w];
                }
            }
        }
        let mut raw = Lut2 {
            size,
            channels,
            data: vec![0.0; size * size * channels],
        };
        raw.data
            .par_chunks_exact_mut(channels)
            .enumerate()
            .for_each(|(i, pixel)| {
                for c in 0..channels {
                    let mut sum = 0.0;
                    for w in 0..81 {
                        sum += spectra.data[i * wavelengths + w] * sensitivity[w][c];
                    }
                    pixel[c] = sum / normalization[c];
                }
            });
        if method == "hanatos2025"
            && p.settings.apply_hanatos2025_adaptation_surface
            && let Some(surface) = &film.surface
        {
            ensure!(
                surface.len() >= channels && surface.iter().all(|row| row.len() == 15),
                "Unsupported adaptation surface coefficients"
            );
            let center = tri_to_quad(reference.xy);
            raw.data
                .par_chunks_exact_mut(channels)
                .enumerate()
                .for_each(|(i, pixel)| {
                    let x = (i / size) as f64 / (size - 1) as f64 - center[0];
                    let y = (i % size) as f64 / (size - 1) as f64 - center[1];
                    let x2 = x * x;
                    let y2 = y * y;
                    let x3 = x2 * x;
                    let y3 = y2 * y;
                    for c in 0..channels {
                        let p = &surface[c];
                        // The constant coefficient is intentionally omitted so
                        // reference-illuminant chromaticity keeps white balance.
                        let correction = p[1] * x
                            + p[2] * y
                            + p[3] * x2
                            + p[4] * y2
                            + p[5] * x * y
                            + p[6] * x3
                            + p[7] * y3
                            + p[8] * x2 * y
                            + p[9] * x * y2
                            + p[10] * x2 * x2
                            + p[11] * y2 * y2
                            + p[12] * x3 * y
                            + p[13] * x2 * y2
                            + p[14] * x * y3;
                        pixel[c] *=
                            2.0f64.powf(correction / (1.0 + (correction / 2.0).powi(2)).sqrt());
                    }
                });
        }
        if p.io.input_gamut_compress.active {
            let custom_hull;
            let polygon = match p.io.input_gamut_compress.boundary.as_str() {
                "inscribed_hull" => {
                    if p.io.input_gamut_compress.hull_detail == 5.0 {
                        &t.input_hull
                    } else {
                        custom_hull = inscribed_hull(
                            t.illuminant("D65")?.xy,
                            p.io.input_gamut_compress.hull_detail,
                            &t.locus,
                        )?;
                        &custom_hull
                    }
                }
                "locus" => &t.locus,
                _ => anyhow::bail!("Unknown input gamut boundary"),
            };
            let center = t.illuminant("D65")?.xy;
            let mut remap = vec![0.0; raw.data.len()];
            remap
                .par_chunks_exact_mut(channels)
                .enumerate()
                .for_each(|(i, out)| {
                    let x = (i / size) as f64 / (size - 1) as f64;
                    let y = (i % size) as f64 / (size - 1) as f64;
                    let xy = [1.0 - x.sqrt(), y * x.sqrt()];
                    let xy = compress_xy(xy, center, polygon, p.io.input_gamut_compress.knee);
                    raw.bilinear_into(tri_to_quad(xy), out);
                });
            raw.data = remap;
        }
        Ok(Self {
            lut: raw,
            projection_white,
        })
    }
}

fn compress_xy(xy: [f64; 2], center: [f64; 2], polygon: &[[f64; 2]], params: [f64; 3]) -> [f64; 2] {
    let delta = [xy[0] - center[0], xy[1] - center[1]];
    let distance = delta[0].hypot(delta[1]);
    if distance < 1e-9 {
        return xy;
    }
    let direction = [delta[0] / distance, delta[1] / distance];
    let boundary = ray_distance(center, direction, polygon);
    let d = knee(distance / boundary.max(1e-12), params) * boundary;
    [center[0] + direction[0] * d, center[1] + direction[1] * d]
}
fn ray_distance(center: [f64; 2], direction: [f64; 2], polygon: &[[f64; 2]]) -> f64 {
    let mut boundary = f64::INFINITY;
    for pair in polygon.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let ex = b[0] - a[0];
        let ey = b[1] - a[1];
        let denom = direction[0] * ey - direction[1] * ex;
        if denom.abs() <= 1e-12 {
            continue;
        }
        let ox = center[0] - a[0];
        let oy = center[1] - a[1];
        let t = (-ox * ey + oy * ex) / denom;
        let s = (-ox * direction[1] + oy * direction[0]) / denom;
        if t > 1e-9 && (-1e-9..=1.0 + 1e-9).contains(&s) {
            boundary = boundary.min(t);
        }
    }
    boundary
}
/// Build the reference's periodic Gaussian low-pass hull in double precision.
/// Only nondefault detail values need this setup-time Fourier calculation.
fn inscribed_hull(center: [f64; 2], detail: f64, locus: &[[f64; 2]]) -> Result<Vec<[f64; 2]>> {
    ensure!(
        detail.is_finite() && detail > 0.0,
        "Hull detail must be positive and finite"
    );
    let n = 1024;
    let tau = std::f64::consts::TAU;
    let dirs: Vec<_> = (0..n)
        .map(|i| {
            let a = tau * i as f64 / n as f64;
            [a.cos(), a.sin()]
        })
        .collect();
    let reach: Vec<_> = dirs
        .iter()
        .map(|d| ray_distance(center, *d, locus))
        .collect();
    ensure!(
        reach.iter().all(|v| v.is_finite()),
        "Locus does not enclose the reference white"
    );
    let modes: Vec<_> = (0..=n / 2)
        .into_par_iter()
        .map(|k| {
            let attenuation = (-0.5 * (k as f64 / detail).powi(2)).exp();
            if attenuation < 1e-30 {
                return [0.0; 2];
            }
            let mut value = [0.0; 2];
            for (j, r) in reach.iter().enumerate() {
                let a = tau * (j * k) as f64 / n as f64;
                value[0] += r * a.cos();
                value[1] -= r * a.sin();
            }
            value.map(|v| v * attenuation / n as f64)
        })
        .collect();
    let smooth: Vec<_> = (0..n)
        .into_par_iter()
        .map(|i| {
            let mut value = modes[0][0];
            for (k, m) in modes.iter().enumerate().skip(1) {
                let a = tau * (i * k) as f64 / n as f64;
                let weight = if k == n / 2 { 1.0 } else { 2.0 };
                value += weight * (m[0] * a.cos() - m[1] * a.sin());
            }
            value.max(1e-12)
        })
        .collect();
    let scale = reach
        .iter()
        .zip(&smooth)
        .map(|(a, b)| a / b)
        .fold(f64::INFINITY, f64::min);
    let mut hull: Vec<_> = dirs
        .iter()
        .zip(smooth)
        .map(|(d, r)| [center[0] + scale * r * d[0], center[1] + scale * r * d[1]])
        .collect();
    hull.push(hull[0]);
    Ok(hull)
}

pub fn integrate(
    profile: &Profile,
    density: &[f64],
    weights: &[[f64; 3]],
    channels: usize,
    out: &mut [f64],
) {
    out.fill(0.0);
    for (w, weight) in weights.iter().enumerate() {
        let transmission = (-std::f64::consts::LN_10 * profile.spectral_density(density, w)).exp();
        if transmission.is_nan() {
            continue;
        }
        for c in 0..channels {
            out[c] += transmission * weight[c];
        }
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    #[test]
    fn response_cache_uses_effective_spectral_settings() -> Result<()> {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let tables = Tables::load(&data)?;
        let mut p = Params::default();
        let mut film = Profile::load(&data, &p.film_profile)?;
        let original = FilmResponse::shared(&data, &tables, &film, &p)?;
        p.camera.exposure_compensation_ev = 0.5;
        p.settings.random_seed = Some(19);
        p.workflow.saving_color_space = Some("Display P3".into());
        assert!(std::sync::Arc::ptr_eq(
            &original,
            &FilmResponse::shared(&data, &tables, &film, &p)?
        ));
        for which in 0..7 {
            let mut p = p.clone();
            match which {
                0 => p.camera.color_filter = "UV".into(),
                1 => p.settings.apply_hanatos2025_adaptation_window = false,
                2 => p.settings.apply_hanatos2025_adaptation_surface = true,
                3 => p.settings.spectral_gaussian_blur = 0.5,
                4 => p.io.input_gamut_compress.active = false,
                5 => p.io.input_gamut_compress.hull_detail = 6.,
                _ => film.sensitivity[40][0] *= 1.01,
            }
            // Select an actual copied filter; no handmade measurement data.
            if which == 0 {
                p.camera.color_filter = tables
                    .camera_filters
                    .keys()
                    .find(|k| k.as_str() != "none")
                    .unwrap()
                    .clone();
            }
            let shared = FilmResponse::shared(&data, &tables, &film, &p)?;
            let fresh = FilmResponse::new(&data, &tables, &film, &p)?;
            assert_eq!(shared.lut.data, fresh.lut.data);
            assert_eq!(shared.projection_white, fresh.projection_white);
            assert!(!std::sync::Arc::ptr_eq(&original, &shared));
        }
        Ok(())
    }
}
