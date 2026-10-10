//! Reference tetrahedral colour-density LUT and linear monochrome LUT.
use crate::interpolation::linear;
use anyhow::{Result, ensure};
use rayon::prelude::*;

pub(crate) struct DensityLut {
    size: usize,
    channels: usize,
    minimum: Vec<f64>,
    maximum: Vec<f64>,
    axis: Vec<f64>,
    data: Vec<f64>,
}
impl DensityLut {
    pub fn new(
        minimum: Vec<f64>,
        maximum: Vec<f64>,
        channels: usize,
        resolution: usize,
        function: impl Fn(&[f64]) -> [f64; 3] + Sync,
    ) -> Result<Self> {
        ensure!(
            [1, 3].contains(&minimum.len())
                && minimum.len() == maximum.len()
                && (1..=3).contains(&channels),
            "Unsupported density LUT shape"
        );
        ensure!(
            minimum
                .iter()
                .zip(&maximum)
                .all(|(a, b)| a.is_finite() && b.is_finite() && b > a),
            "Invalid density LUT bounds"
        );
        ensure!(
            (1..=256).contains(&resolution),
            "LUT resolution must be 1..256"
        );
        let one_dimensional = minimum.len() == 1;
        let size = if one_dimensional { 1024 } else { resolution };
        let node = |c: usize, i: usize| {
            if i == size - 1 && size > 1 {
                maximum[c]
            } else if size == 1 {
                minimum[c]
            } else {
                minimum[c] + i as f64 * ((maximum[c] - minimum[c]) / (size - 1) as f64)
            }
        };
        let axis = if one_dimensional {
            (0..size).map(|i| node(0, i)).collect()
        } else {
            Vec::new()
        };
        let count = if one_dimensional { size } else { size.pow(3) };
        let mut data = vec![0.0; count * channels];
        data.par_chunks_exact_mut(channels)
            .enumerate()
            .for_each(|(i, out)| {
                let value = if one_dimensional {
                    function(&[node(0, i)])
                } else {
                    function(&[
                        node(0, i / (size * size)),
                        node(1, i / size % size),
                        node(2, i % size),
                    ])
                };
                out.copy_from_slice(&value[..channels]);
            });
        if one_dimensional {
            data = (0..channels)
                .flat_map(|c| (0..size).map(move |i| i * channels + c))
                .map(|i| data[i])
                .collect();
        }
        Ok(Self {
            size,
            channels,
            minimum,
            maximum,
            axis,
            data,
        })
    }
    pub fn apply(&self, values: &[f64]) -> [f64; 3] {
        let mut out = [0.0; 3];
        if !self.axis.is_empty() {
            for (c, v) in out.iter_mut().enumerate().take(self.channels) {
                *v = linear(
                    values[0],
                    &self.axis,
                    &self.data[c * self.size..(c + 1) * self.size],
                );
            }
            return out;
        }
        if self.size == 1 {
            out[..self.channels].copy_from_slice(&self.data);
            return out;
        }
        let coords: [f64; 3] = std::array::from_fn(|c| {
            ((values[c] - self.minimum[c]) / (self.maximum[c] - self.minimum[c])
                * (self.size - 1) as f64)
                .clamp(0.0, (self.size - 1) as f64)
        });
        let base = coords.map(|v| (v.floor() as usize).min(self.size - 2));
        let f: [f64; 3] = std::array::from_fn(|c| coords[c] - base[c] as f64);
        let [r, g, b] = f;
        let order = if r > g {
            if g > b {
                [0, 1, 2]
            } else if r > b {
                [0, 2, 1]
            } else {
                [2, 0, 1]
            }
        } else if b > g {
            [2, 1, 0]
        } else if b > r {
            [1, 2, 0]
        } else {
            [1, 0, 2]
        };
        let at = |p: [usize; 3], c| {
            self.data[((p[0] * self.size + p[1]) * self.size + p[2]) * self.channels + c]
        };
        for (c, v) in out.iter_mut().enumerate().take(self.channels) {
            let mut p = base;
            let mut previous = at(p, c);
            *v = previous;
            for a in order {
                p[a] += 1;
                let next = at(p, c);
                *v += f[a] * (next - previous);
                previous = next;
            }
        }
        out
    }
}
