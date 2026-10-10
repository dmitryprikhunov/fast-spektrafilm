//! Reference luminance correction and its exposure anchors.
use crate::{
    color::{Tables, decode, mat_vec},
    interpolation::linear,
    params::Params,
    profile::Profile,
    spectral::integrate,
};
use anyhow::{Result, ensure};

pub(crate) struct ScanReference {
    pub film_gain: f64,
    pub print_gain: f64,
    affine: Option<[f64; 2]>,
}
impl ScanReference {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tables: &Tables,
        params: &Params,
        film: &Profile,
        print: &Profile,
        film_base: &[f64],
        print_base: &[f64],
        print_weights: &[[f64; 3]],
        scan_weights: &[[f64; 3]],
        print_factor: f64,
        preflash: [f64; 3],
    ) -> Result<Self> {
        let mut result = Self {
            film_gain: 1.0,
            print_gain: 1.0,
            affine: None,
        };
        let s = &params.scanner;
        let film_scan = params.workflow.route == "input > film > scan";
        if !(s.black_correction || s.white_correction)
            || params.workflow.route == "input"
            || (film_scan && film.info.polarity == "negative")
        {
            return Ok(result);
        }
        ensure!(
            film_scan || print.info.polarity == "negative",
            "Black/white correction requires reversal film or a negative print profile"
        );
        // The colour service probes stock curves; print chemistry is applied
        // only when developing the actual print, after these references.
        let scanned = if film_scan { film } else { print };
        let (black, white) = if film_scan {
            (film.max(), vec![0.0; film.channels()])
        } else {
            let maximum = film.max();
            let minimum: Vec<_> = (0..film.channels())
                .map(|c| -params.grain.density_min[c])
                .collect();
            let expose = |d: &[f64]| {
                let mut raw = [0.0; 3];
                integrate(film, d, print_weights, print.channels(), &mut raw);
                // Keep per-channel preflash and the original positive floor.
                let logs: Vec<_> = (0..print.channels())
                    .map(|c| ((raw[c] * print_factor + preflash[c]).max(0.0) + 1e-10).log10())
                    .collect();
                let mut out = vec![0.0; print.channels()];
                print.density(&logs, &mut out);
                out
            };
            // Printing black/white probes use the original, unnormalized film
            // curves, even though development later removes their minimum.
            let black = expose(&minimum);
            let white = expose(&maximum);
            (black, white)
        };
        let luminance = |d: &[f64]| {
            let mut xyz = [0.0; 3];
            integrate(scanned, d, scan_weights, 3, &mut xyz);
            10.0f64.powf((xyz[1].max(0.0) + 1e-10).log10())
        };
        let y_black = luminance(&black);
        let y_white = luminance(&white);
        let identity = tables.rgb_to_rgb("sRGB", "sRGB")?;
        let level = |v: f64| {
            mat_vec(&identity, [decode("sRGB", v); 3])
                .iter()
                .sum::<f64>()
                / 3.0
        };
        let white_level = if s.white_correction {
            level(s.white_level)
        } else {
            y_white
        };
        let black_level = if s.black_correction {
            level(s.black_level)
        } else {
            y_black
        };
        let m = (white_level - black_level) / (y_white - y_black + 1e-10);
        let q = black_level - m * y_black;
        let midgray = (0.184 - q) / m;
        ensure!(
            midgray > 0.0 && midgray.is_finite() && m.is_finite(),
            "Black/white reference levels produce an invalid exposure anchor"
        );
        result.affine = Some([m, q]);
        let profile = if film_scan { film } else { print };
        let base = if film_scan { film_base } else { print_base };
        let valid_base: Vec<_> = base.iter().copied().filter(|v| v.is_finite()).collect();
        let base_mean = valid_base.iter().sum::<f64>() / valid_base.len() as f64;
        let mut curve: Vec<_> = (0..profile.axis.len())
            .map(|i| profile.curves.iter().map(|c| c[i]).sum::<f64>() / profile.channels() as f64)
            .collect();
        let d_mid = -0.184f64.log10() - base_mean;
        let d_corrected = -midgray.log10() - base_mean;
        if film_scan {
            curve.iter_mut().for_each(|v| *v = -*v);
            let delta = -linear(-d_corrected, &curve, &profile.axis)
                + linear(-d_mid, &curve, &profile.axis);
            result.film_gain = 1.0 / 10.0f64.powf(delta);
        } else {
            let delta =
                linear(d_corrected, &curve, &profile.axis) - linear(d_mid, &curve, &profile.axis);
            result.print_gain = 10.0f64.powf(delta);
        }
        Ok(result)
    }
    pub fn correct(&self, xyz: &mut [f64; 3]) {
        if let Some([m, q]) = self.affine {
            let scale = (m * xyz[1] + q).clamp(0.0, 1.0) / (xyz[1] + 1e-10);
            for v in xyz {
                *v *= scale;
            }
        }
    }
}
