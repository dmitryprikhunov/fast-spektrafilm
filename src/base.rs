use crate::{interpolation::linear, params::Base, profile::Profile};
fn reflect(i: isize, n: usize) -> usize {
    let j = i.rem_euclid(2 * n as isize);
    if j < n as isize {
        j as usize
    } else {
        (2 * n as isize - 1 - j) as usize
    }
}
pub(crate) fn gaussian(values: &[f64], sigma: f64) -> Vec<f64> {
    if sigma <= 0.0 {
        return values.to_vec();
    }
    let radius = (4.0 * sigma + 0.5) as isize;
    let mut kernel = (-radius..=radius)
        .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
        .collect::<Vec<_>>();
    let sum = kernel.iter().sum::<f64>();
    for v in &mut kernel {
        *v /= sum;
    }
    (0..values.len())
        .map(|i| {
            kernel
                .iter()
                .enumerate()
                .map(|(k, w)| values[reflect(i as isize + k as isize - radius, values.len())] * w)
                .sum()
        })
        .collect()
}
pub fn apply(profile: &mut Profile, p: &Base, is_film: bool) {
    if !p.active {
        return;
    }
    let neutral =
        p.cyan == 1.0 && p.magenta == 1.0 && p.yellow == 1.0 && (!is_film || p.tilt == 0.0);
    if !neutral {
        let peaks = if is_film {
            [460.0, 555.0, 650.0]
        } else {
            [445.0, 530.0, 610.0]
        };
        let mut channels = [p.yellow, p.magenta, p.cyan];
        if !is_film {
            for (i, v) in channels.iter_mut().enumerate() {
                *v *= linear(peaks[i], &profile.wavelengths, &profile.base);
            }
        }
        let spectral = profile
            .wavelengths
            .iter()
            .map(|w| linear(*w, &peaks, &channels))
            .collect::<Vec<_>>();
        let smoothed = gaussian(&spectral, if is_film { 7.0 } else { 4.0 });
        if is_film {
            let tilt = profile
                .wavelengths
                .iter()
                .map(|w| (p.tilt / (650.0 - 555.0) * (w - 555.0) + 1.0).max(0.0))
                .collect::<Vec<_>>();
            let tilt = gaussian(&tilt, 7.0);
            for (i, v) in profile.base.iter_mut().enumerate() {
                *v = *v * p.scale * smoothed[i] * tilt[i];
            }
        } else {
            profile.base = smoothed;
        }
    }
    for v in &mut profile.base {
        *v *= p.scale;
    }
}
