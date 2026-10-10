//! The experimental coupled gamma / developer exhaustion model.
use crate::{params::Chemistry, profile::Profile};
use anyhow::{Result, ensure};
fn cdf(z: f64, model: &str, alpha: f64) -> f64 {
    if model == "norm_cdfs" {
        0.5 * libm::erfc(-z / std::f64::consts::SQRT_2)
    } else {
        let u = (z / 5.8013 + 0.5).clamp(0.0, 1.0);
        let v = (u + alpha * u * (1.0 - u) * (2.0 * u - 1.0).powi(2)).clamp(0.0, 1.0);
        v.powi(4) * (35.0 + v * (-84.0 + v * (70.0 - 20.0 * v)))
    }
}
fn mixed(z: f64, model: &str, alpha: f64, exhaustion: f64) -> f64 {
    let original = cdf(z, model, alpha);
    if exhaustion > 0.0 {
        let location = -(2.0f64.ln()).ln();
        let width = 0.5 * 2.0f64.ln() * (2.0 * std::f64::consts::PI).sqrt();
        (1.0 - exhaustion) * original + exhaustion * (-(-(z / width + location)).exp()).exp()
    } else {
        original
    }
}
pub fn apply(profile: &mut Profile, p: &Chemistry) -> Result<()> {
    if !p.active {
        return Ok(());
    }
    let factors = [
        p.gamma_factor,
        p.gamma_factor_fast,
        p.gamma_factor_slow,
        p.gamma_factor_red,
        p.gamma_factor_green,
        p.gamma_factor_blue,
    ];
    ensure!(
        factors.iter().all(|v| v.is_finite() && *v > 0.0),
        "Chemistry gamma must be positive and finite"
    );
    ensure!(
        (0.0..=1.0).contains(&p.developer_exhaustion),
        "Developer exhaustion must be in [0,1]"
    );
    if factors.iter().all(|v| *v == 1.0) && p.developer_exhaustion == 0.0 {
        return Ok(());
    }
    let model = profile
        .model
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Chemistry needs a density curve model"))?;
    let sign = if profile.info.polarity == "positive" {
        -1.0
    } else {
        1.0
    };
    for c in 0..profile.channels() {
        let mut centers = model.centers[c].clone();
        let mut sigmas = model.sigmas[c].clone();
        let count = centers.len();
        let mut order = (0..count).collect::<Vec<_>>();
        order.sort_by(|a, b| centers[*a].total_cmp(&centers[*b]));
        let gamma = p.gamma_factor
            * [
                p.gamma_factor_red,
                p.gamma_factor_green,
                p.gamma_factor_blue,
            ][c];
        // Sequential updates preserve the reference's repeated-index behavior
        // for models with one or two sublayers.
        for (layer, g) in [
            (order[0], gamma * p.gamma_factor_fast),
            (order[count / 2], gamma * p.gamma_factor_slow),
            (order[count - 1], gamma * p.gamma_factor_slow),
        ] {
            sigmas[layer] = (sigmas[layer] / g).max(0.05);
            centers[layer] /= g;
        }
        let alpha = |l: usize| model.alphas.as_ref().map_or(0.0, |a| a[c][l]);
        let density_at = |x: f64, offset: f64, exhaustion: f64| -> f64 {
            (0..count)
                .map(|l| {
                    model.amplitudes[c][l]
                        * mixed(
                            sign * (x - centers[l] - offset) / sigmas[l],
                            &model.model_type,
                            alpha(l),
                            exhaustion,
                        )
                })
                .sum()
        };
        let mut offset = 0.0;
        if p.developer_exhaustion > 1e-8 {
            let target = density_at(0.0, 0.0, 0.0);
            let residual = |o: f64| density_at(0.0, o, p.developer_exhaustion) - target;
            if residual(0.0).abs() > 1e-12 {
                let mut lo = -0.25;
                let mut hi = 0.25;
                for _ in 0..12 {
                    let flo = residual(lo);
                    let fhi = residual(hi);
                    if flo == 0.0 {
                        offset = lo;
                        break;
                    }
                    if fhi == 0.0 {
                        offset = hi;
                        break;
                    }
                    if flo * fhi < 0.0 {
                        for _ in 0..60 {
                            let middle = 0.5 * (lo + hi);
                            let f = residual(middle);
                            if f * residual(lo) > 0.0 {
                                lo = middle;
                            } else {
                                hi = middle;
                            }
                            if (hi - lo).abs() < 1e-13 {
                                break;
                            }
                        }
                        offset = 0.5 * (lo + hi);
                        break;
                    }
                    lo *= 2.0;
                    hi *= 2.0;
                }
            }
        }
        for (i, x) in profile.axis.iter().enumerate() {
            profile.curves[c][i] = density_at(*x, offset, p.developer_exhaustion);
            for l in 0..count {
                profile.layers[c][l][i] = model.amplitudes[c][l]
                    * mixed(
                        sign * (x - centers[l] - offset) / sigmas[l],
                        &model.model_type,
                        alpha(l),
                        p.developer_exhaustion,
                    );
            }
        }
    }
    Ok(())
}
