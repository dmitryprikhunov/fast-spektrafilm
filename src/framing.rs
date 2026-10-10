//! Reference framing: NumPy crop rounding, mirror anti-aliasing and cubic
//! B-spline rescaling. Spline recurrences follow SciPy's ni_splines.c;
//! its BSD license is retained in licenses/scipy.txt.
use crate::{Image, params::Io};
use anyhow::{Result, ensure};
use rayon::prelude::*;

pub fn validate(io: &Io) -> Result<()> {
    ensure!(
        io.upscale_factor.is_finite() && io.upscale_factor > 0.0,
        "Upscale factor must be positive and finite"
    );
    ensure!(
        io.crop_center.iter().all(|v| v.is_finite())
            && io.crop_size.iter().all(|v| v.is_finite() && *v > 0.0),
        "Crop coordinates must be finite and sizes positive"
    );
    Ok(())
}

pub fn preprocess(mut image: Image, io: &Io) -> Result<Image> {
    if io.crop {
        let long = image.width.max(image.height) as f64;
        let span = |n: usize, center: f64, size: f64| {
            let count = (long * size).round_ties_even() as isize;
            let mid = (n as f64 * center).round_ties_even();
            let mut start = (mid - count as f64 / 2.0).round_ties_even() as isize;
            start = start.max(0);
            if start + count > n as isize {
                start = n as isize - count;
            }
            // NumPy slicing wraps negative starts before clamping to the array.
            let index =
                |i: isize| (if i < 0 { i + n as isize } else { i }).clamp(0, n as isize) as usize;
            (index(start), index(start + count))
        };
        let (x0, x1) = span(image.width, io.crop_center[0], io.crop_size[0]);
        let (y0, y1) = span(image.height, io.crop_center[1], io.crop_size[1]);
        ensure!(x1 > x0 && y1 > y0, "Crop produces an empty image");
        let mut data = Vec::with_capacity((x1 - x0) * (y1 - y0) * image.channels);
        for y in y0..y1 {
            data.extend_from_slice(
                &image.data[(y * image.width + x0) * image.channels
                    ..(y * image.width + x1) * image.channels],
            );
        }
        image = Image::new(x1 - x0, y1 - y0, image.channels, data)?;
    }
    if io.upscale_factor != 1.0 {
        image = rescale(image, io.upscale_factor)?;
    }
    Ok(image)
}

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

fn spline_filter(line: &mut [f64]) {
    let n = line.len();
    if n < 2 {
        return;
    }
    let z: f64 = -0.267_949_192_431_122_7;
    let gain = (1.0 - z) * (1.0 - 1.0 / z);
    for v in line.iter_mut() {
        *v *= gain;
    }
    let zn = z.powf((n - 1) as f64);
    line[0] += zn * line[n - 1];
    let mut zi = z;
    for i in 1..n - 1 {
        line[0] += zi * (line[i] + zn * line[n - 1 - i]);
        zi *= z;
    }
    line[0] /= 1.0 - zn * zn;
    for i in 1..n {
        line[i] += z * line[i - 1];
    }
    line[n - 1] = (z * line[n - 2] + line[n - 1]) * z / (z * z - 1.0);
    for i in (0..n - 1).rev() {
        line[i] = z * (line[i + 1] - line[i]);
    }
}

fn filter_axis(image: &mut Image, axis: usize, spline: bool, sigma: f64) {
    let dims = [image.height, image.width, image.channels];
    let strides = [image.width * image.channels, image.channels, 1];
    let n = dims[axis];
    if n < 2 || (!spline && sigma <= 1e-15) {
        return;
    }
    let radius = (sigma * 4.0 + 0.5) as isize;
    let mut kernel = if spline {
        vec![]
    } else {
        (-radius..=radius)
            .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
            .collect::<Vec<_>>()
    };
    let sum = kernel.iter().sum::<f64>();
    for v in &mut kernel {
        *v /= sum;
    }
    let mut line = vec![0.0; n];
    let mut filtered = vec![0.0; n];
    for start in 0..image.data.len() {
        if !(start / strides[axis]).is_multiple_of(n) {
            continue;
        }
        for (i, v) in line.iter_mut().enumerate() {
            *v = image.data[start + i * strides[axis]];
        }
        if spline {
            spline_filter(&mut line);
        } else {
            for (i, v) in filtered.iter_mut().enumerate() {
                // SciPy sums paired symmetric taps, centre first.
                *v = line[i] * kernel[radius as usize];
                for j in 1..=radius {
                    *v += (line[mirror(i as isize - j, n)] + line[mirror(i as isize + j, n)])
                        * kernel[(radius + j) as usize];
                }
            }
            line.copy_from_slice(&filtered);
        }
        for (i, v) in line.iter().enumerate() {
            image.data[start + i * strides[axis]] = *v;
        }
    }
}

fn weights(mut x: f64, n: usize) -> ([usize; 4], [f64; 4]) {
    if n == 1 {
        x = 0.0;
    } else if x < 0.0 {
        x = -x;
    } else if x > (n - 1) as f64 {
        x = 2.0 * (n - 1) as f64 - x;
    }
    let start = x.floor() as isize - 1;
    let y = x - x.floor();
    let z = 1.0 - y;
    let mut w = [
        z * z * z / 6.0,
        (y * y * (y - 2.0) * 3.0 + 4.0) / 6.0,
        (z * z * (z - 2.0) * 3.0 + 4.0) / 6.0,
        1.0,
    ];
    for i in 0..3 {
        w[3] -= w[i];
    }
    (std::array::from_fn(|i| mirror(start + i as isize, n)), w)
}

fn rescale(mut image: Image, factor: f64) -> Result<Image> {
    let w = (image.width as f64 * factor).round_ties_even().max(1.0) as usize;
    let h = (image.height as f64 * factor).round_ties_even().max(1.0) as usize;
    ensure!(
        w.checked_mul(h)
            .and_then(|v| v.checked_mul(image.channels))
            .is_some(),
        "Rescaled dimensions overflow"
    );
    let lo = image.data.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = image.data.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let fy = image.height as f64 / h as f64;
    let fx = image.width as f64 / w as f64;
    filter_axis(&mut image, 0, false, ((fy - 1.0) / 2.0).max(0.0));
    filter_axis(&mut image, 1, false, ((fx - 1.0) / 2.0).max(0.0));
    for axis in 0..3 {
        filter_axis(&mut image, axis, true, 0.0);
    }
    let xw = (0..w)
        .map(|x| weights((x as f64 + 0.5) * fx - 0.5, image.width))
        .collect::<Vec<_>>();
    let yw = (0..h)
        .map(|y| weights((y as f64 + 0.5) * fy - 0.5, image.height))
        .collect::<Vec<_>>();
    let cw = (0..image.channels)
        .map(|c| weights(c as f64, image.channels))
        .collect::<Vec<_>>();
    let mut data = vec![0.0; w * h * image.channels];
    data.par_chunks_exact_mut(w * image.channels)
        .enumerate()
        .for_each(|(y, row)| {
            for x in 0..w {
                for c in 0..image.channels {
                    let mut value = 0.0;
                    for a in 0..4 {
                        for b in 0..4 {
                            for d in 0..4 {
                                let mut term = image.data[(yw[y].0[a] * image.width + xw[x].0[b])
                                    * image.channels
                                    + cw[c].0[d]];
                                term *= yw[y].1[a];
                                term *= xw[x].1[b];
                                term *= cw[c].1[d];
                                value += term;
                            }
                        }
                    }
                    row[x * image.channels + c] = value.clamp(lo, hi);
                }
            }
        });
    Image::new(w, h, image.channels, data)
}
