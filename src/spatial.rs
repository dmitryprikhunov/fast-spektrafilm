//! Same FIR / Young & van Vliet IIR dispatch and boundaries as experimental.
use crate::Image;
use rayon::prelude::*;
const STRIP: usize = 16;
const COLUMNS: usize = 32;
trait Scalar: Copy + Default + Send + Sync {
    fn get(self) -> f64;
    fn put(value: f64) -> Self;
}
impl Scalar for f64 {
    fn get(self) -> f64 {
        self
    }
    fn put(value: f64) -> Self {
        value
    }
}
impl Scalar for f32 {
    fn get(self) -> f64 {
        self as f64
    }
    fn put(value: f64) -> Self {
        value as f32
    }
}
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
    Image {
        width: image.width,
        height: image.height,
        channels: image.channels,
        data: filter(
            &image.data,
            image.width,
            image.height,
            image.channels,
            sigmas,
            float32,
        ),
    }
}
pub(crate) fn gaussian_f32_data(
    data: &[f32],
    w: usize,
    h: usize,
    c: usize,
    sigmas: &[f64],
) -> Vec<f32> {
    filter(data, w, h, c, sigmas, true)
}
fn filter<T: Scalar>(
    input: &[T],
    w: usize,
    h: usize,
    c: usize,
    sigmas: &[f64],
    float32: bool,
) -> Vec<T> {
    let store = |v: f64| T::put(if float32 { v as f32 as f64 } else { v });
    let mut output = vec![T::default(); input.len()];
    for ch in 0..c {
        let sigma = sigmas[ch.min(sigmas.len() - 1)];
        if sigma <= 0. {
            for i in 0..w * h {
                output[i * c + ch] = input[i * c + ch];
            }
        } else if sigma < 3. {
            let radius = (3. * sigma + 0.5) as isize;
            let mut kernel: Vec<_> = (-radius..=radius)
                .map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp())
                .collect();
            let sum: f64 = kernel.iter().sum();
            for k in &mut kernel {
                *k /= sum;
            }
            output
                .par_chunks_mut(STRIP * w * c)
                .enumerate()
                .for_each_init(
                    || vec![T::default(); STRIP * w],
                    |temp, (strip, out)| {
                        let rows = out.len() / (w * c);
                        temp[..rows * w].fill(T::default());
                        for y in 0..rows {
                            let y_global = strip * STRIP + y;
                            // Reference FIR is vertical then horizontal; retain every
                            // per-tap vertical f32 store, not just the final value.
                            for (k, weight) in kernel.iter().enumerate() {
                                let row = reflect(y_global as isize + k as isize - radius, h) * w;
                                for x in 0..w {
                                    temp[y * w + x] = store(
                                        temp[y * w + x].get()
                                            + input[(row + x) * c + ch].get() * weight,
                                    );
                                }
                            }
                            for x in 0..w {
                                let mut value = 0.;
                                for (k, weight) in kernel.iter().enumerate() {
                                    let xi = if x >= radius as usize && x + (radius as usize) < w {
                                        x + k - radius as usize
                                    } else {
                                        reflect(x as isize + k as isize - radius, w)
                                    };
                                    value += temp[y * w + xi].get() * weight;
                                }
                                out[(y * w + x) * c + ch] = store(value);
                            }
                        }
                    },
                );
        } else {
            let q = 0.98711 * sigma - 0.96330;
            let q2 = q * q;
            let q3 = q2 * q;
            let b0 = 1.57825 + 2.44413 * q + 1.4281 * q2 + 0.422205 * q3;
            let rb1 = 2.44413 * q + 2.85619 * q2 + 1.26661 * q3;
            let rb2 = -(1.4281 * q2 + 1.26661 * q3);
            let rb3 = 0.422205 * q3;
            let b = 1. - (rb1 + rb2 + rb3) / b0;
            let b1 = rb1 / b0;
            let b2 = rb2 / b0;
            let b3 = rb3 / b0;
            output
                .par_chunks_mut(w * c)
                .enumerate()
                .for_each(|(y, row)| {
                    let mut state = [input[y * w * c + ch].get(); 3];
                    for x in 0..w {
                        let value = b * input[(y * w + x) * c + ch].get()
                            + b1 * state[0]
                            + b2 * state[1]
                            + b3 * state[2];
                        row[x * c + ch] = store(value);
                        state = [value, state[0], state[1]];
                    }
                    let mut state = [row[(w - 1) * c + ch].get(); 3];
                    for x in (0..w).rev() {
                        let value = b * row[x * c + ch].get()
                            + b1 * state[0]
                            + b2 * state[1]
                            + b3 * state[2];
                        row[x * c + ch] = store(value);
                        state = [value, state[0], state[1]];
                    }
                });
            // Use the output as horizontal storage, then gather independent
            // contiguous column blocks. Scratch remains one scalar plane.
            let mut columns = vec![T::default(); w * h];
            columns
                .par_chunks_mut(COLUMNS * h)
                .enumerate()
                .for_each(|(block, values)| {
                    let x0 = block * COLUMNS;
                    let width = COLUMNS.min(w - x0);
                    for y in 0..h {
                        for x in 0..width {
                            values[y * width + x] = output[(y * w + x0 + x) * c + ch];
                        }
                    }
                    for x in 0..width {
                        let mut state = [values[x].get(); 3];
                        for y in 0..h {
                            let i = y * width + x;
                            let value =
                                b * values[i].get() + b1 * state[0] + b2 * state[1] + b3 * state[2];
                            values[i] = store(value);
                            state = [value, state[0], state[1]];
                        }
                        let mut state = [values[(h - 1) * width + x].get(); 3];
                        for y in (0..h).rev() {
                            let i = y * width + x;
                            let value =
                                b * values[i].get() + b1 * state[0] + b2 * state[1] + b3 * state[2];
                            values[i] = store(value);
                            state = [value, state[0], state[1]];
                        }
                    }
                });
            output
                .par_chunks_mut(w * c)
                .enumerate()
                .for_each(|(y, row)| {
                    for x in 0..w {
                        let block = x / COLUMNS;
                        let width = COLUMNS.min(w - block * COLUMNS);
                        row[x * c + ch] = columns[block * COLUMNS * h + y * width + x % COLUMNS];
                    }
                });
        }
    }
    output
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
