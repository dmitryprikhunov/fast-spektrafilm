//! skimage resize(order=1, mode="reflect", anti_aliasing=True).
//! Compute only Gaussian-filtered rows needed by the output, including RAW
//! samples borrowed from LibRaw, without a full-frame floating-point buffer.
use crate::Image;
use anyhow::{Result, ensure};
use rayon::prelude::*;
fn mirror(i: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let period = 2 * (n - 1) as isize;
    let i = i.rem_euclid(period);
    if i < n as isize {
        i as usize
    } else {
        (period - i) as usize
    }
}
fn kernel(factor: f64) -> (Vec<f64>, isize) {
    let sigma = ((factor - 1.) / 2.).max(0.);
    if sigma == 0. {
        return (vec![1.], 0);
    }
    let radius = (4. * sigma + 0.5) as isize;
    let mut k: Vec<_> = (-radius..=radius)
        .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
        .collect();
    let sum: f64 = k.iter().sum();
    k.iter_mut().for_each(|v| *v /= sum);
    (k, radius)
}
pub(crate) fn sampled(
    width: usize,
    height: usize,
    channels: usize,
    edge: usize,
    sample: impl Fn(usize, usize, usize) -> f64 + Sync,
) -> Result<Image> {
    ensure!(edge > 0, "Preview size must be positive");
    let scale = (edge as f64 / width.max(height) as f64).min(1.);
    let w = (width as f64 * scale) as usize;
    let h = (height as f64 * scale) as usize;
    ensure!(w > 0 && h > 0, "Preview size produces an empty axis");
    let fx = width as f64 / w as f64;
    let fy = height as f64 / h as f64;
    let (kx, rx) = kernel(fx);
    let (ky, ry) = kernel(fy);
    let mut data = vec![0.; w * h * channels];
    data.par_chunks_exact_mut(w * channels)
        .enumerate()
        .for_each(|(y, out)| {
            let center = (y as f64 + 0.5) * fy - 0.5;
            let y0 = center.floor() as isize;
            let dy = center - y0 as f64;
            let mut rows = vec![0.; 2 * width * channels];
            for row in 0..2 {
                for x in 0..width {
                    for c in 0..channels {
                        rows[(row * width + x) * channels + c] = ky
                            .iter()
                            .enumerate()
                            .map(|(k, weight)| {
                                sample(x, mirror(y0 + row as isize + k as isize - ry, height), c)
                                    * weight
                            })
                            .sum();
                    }
                }
            }
            for x in 0..w {
                let center = (x as f64 + 0.5) * fx - 0.5;
                let x0 = center.floor() as isize;
                let dx = center - x0 as f64;
                for c in 0..channels {
                    let filtered = |row: usize, xi: isize| {
                        kx.iter()
                            .enumerate()
                            .map(|(k, weight)| {
                                rows[(row * width + mirror(xi + k as isize - rx, width)) * channels
                                    + c]
                                    * weight
                            })
                            .sum::<f64>()
                    };
                    let a = filtered(0, x0) * (1. - dx) + filtered(0, x0 + 1) * dx;
                    let b = filtered(1, x0) * (1. - dx) + filtered(1, x0 + 1) * dx;
                    out[x * channels + c] = a * (1. - dy) + b * dy;
                }
            }
        });
    Image::new(w, h, channels, data)
}
pub fn resize(image: &Image, edge: usize) -> Result<Image> {
    image.validate()?;
    ensure!(edge > 0, "Preview size must be positive");
    if image.width.max(image.height) <= edge {
        return Ok(image.clone());
    }
    sampled(
        image.width,
        image.height,
        image.channels,
        edge,
        |x, y, c| image.data[(y * image.width + x) * image.channels + c],
    )
}
