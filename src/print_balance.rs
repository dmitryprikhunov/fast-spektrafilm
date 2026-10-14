//! Rebalance the printer after film/base changes using the pinned midgray solve.
use crate::{Image, Params, Simulator, Tap};
use anyhow::{Result, ensure};
use std::path::Path;
pub fn solve_neutral_filter_shifts(data: &Path, params: &Params, midgray: f64) -> Result<[f64; 2]> {
    ensure!(
        midgray.is_finite() && midgray > 0.,
        "Midgray must be positive and finite"
    );
    let mut p = params.clone();
    p.workflow.route = "input > film > print > scan".into();
    p.settings.use_enlarger_lut = false;
    p.settings.use_scanner_lut = false;
    p.io.output_cctf_encoding = false;
    p.io.output_gamut_compress.algorithm = "off".into();
    p.io.output_gamut_compress.lightness_compression = None;
    let gray = Image::new(2, 2, 3, vec![midgray; 12])?;
    Simulator::new(data, p.clone())?;
    let x = crate::optimization::least_squares(
        vec![p.enlarger.m_filter_shift, p.enlarger.y_filter_shift],
        |x| {
            let mut trial = p.clone();
            trial.enlarger.m_filter_shift = x[0];
            trial.enlarger.y_filter_shift = x[1];
            let out = Simulator::new(data, trial).and_then(|sim| sim.process(&gray, Tap::RgbOut));
            match out {
                Ok(out) => {
                    let rgb: [f64; 3] = std::array::from_fn(|c| {
                        out.data.chunks_exact(3).map(|v| v[c]).sum::<f64>() / 4.
                    });
                    let g = rgb[1].max(1e-6);
                    vec![(rgb[0] - rgb[1]) / g, (rgb[2] - rgb[1]) / g]
                }
                Err(_) => vec![f64::NAN; 2],
            }
        },
        40,
    )?;
    Ok([x[0], x[1]])
}
