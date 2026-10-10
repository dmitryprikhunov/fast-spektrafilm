use crate::Image;
use rayon::prelude::*;

fn mirror(i: isize, n: usize) -> usize {
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
fn kernel(factor: f64) -> (Vec<f64>, isize) {
    let sigma = ((factor - 1.0) / 2.0).max(0.0);
    if sigma == 0.0 {
        return (vec![1.0], 0);
    }
    let radius = (4.0 * sigma + 0.5) as isize;
    let mut k = (-radius..=radius)
        .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
        .collect::<Vec<_>>();
    let sum = k.iter().sum::<f64>();
    for v in &mut k {
        *v /= sum;
    }
    (k, radius)
}
/// Equivalent to reference rescale(order=0), including its anti-alias filter.
/// Only rows and columns contributing to the 256px output are computed.
pub fn preview(image: &Image) -> Image {
    let max = image.width.max(image.height);
    if max <= 256 {
        return image.clone();
    }
    let width = ((image.width as f64 * 256.0 / max as f64).round_ties_even() as usize).max(1);
    let height = ((image.height as f64 * 256.0 / max as f64).round_ties_even() as usize).max(1);
    let c = image.channels;
    let fy = image.height as f64 / height as f64;
    let fx = image.width as f64 / width as f64;
    let (ky, ry) = kernel(fy);
    let (kx, rx) = kernel(fx);
    let mut rows = vec![0.0; height * image.width * c];
    rows.par_chunks_exact_mut(image.width * c)
        .enumerate()
        .for_each(|(y, row)| {
            let center = ((y as f64 + 0.5) * fy).floor() as isize;
            for (k, weight) in ky.iter().enumerate() {
                let yi = mirror(center + k as isize - ry, image.height);
                for (x, out) in row.iter_mut().enumerate() {
                    *out += image.data[yi * image.width * c + x] * weight;
                }
            }
        });
    let mut data = vec![0.0; width * height * c];
    data.par_chunks_exact_mut(width * c)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..width {
                let center = ((x as f64 + 0.5) * fx).floor() as isize;
                for (k, weight) in kx.iter().enumerate() {
                    let xi = mirror(center + k as isize - rx, image.width);
                    for ch in 0..c {
                        row[x * c + ch] += rows[(y * image.width + xi) * c + ch] * weight;
                    }
                }
            }
        });
    Image {
        width,
        height,
        channels: c,
        data,
    }
}
pub fn scale(raw: &Image, method: &str) -> f64 {
    let field = raw
        .data
        .chunks_exact(raw.channels)
        .map(|v| v.iter().sum::<f64>() / raw.channels as f64)
        .collect::<Vec<_>>();
    let mut sum = 0.0;
    let mut weights = 0.0;
    match method {
        "median" => {
            let mut f = field.clone();
            f.sort_by(f64::total_cmp);
            sum = if f.len() % 2 == 0 {
                (f[f.len() / 2 - 1] + f[f.len() / 2]) * 0.5
            } else {
                f[f.len() / 2]
            };
            weights = 1.0;
        }
        "matrix" => {
            let hh = raw.height / 5;
            let ww = raw.width / 5;
            // Empty grids meter to zero in the reference, yielding its 0 EV
            // fallback. This also affects the 1x1 print-balancing midgray.
            if hh == 0 || ww == 0 {
                return 1.0;
            }
            for r in 0..5 {
                for c in 0..5 {
                    if hh == 0 || ww == 0 {
                        continue;
                    }
                    let mut total = 0.0;
                    for y in r * hh..(r + 1) * hh {
                        for x in c * ww..(c + 1) * ww {
                            total += field[y * raw.width + x];
                        }
                    }
                    let distance = ((r as f64 - 2.0) / 2.0).hypot((c as f64 - 2.0) / 2.0)
                        / std::f64::consts::SQRT_2;
                    let weight = 0.5 * (1.0 + (std::f64::consts::PI * distance).cos());
                    sum += total / (hh * ww) as f64 * weight;
                    weights += weight;
                }
            }
        }
        "multi_zone" => {
            for (lo, hi, weight) in [(0.0, 0.05, 0.5), (0.05, 0.25, 0.3), (0.25, 0.5, 0.2)] {
                let mut total = 0.0;
                let mut count = 0;
                for (i, v) in field.iter().enumerate() {
                    let (x, y) = coords(i, raw);
                    let radius = x.hypot(y);
                    if radius >= lo && radius < hi {
                        total += v;
                        count += 1;
                    }
                }
                if count > 0 {
                    sum += weight * total / count as f64;
                    weights += weight;
                }
            }
        }
        "average" | "center_weighted" | "partial" | "highlight_weighted" => {
            for (i, v) in field.iter().enumerate() {
                let (x, y) = coords(i, raw);
                let weight = match method {
                    "center_weighted" => (-(x * x + y * y) / (2.0 * 0.2 * 0.2)).exp(),
                    "partial" => {
                        if x.hypot(y) < 0.15 {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    "highlight_weighted" => v * v,
                    _ => 1.0,
                };
                sum += v * weight;
                weights += weight;
            }
        }
        _ => return 1.0,
    }
    if weights < 1e-12 {
        sum = field.iter().sum();
        weights = field.len() as f64;
    }
    let measured = sum / weights;
    if measured == 0.0 {
        1.0
    } else {
        2.0_f64.powf(-measured.log2())
    }
}
fn coords(i: usize, image: &Image) -> (f64, f64) {
    let max = image.width.max(image.height) as f64;
    (
        ((i % image.width) as f64 / image.width as f64 - 0.5) * image.width as f64 / max,
        ((i / image.width) as f64 / image.height as f64 - 0.5) * image.height as f64 / max,
    )
}
