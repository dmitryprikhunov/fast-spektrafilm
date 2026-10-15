//! Streaming sublayer grain, with the reference's float32 working precision.
use crate::{
    Image,
    interpolation::linear,
    params::Grain,
    profile::Profile,
    random::RandomState,
    spatial::{gaussian, gaussian_f32_data},
};
use anyhow::{Result, ensure};

// NumPy's float32 reduction uses an eight-lane pairwise tree, including on
// flattened strided channel views. A sequential sum moves the USM mass scale.
fn pairwise(values: &[f32], channel: usize, stride: usize, start: usize, n: usize) -> f32 {
    let value = |i: usize| values[(start + i) * stride + channel];
    if n < 8 {
        let mut sum = -0.0f32;
        for i in 0..n {
            sum += value(i);
        }
        return sum;
    }
    if n <= 128 {
        let mut lanes = std::array::from_fn::<_, 8, _>(value);
        let end = n - n % 8;
        for i in (8..end).step_by(8) {
            for (k, lane) in lanes.iter_mut().enumerate() {
                *lane += value(i + k);
            }
        }
        let mut sum = ((lanes[0] + lanes[1]) + (lanes[2] + lanes[3]))
            + ((lanes[4] + lanes[5]) + (lanes[6] + lanes[7]));
        for i in end..n {
            sum += value(i);
        }
        sum
    } else {
        let middle = n / 2 - n / 2 % 8;
        pairwise(values, channel, stride, start, middle)
            + pairwise(values, channel, stride, start + middle, n - middle)
    }
}
pub fn apply(
    density: &Image,
    film: &Profile,
    params: &Grain,
    pixel_um: f64,
    fast: bool,
    seed: u32,
) -> Result<Image> {
    if !params.active {
        return Ok(density.clone());
    }
    let ch = density.channels;
    let n = density.width * density.height;
    let mut out = vec![0.0f32; n * ch];
    let mut numpy_state = RandomState::new(0);
    for c in 0..ch {
        let single = vec![film.curves[c].clone()];
        let layers = if film.layers[c].len() == 1 {
            &single
        } else {
            &film.layers[c]
        };
        let count = layers.len();
        ensure!(
            params.particle_scale_sublayers.len() >= count,
            "Missing grain sublayer scales"
        );
        for values in [
            &params.density_min,
            &params.rms_granularity,
            &params.uniformity,
        ] {
            ensure!(
                values.len() == ch || (ch == 1 && !values.is_empty()),
                "Grain parameter channel count mismatch"
            );
        }
        let density_min = params.density_min[c];
        let uniformity = params.uniformity[c];
        let maxima = layers
            .iter()
            .map(|l| l.iter().copied().fold(f64::NEG_INFINITY, f64::max))
            .collect::<Vec<_>>();
        let total = maxima.iter().sum::<f64>();
        let fractions = maxima.iter().map(|v| v / total).collect::<Vec<_>>();
        let floors = fractions
            .iter()
            .map(|v| v * density_min)
            .collect::<Vec<_>>();
        let absolute_max = maxima
            .iter()
            .zip(&floors)
            .map(|(v, f)| v + f)
            .collect::<Vec<_>>();
        let mut peak: f64 = 0.0;
        for (x, _) in film.axis.iter().enumerate() {
            let mut value = 0.0;
            for l in 0..count {
                let d = layers[l][x] + floors[l];
                value += params.particle_scale_sublayers[l] / fractions[l]
                    * d
                    * (absolute_max[l] - uniformity * d);
            }
            peak = peak.max(value);
        }
        let area =
            (params.rms_granularity[c] / 1000.0).powi(2) * std::f64::consts::PI * 24.0 * 24.0
                / peak.max(1e-9);
        let positive = film.info.polarity == "positive";
        let axis = film.curves[c]
            .iter()
            .map(|v| if positive { -v } else { *v })
            .collect::<Vec<_>>();
        let mut channel = vec![0.0f32; n];
        let mut probabilities = vec![0.0f32; n];
        let mut saturation = vec![0.0f32; n];
        let mut seeds = vec![0i64; n];
        for l in 0..count {
            let n_particles =
                pixel_um * pixel_um * fractions[l] / (area * params.particle_scale_sublayers[l]);
            ensure!(
                n_particles.is_finite() && n_particles > 0.0,
                "Grain particle count must be finite and positive"
            );
            let maximum = absolute_max[l];
            let od = maximum / n_particles;
            numpy_state = RandomState::new((c + l * 10) as u32);
            let mut fast_state = RandomState::new(seed.wrapping_add((c + l * 10) as u32));
            for i in 0..n {
                let d = density.data[i * ch + c];
                let layer = linear(if positive { -d } else { d }, &axis, &layers[l]) as f32;
                // Adding the f64 density-floor array stores back into float32.
                let d = (layer as f64 + floors[l]) as f32;
                let p = (d / maximum as f32).clamp(1e-6, 1.0 - 1e-6);
                let sat = 1.0f32 - (p * uniformity as f32) * (1.0f32 - 1e-6);
                let lambda = (n_particles as f32 / sat) as f64;
                let draw = if fast {
                    // The reference fast sampler stores signed int32 counts.
                    // Preserve its wrap on extremely coarse grids as well.
                    fast_state.fast_poisson(lambda) as i32 as i64
                } else {
                    numpy_state.poisson(lambda)
                };
                probabilities[i] = p;
                saturation[i] = sat;
                seeds[i] = draw;
            }
            for i in 0..n {
                let draw = if fast {
                    fast_state.fast_binomial(seeds[i], probabilities[i] as f64)
                } else {
                    numpy_state.binomial(seeds[i], probabilities[i] as f64)
                };
                let d = (draw as f32 * od as f32) * saturation[i];
                probabilities[i] = d;
            }
            // Reuse the probability plane as the completed float32 layer.
            // The last layer no longer needs count/saturation scratch.
            if l + 1 == count {
                saturation = Vec::new();
                seeds = Vec::new();
            }
            if params.blur_dye_clouds_um > 0.0 {
                probabilities = gaussian_f32_data(
                    &probabilities,
                    density.width,
                    density.height,
                    1,
                    &[params.blur_dye_clouds_um * od.sqrt()],
                );
            }
            for (v, d) in channel.iter_mut().zip(&probabilities) {
                *v += *d;
            }
        }
        for (i, v) in channel.iter().enumerate() {
            out[i * ch + c] = *v;
        }
    }
    let micro_sigma = params.micro_structure[1] * 0.001 / pixel_um;
    if micro_sigma > 0.05 {
        let sigma2 = (micro_sigma * micro_sigma).ln_1p();
        let mut clumps = Image {
            width: density.width,
            height: density.height,
            channels: ch,
            data: (0..n * ch)
                .map(|_| (numpy_state.normal() * sigma2.sqrt() - 0.5 * sigma2).exp())
                .collect(),
        };
        if params.micro_structure[0] / pixel_um > 0.4 {
            clumps = gaussian(&clumps, &[params.micro_structure[0] / pixel_um]);
        }
        for (v, c) in out.iter_mut().zip(clumps.data) {
            *v = (*v as f64 * c) as f32;
        }
    }
    if params.blur > 0.0 {
        out = gaussian_f32_data(&out, density.width, density.height, ch, &[params.blur]);
    }
    if params.mult_usm_sigma > 0.0 && params.mult_usm_amount > 0.0 {
        let positive: Vec<_> = out.iter().map(|v| v.max(0.)).collect();
        let blurred = gaussian_f32_data(
            &positive,
            density.width,
            density.height,
            ch,
            &[params.mult_usm_sigma],
        );
        for (i, v) in out.iter_mut().enumerate() {
            let d = positive[i];
            let b = blurred[i].max(1e-6);
            *v = if d <= 0.0 {
                0.0
            } else {
                (d.max(1e-6).powf((1.0 + params.mult_usm_amount) as f32)
                    / b.powf(params.mult_usm_amount as f32))
            };
        }
        for c in 0..ch {
            // NumPy's float32 strided reduction accumulates in float32.
            let sum = pairwise(&out, c, ch, 0, n);
            let original = pairwise(&positive, c, ch, 0, n);
            if sum > 0.0 {
                let scale = original / sum;
                for v in out.iter_mut().skip(c).step_by(ch) {
                    *v *= scale;
                }
            }
        }
    }
    let data = out
        .into_iter()
        .enumerate()
        .map(|(i, v)| (v as f64 - params.density_min[i % ch]) as f32 as f64)
        .collect();
    Image::new(density.width, density.height, ch, data)
}

pub fn apply_presets(data: &std::path::Path, film: &Profile, grain: &mut Grain) -> Result<()> {
    let db: toml::Value =
        toml::from_str(&std::fs::read_to_string(data.join("presets/grain.toml"))?)?;
    let Some(preset) = db.get(&film.info.stock) else {
        return Ok(());
    };
    let model = if film.channels() == 1 { "bw" } else { "color" };
    let mut value = serde_json::to_value(&*grain)?;
    if let Some(defaults) = db
        .get("defaults")
        .and_then(|v| v.get(model))
        .and_then(|v| v.get(&film.info.polarity))
        .and_then(toml::Value::as_table)
    {
        for (k, v) in defaults {
            value[k] = serde_json::to_value(preset.get(k).unwrap_or(v))?;
        }
    }
    if let Some(preset) = preset.as_table() {
        for (k, v) in preset {
            if value.get(k).is_some() {
                value[k] = serde_json::to_value(v)?;
            }
        }
    }
    *grain = serde_json::from_value(value)?;
    Ok(())
}
