//! Same FIR / Young & van Vliet IIR dispatch and boundaries as experimental.
use crate::Image;
use rayon::prelude::*;
fn reflect(i: isize, n: usize) -> usize {
    let period = 2 * n as isize;
    let j = i.rem_euclid(period);
    if j < n as isize {
        j as usize
    } else {
        (period - 1 - j) as usize
    }
}
pub fn gaussian(image: &Image, sigmas: &[f64]) -> Image {
    gaussian_working(image, sigmas, false)
}
/// Preserve float32 working-array stores in the reference grain filters.
pub fn gaussian_f32(image: &Image, sigmas: &[f64]) -> Image {
    gaussian_working(image, sigmas, true)
}
fn gaussian_working(image: &Image, sigmas: &[f64], float32: bool) -> Image {
    let store = |v: f64| if float32 { v as f32 as f64 } else { v };
    let w = image.width;
    let h = image.height;
    let c = image.channels;
    let mut output = vec![0.0; image.data.len()];
    // One channel at a time bounds scratch memory independently of channel count.
    for ch in 0..c {
        let sigma = sigmas[ch.min(sigmas.len() - 1)];
        // Small physical kernels round to one tap on preview/crop grids. Keep
        // both reference stores, while avoiding planes and reflected indexing.
        if sigma > 0.0 && sigma < 3.0 && (3.0 * sigma + 0.5) as isize == 0 {
            for i in 0..w * h {
                output[i * c + ch] = store(0.0 + store(0.0 + image.data[i * c + ch]));
            }
            continue;
        }
        let mut plane = (0..w * h)
            .map(|i| image.data[i * c + ch])
            .collect::<Vec<_>>();
        if sigma > 0.0 && sigma < 3.0 {
            let radius = (3.0 * sigma + 0.5) as isize;
            let mut kernel = (-radius..=radius)
                .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
                .collect::<Vec<_>>();
            let sum: f64 = kernel.iter().sum();
            for k in &mut kernel {
                *k /= sum;
            }
            let mut temp = vec![0.0; w * h];
            // Reference FIR applies vertical convolution before horizontal.
            temp.par_chunks_exact_mut(w)
                .enumerate()
                .for_each(|(y, row)| {
                    // k stays in the same summation order for every pixel.
                    // Its reflected row is shared across x and computed once.
                    for (k, weight) in kernel.iter().enumerate() {
                        let start = reflect(y as isize + k as isize - radius, h) * w;
                        for (x, out) in row.iter_mut().enumerate() {
                            *out += plane[start + x] * weight;
                            *out = store(*out);
                        }
                    }
                });
            let horizontal: Vec<Vec<usize>> = (0..w)
                .map(|x| {
                    (0..kernel.len())
                        .map(|k| reflect(x as isize + k as isize - radius, w))
                        .collect()
                })
                .collect();
            plane
                .par_chunks_exact_mut(w)
                .enumerate()
                .for_each(|(y, row)| {
                    for (x, out) in row.iter_mut().enumerate() {
                        *out = 0.0;
                        for (k, weight) in kernel.iter().enumerate() {
                            *out += temp[y * w + horizontal[x][k]] * weight;
                        }
                        *out = store(*out);
                    }
                });
        } else if sigma >= 3.0 {
            let q = 0.98711 * sigma - 0.96330;
            let q2 = q * q;
            let q3 = q2 * q;
            let b0 = 1.57825 + 2.44413 * q + 1.4281 * q2 + 0.422205 * q3;
            let rb1 = 2.44413 * q + 2.85619 * q2 + 1.26661 * q3;
            let rb2 = -(1.4281 * q2 + 1.26661 * q3);
            let rb3 = 0.422205 * q3;
            let b = 1.0 - (rb1 + rb2 + rb3) / b0;
            let b1 = rb1 / b0;
            let b2 = rb2 / b0;
            let b3 = rb3 / b0;
            plane.par_chunks_exact_mut(w).for_each(|row| {
                let mut state = [row[0]; 3];
                for v in row.iter_mut() {
                    let out = b * (*v) + b1 * state[0] + b2 * state[1] + b3 * state[2];
                    *v = store(out);
                    state = [out, state[0], state[1]];
                }
                let mut state = [row[w - 1]; 3];
                for v in row.iter_mut().rev() {
                    let out = b * (*v) + b1 * state[0] + b2 * state[1] + b3 * state[2];
                    *v = store(out);
                    state = [out, state[0], state[1]];
                }
            });
            for x in 0..w {
                let mut state = [plane[x]; 3];
                for y in 0..h {
                    let i = y * w + x;
                    let out = b * plane[i] + b1 * state[0] + b2 * state[1] + b3 * state[2];
                    plane[i] = store(out);
                    state = [out, state[0], state[1]];
                }
                let mut state = [plane[(h - 1) * w + x]; 3];
                for y in (0..h).rev() {
                    let i = y * w + x;
                    let out = b * plane[i] + b1 * state[0] + b2 * state[1] + b3 * state[2];
                    plane[i] = store(out);
                    state = [out, state[0], state[1]];
                }
            }
        }
        for (i, v) in plane.iter().enumerate() {
            output[i * c + ch] = *v;
        }
    }
    Image {
        width: w,
        height: h,
        channels: c,
        data: output,
    }
}
pub fn exponential(image: &Image, decays: &[f64]) -> Image {
    let mut out = Image {
        width: image.width,
        height: image.height,
        channels: image.channels,
        data: vec![0.0; image.data.len()],
    };
    for (a, r) in [(0.1633, 0.5360), (0.6496, 1.5236), (0.1870, 2.7684)] {
        let component = gaussian(image, &decays.iter().map(|v| v * r).collect::<Vec<_>>());
        for (o, v) in out.data.iter_mut().zip(component.data) {
            *o += a * v;
        }
    }
    out
}
