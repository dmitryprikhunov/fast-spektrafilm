use crate::interpolation::linear;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::path::Path;

#[derive(Clone, Deserialize)]
pub struct Info {
    pub stock: String,
    pub name: String,
    #[serde(rename = "type")]
    pub polarity: String,
    pub channel_model: String,
    pub reference_illuminant: String,
    pub viewing_illuminant: String,
    pub support: String,
    pub stage: String,
    #[serde(rename = "use")]
    pub usage: String,
    pub antihalation: String,
}
#[derive(Clone, Deserialize)]
pub struct Model {
    pub model_type: String,
    pub centers: Vec<Vec<f64>>,
    pub amplitudes: Vec<Vec<f64>>,
    pub sigmas: Vec<Vec<f64>>,
    pub alphas: Option<Vec<Vec<f64>>>,
}
#[derive(Clone, Deserialize)]
struct Data {
    wavelengths: Vec<f64>,
    log_sensitivity: Vec<Vec<Option<f64>>>,
    channel_density: Vec<Vec<Option<f64>>>,
    base_density: serde_json::Value,
    log_exposure: Vec<f64>,
    density_curves: Vec<Vec<f64>>,
    density_curves_model: Option<Model>,
    development_time: Option<Vec<f64>>,
    hanatos2025_adaptation_window_params: Option<Vec<f64>>,
    hanatos2025_adaptation_surface_params: Option<Vec<Vec<f64>>>,
}
#[derive(Clone)]
pub struct Profile {
    pub info: Info,
    pub wavelengths: Vec<f64>,
    pub sensitivity: Vec<Vec<f64>>,
    pub dyes: Vec<Vec<f64>>,
    pub base: Vec<f64>,
    pub axis: Vec<f64>,
    pub curves: Vec<Vec<f64>>,
    /// Unnormalised model sublayers, indexed channel / sublayer / exposure.
    pub layers: Vec<Vec<Vec<f64>>>,
    pub model: Option<Model>,
    pub window: Option<Vec<f64>>,
    pub surface: Option<Vec<Vec<f64>>>,
}
impl Profile {
    /// The experimental GUI permutes dye spectra only, leaving sensitivity and
    /// development layers intact. This is not an output RGB channel swap.
    pub fn swap_dyes(&mut self, order: [usize; 3]) -> Result<()> {
        let mut sorted = order;
        sorted.sort();
        ensure!(
            sorted == [0, 1, 2],
            "Dye order must contain cyan, magenta and yellow once each"
        );
        if order == [0, 1, 2] {
            return Ok(());
        }
        ensure!(
            self.channels() == 3,
            "Dye swapping requires a three-channel stock"
        );
        for row in &mut self.dyes {
            let old = row.clone();
            for c in 0..3 {
                row[c] = old[order[c]];
            }
        }
        Ok(())
    }

    pub fn load(data_dir: &Path, stock: &str) -> Result<Self> {
        Self::load_development(data_dir, stock, None)
    }
    pub fn load_development(
        data_dir: &Path,
        stock: &str,
        development: Option<f64>,
    ) -> Result<Self> {
        ensure!(
            !stock.contains('/') && !stock.contains('\\'),
            "Invalid profile name"
        );
        use std::sync::{Arc, Mutex, OnceLock};
        static CACHE: OnceLock<Mutex<crate::cache::ByteCache<Profile>>> = OnceLock::new();
        let path = data_dir.join("profiles").join(format!("{stock}.json"));
        let key = format!(
            "{}:{:?}",
            crate::cache::file_key(&path)?,
            development.map(f64::to_bits)
        );
        let cache =
            CACHE.get_or_init(|| Mutex::new(crate::cache::ByteCache::new(16 * 1024 * 1024)));
        if let Some(profile) = cache.lock().unwrap().get(&key) {
            return Ok((*profile).clone());
        }
        let profile = Self::load_uncached(data_dir, stock, development)?;
        let bytes = path.metadata()?.len() as usize * 8;
        cache
            .lock()
            .unwrap()
            .insert(key, bytes, Arc::new(profile.clone()));
        Ok(profile)
    }
    fn load_uncached(data_dir: &Path, stock: &str, development: Option<f64>) -> Result<Self> {
        #[derive(Deserialize)]
        struct Payload {
            info: Info,
            data: Data,
        }
        let payload: Payload = serde_json::from_reader(std::fs::File::open(
            data_dir.join("profiles").join(format!("{stock}.json")),
        )?)?;
        let Payload { info, mut data } = payload;
        ensure!(
            data.wavelengths.len() == 81
                && data
                    .wavelengths
                    .iter()
                    .enumerate()
                    .all(|(i, v)| *v == 380.0 + 5.0 * i as f64),
            "Unsupported profile wavelength grid"
        );
        let select = data
            .development_time
            .as_ref()
            .filter(|t| !t.is_empty())
            .map(|t| {
                development.map_or((t.len() - 1) / 2, |time| {
                    t.iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| (*a - time).abs().total_cmp(&(*b - time).abs()))
                        .unwrap()
                        .0
                })
            });
        let base = if let Some(index) = select {
            data.density_curves = data.density_curves.iter().map(|r| vec![r[index]]).collect();
            if let Some(m) = data.density_curves_model.as_mut() {
                m.centers = vec![m.centers[index].clone()];
                m.amplitudes = vec![m.amplitudes[index].clone()];
                m.sigmas = vec![m.sigmas[index].clone()];
                m.alphas = m.alphas.as_ref().map(|v| vec![v[index].clone()]);
            }
            data.base_density
                .as_array()
                .context("Missing base")?
                .iter()
                .map(|v| {
                    v.as_array()
                        .map_or_else(|| v.as_f64(), |r| r[index].as_f64())
                        .or(Some(f64::NAN))
                        .context("Invalid base density")
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            serde_json::from_value::<Vec<Option<f64>>>(data.base_density)?
                .into_iter()
                .map(|v| v.unwrap_or(f64::NAN))
                .collect()
        };
        let channels = if info.channel_model == "bw" { 1 } else { 3 };
        let mut curves = vec![vec![0.0; data.log_exposure.len()]; channels];
        let mut layers = vec![Vec::new(); channels];
        if let Some(m) = &data.density_curves_model {
            ensure!(
                ["norm_cdfs", "sept_norm_cdfs"].contains(&m.model_type.as_str()),
                "Unknown density curve model"
            );
            for (c, curve) in curves.iter_mut().enumerate() {
                layers[c] = vec![vec![0.0; data.log_exposure.len()]; m.centers[c].len()];
                for (i, x) in data.log_exposure.iter().enumerate() {
                    for l in 0..m.centers[c].len() {
                        let mut z = (x - m.centers[c][l]) / m.sigmas[c][l];
                        if info.polarity == "positive" {
                            z = -z;
                        }
                        let cdf = if m.model_type == "norm_cdfs" {
                            0.5 * libm::erfc(-z / std::f64::consts::SQRT_2)
                        } else {
                            let u = (z / 5.8013 + 0.5).clamp(0.0, 1.0);
                            let alpha = m.alphas.as_ref().map_or(0.0, |a| a[c][l]);
                            let v = (u + alpha * u * (1.0 - u) * (2.0 * u - 1.0).powi(2))
                                .clamp(0.0, 1.0);
                            v.powi(4) * (35.0 + v * (-84.0 + v * (70.0 - 20.0 * v)))
                        };
                        let value = m.amplitudes[c][l] * cdf;
                        layers[c][l][i] = value;
                        curve[i] += value;
                    }
                }
            }
        } else {
            for c in 0..channels {
                for (i, row) in data.density_curves.iter().enumerate() {
                    curves[c][i] = row[c];
                }
            }
            for c in 0..channels {
                layers[c] = vec![curves[c].clone()];
            }
        }
        ensure!(
            data.log_exposure.windows(2).all(|w| w[0] < w[1]),
            "Non-monotone exposure grid"
        );
        ensure!(
            data.channel_density.len() == 81
                && base.len() == 81
                && data.log_sensitivity.len() == 81,
            "Invalid spectral shapes"
        );
        let sensitivity = data
            .log_sensitivity
            .iter()
            .map(|r| {
                r.iter()
                    .map(|v| v.map_or(0.0, |v| 10.0_f64.powf(v)))
                    .collect()
            })
            .collect();
        Ok(Self {
            info,
            wavelengths: data.wavelengths,
            sensitivity,
            dyes: data
                .channel_density
                .into_iter()
                .map(|r| r.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect())
                .collect(),
            base,
            axis: data.log_exposure,
            curves,
            layers,
            model: data.density_curves_model,
            window: data.hanatos2025_adaptation_window_params,
            surface: data.hanatos2025_adaptation_surface_params,
        })
    }
    pub fn channels(&self) -> usize {
        self.curves.len()
    }
    pub fn density(&self, log_e: &[f64], out: &mut [f64]) {
        for c in 0..self.channels() {
            out[c] = linear(log_e[c], &self.axis, &self.curves[c]);
        }
    }
    pub fn min(&self) -> Vec<f64> {
        self.curves
            .iter()
            .map(|v| v.iter().copied().fold(f64::INFINITY, f64::min))
            .collect()
    }
    pub fn max(&self) -> Vec<f64> {
        self.curves
            .iter()
            .map(|v| v.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            .collect()
    }
    pub fn spectral_density(&self, density: &[f64], w: usize) -> f64 {
        let mut value = 0.0;
        for (d, k) in density.iter().zip(&self.dyes[w]) {
            value += d * k;
        }
        value + self.base[w]
    }
}
