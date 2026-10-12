//! Inverse Beer–Lambert scan model and clear-base/device calibration.
use crate::{
    Image, Params, base,
    color::{Tables, mat_vec},
    params::Base,
    profile::Profile,
};
use anyhow::{Result, ensure};
use nalgebra::{DMatrix, DVector, Matrix3, Vector3};
use rayon::prelude::*;

pub struct FilmConverter {
    dye: Vec<[f32; 3]>,
    base: Vec<f32>,
    illum: Vec<f32>,
    cmfs: Vec<[f32; 3]>,
    matrix: Matrix3<f32>,
    norm: f32,
    maximum: Vector3<f32>,
    center: Vector3<f32>,
    log_center: Vector3<f32>,
    seed_inverse: Matrix3<f32>,
    calibration: Matrix3<f32>,
    gain: f32,
}
fn matrix(rows: [[f64; 3]; 3]) -> Matrix3<f32> {
    Matrix3::from_fn(|r, c| rows[r][c] as f32)
}
impl FilmConverter {
    /// `film` must have its tuned base and normalized development curves.
    pub fn new(tables: &Tables, film: &Profile, p: &Params) -> Result<Self> {
        ensure!(
            film.channels() == 3,
            "Negative conversion requires a three-channel film profile"
        );
        ensure!(
            p.convert.exposure_compensation_ev.is_finite()
                && p.convert
                    .calibration
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite()),
            "Invalid scan exposure or calibration"
        );
        let lamp = tables.illuminant(&p.convert.scan_illuminant)?;
        let norm = (0..81)
            .map(|w| lamp.spectrum[w] * tables.cmfs[w][1])
            .sum::<f64>() as f32;
        let mut this = Self {
            dye: Vec::new(),
            base: Vec::new(),
            illum: Vec::new(),
            cmfs: tables.cmfs.iter().map(|v| v.map(|x| x as f32)).collect(),
            matrix: matrix(tables.xyz_to_rgb(&p.io.input_color_space, lamp.xy)?),
            norm,
            maximum: Vector3::from_fn(|c, _| film.max()[c] as f32),
            center: Vector3::zeros(),
            log_center: Vector3::zeros(),
            seed_inverse: Matrix3::zeros(),
            calibration: matrix(p.convert.calibration),
            gain: 2.0f64.powf(p.convert.exposure_compensation_ev) as f32,
        };
        for w in 0..81 {
            let valid = film.base[w].is_finite() && film.dyes[w].iter().all(|v| v.is_finite());
            this.dye.push(std::array::from_fn(|c| {
                if valid { film.dyes[w][c] as f32 } else { 0. }
            }));
            this.base.push(if valid { film.base[w] as f32 } else { 0. });
            this.illum
                .push(if valid { lamp.spectrum[w] as f32 } else { 0. });
        }
        this.center = this.maximum * 0.5;
        let (rgb, j) = this.forward(this.center);
        this.log_center = rgb.map(|v| v.max(1e-12).log10());
        let g = Matrix3::from_fn(|r, c| -j[(r, c)] / (rgb[r].max(1e-12) * std::f32::consts::LN_10));
        this.seed_inverse = g
            .svd(true, true)
            .pseudo_inverse(1e-7)
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(this)
    }
    fn forward(&self, x: Vector3<f32>) -> (Vector3<f32>, Matrix3<f32>) {
        let mut xyz = Vector3::zeros();
        let mut jac = Matrix3::zeros();
        for w in 0..self.dye.len() {
            let d = self.base[w] + (0..3).map(|c| x[c] * self.dye[w][c]).sum::<f32>();
            let l = self.illum[w] * (-std::f32::consts::LN_10 * d).exp();
            let l = if l.is_nan() { 0. } else { l.min(1e12) };
            for k in 0..3 {
                xyz[k] += l * self.cmfs[w][k];
                for c in 0..3 {
                    jac[(k, c)] += l * (self.dye[w][c] * self.cmfs[w][k]);
                }
            }
        }
        (
            self.matrix * (xyz / self.norm),
            self.matrix * (jac * (-std::f32::consts::LN_10 / self.norm)),
        )
    }
    pub fn scan(&self, density: [f64; 3]) -> [f64; 3] {
        let v = self.forward(Vector3::from(density.map(|v| v as f32))).0;
        [v[0] as f64, v[1] as f64, v[2] as f64]
    }
    fn bounded(&self, x: Vector3<f32>) -> Vector3<f32> {
        Vector3::from_fn(|c, _| x[c].clamp(0., self.maximum[c]))
    }
    fn seed(&self, y: Vector3<f32>) -> Vector3<f32> {
        self.bounded(
            self.center + self.seed_inverse * (self.log_center - y.map(|v| v.max(1e-12).log10())),
        )
    }
    fn invert_vectors(&self, y: &[Vector3<f32>]) -> Vec<Vector3<f32>> {
        let mut x: Vec<_> = y.iter().map(|v| self.seed(*v)).collect();
        for _ in 0..8 {
            let fj: Vec<_> = x.par_iter().map(|v| self.forward(*v)).collect();
            if fj
                .iter()
                .zip(y)
                .map(|((f, _), y)| (f - y).abs().max())
                .fold(0.0f32, f32::max)
                < 1e-6
            {
                break;
            }
            x.par_iter_mut()
                .zip(fj)
                .zip(y)
                .for_each(|((x, (f, j)), y)| {
                    let h = j.transpose() * j + Matrix3::identity() * 1e-7;
                    if let Some(dx) = h.lu().solve(&(j.transpose() * (f - y))) {
                        *x = self.bounded(*x - dx);
                    }
                });
        }
        x
    }
    pub fn invert(&self, y: [f64; 3]) -> [f64; 3] {
        let x = self.invert_vectors(&[Vector3::from(y.map(|v| v as f32))])[0];
        [x[0] as f64, x[1] as f64, x[2] as f64]
    }
    pub fn pretransform(&self, rgb: [f64; 3]) -> [f64; 3] {
        let v = (self.calibration.transpose() * Vector3::from(rgb.map(|v| v as f32))) * self.gain;
        [v[0] as f64, v[1] as f64, v[2] as f64]
    }
    pub fn convert(&self, image: &Image) -> Image {
        let y: Vec<_> = image
            .data
            .chunks_exact(3)
            .map(|v| Vector3::from(self.pretransform([v[0], v[1], v[2]]).map(|v| v as f32)))
            .collect();
        let mut result = image.clone();
        for (out, x) in result.data.chunks_exact_mut(3).zip(self.invert_vectors(&y)) {
            for c in 0..3 {
                out[c] = x[c] as f64;
            }
        }
        result
    }
    pub fn gamut_bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for a in 0..12 {
            for b in 0..12 {
                for c in 0..12 {
                    let v = self.scan(std::array::from_fn(|k| {
                        [a, b, c][k] as f64 / 11. * self.maximum[k] as f64
                    }));
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                }
            }
        }
        let margin: [f64; 3] = std::array::from_fn(|c| 0.02 * (hi[c] - lo[c]));
        (
            std::array::from_fn(|c| lo[c] - margin[c]),
            std::array::from_fn(|c| hi[c] + margin[c]),
        )
    }
    pub fn fit_calibration(&self, image: &Image) -> Result<[[f64; 3]; 3]> {
        image.validate()?;
        ensure!(image.channels == 3, "Calibration requires RGB");
        let pixels: Vec<_> = image
            .data
            .chunks_exact(3)
            .map(|v| [v[0].max(0.), v[1].max(0.), v[2].max(0.)])
            .collect();
        let n = pixels.len().min(1500);
        let sampled: Vec<_> = (0..n)
            .map(|i| {
                pixels[if n == 1 {
                    0
                } else {
                    i * (pixels.len() - 1) / (n - 1)
                }]
            })
            .collect();
        let base_obs = sample_bright(&sampled, 99., [0.2, 0.7, 0.1])?;
        let base_true = self.scan([0.; 3]);
        let residual = |x: &[f64]| {
            let corr: Vec<_> = sampled
                .iter()
                .map(|v| {
                    std::array::from_fn::<_, 3, _>(|c| {
                        (0..3).map(|k| v[k] * x[k * 3 + c]).sum::<f64>() * self.gain as f64
                    })
                })
                .collect();
            let inv = self.invert_vectors(
                &corr
                    .iter()
                    .map(|v| Vector3::from(v.map(|v| v.max(0.) as f32)))
                    .collect::<Vec<_>>(),
            );
            let mut r = Vec::with_capacity(n * 3 + 12);
            for (v, d) in corr.iter().zip(inv) {
                let f = self.forward(d).0;
                for c in 0..3 {
                    r.push(v[c] - f[c] as f64);
                }
            }
            for c in 0..3 {
                r.push(
                    5. * ((0..3).map(|k| base_obs[k] * x[k * 3 + c]).sum::<f64>()
                        * self.gain as f64
                        - base_true[c]),
                );
            }
            for (i, v) in x.iter().enumerate() {
                r.push(0.03 * (v - if i % 4 == 0 { 1. } else { 0. }));
            }
            r
        };
        let x = least_squares(vec![1., 0., 0., 0., 1., 0., 0., 0., 1.], residual, 30, 1e-4)?;
        Ok(std::array::from_fn(|r| {
            std::array::from_fn(|c| x[r * 3 + c])
        }))
    }
}
fn sample_bright(pixels: &[[f64; 3]], percentile: f64, weights: [f64; 3]) -> Result<[f64; 3]> {
    ensure!(
        !pixels.is_empty() && percentile.is_finite(),
        "No finite pixels for base detection"
    );
    let lum: Vec<_> = pixels
        .iter()
        .map(|p| (0..3).map(|c| p[c] * weights[c]).sum::<f64>())
        .collect();
    let mut sorted = lum.clone();
    sorted.sort_by(f64::total_cmp);
    let q = percentile.clamp(0., 100.) / 100. * (sorted.len() - 1) as f64;
    let i = q.floor() as usize;
    let threshold =
        sorted[i] + (q - i as f64) * (sorted[(i + 1).min(sorted.len() - 1)] - sorted[i]);
    let mut sum = [0.; 3];
    let mut n = 0.;
    for (p, l) in pixels.iter().zip(lum) {
        if l >= threshold {
            for c in 0..3 {
                sum[c] += p[c];
            }
            n += 1.;
        }
    }
    Ok(sum.map(|v| v / n))
}
pub fn sample_base(image: &Image, percentile: f64) -> Result<[f64; 3]> {
    ensure!(image.channels == 3, "Base detection requires RGB");
    let pixels: Vec<_> = image
        .data
        .chunks_exact(3)
        .filter(|p| p.iter().all(|v| v.is_finite()))
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    sample_bright(&pixels, percentile, [0.2126, 0.7152, 0.0722])
}
pub fn fit_base(
    tables: &Tables,
    stock: &Profile,
    p: &Params,
    target: [f64; 3],
) -> Result<(Base, f64)> {
    ensure!(
        target.iter().all(|v| v.is_finite()) && target.iter().map(|v| v * v).sum::<f64>() > 0.,
        "Base sample must have positive energy"
    );
    let lamp = tables.illuminant(&p.convert.scan_illuminant)?;
    let m = tables.xyz_to_rgb(&p.io.input_color_space, lamp.xy)?;
    let norm = (0..81)
        .map(|w| lamp.spectrum[w] * tables.cmfs[w][1])
        .sum::<f64>();
    let forward = |x: &[f64]| {
        let mut film = stock.clone();
        let mut b = p.film_base.clone();
        b.active = true;
        b.cyan = x[0];
        b.magenta = x[1];
        b.yellow = x[2];
        base::apply(&mut film, &b, true);
        let mut xyz = [0.; 3];
        for w in 0..81 {
            if film.base[w].is_finite() && film.dyes[w].iter().all(|v| v.is_finite()) {
                for (c, v) in xyz.iter_mut().enumerate() {
                    *v += lamp.spectrum[w] * 10.0f64.powf(-film.base[w]) * tables.cmfs[w][c] / norm;
                }
            }
        }
        mat_vec(&m, xyz)
    };
    let gain = |f: [f64; 3]| {
        (0..3).map(|c| f[c] * target[c]).sum::<f64>() / target.iter().map(|v| v * v).sum::<f64>()
    };
    let residual = |x: &[f64]| {
        let f = forward(x);
        let g = gain(f);
        let mut r: Vec<_> = (0..3).map(|c| f[c] - g * target[c]).collect();
        r.extend(x.iter().map(|v| 0.02 * (v - 1.)));
        r
    };
    let x = least_squares(
        vec![p.film_base.cyan, p.film_base.magenta, p.film_base.yellow],
        residual,
        100,
        1e-5,
    )?;
    let ev = gain(forward(&x)).max(1e-9).log2();
    let mut b = p.film_base.clone();
    b.active = true;
    b.cyan = x[0];
    b.magenta = x[1];
    b.yellow = x[2];
    Ok((b, ev))
}
fn least_squares(
    mut x: Vec<f64>,
    residual: impl Fn(&[f64]) -> Vec<f64>,
    iterations: usize,
    step: f64,
) -> Result<Vec<f64>> {
    let mut lambda = 1e-3;
    for _ in 0..iterations {
        let r = DVector::from_vec(residual(&x));
        ensure!(r.iter().all(|v| v.is_finite()), "Calibration diverged");
        let j = DMatrix::from_fn(r.len(), x.len(), |_, _| 0.);
        let mut j = j;
        for c in 0..x.len() {
            let mut trial = x.clone();
            let h = step * x[c].abs().max(1.);
            trial[c] += h;
            let v = residual(&trial);
            for k in 0..r.len() {
                j[(k, c)] = (v[k] - r[k]) / h;
            }
        }
        let h = j.transpose() * &j + DMatrix::identity(x.len(), x.len()) * lambda;
        let Some(dx) = h.lu().solve(&(j.transpose() * &r)) else {
            break;
        };
        let trial: Vec<_> = x.iter().zip(&dx).map(|(v, d)| v - d).collect();
        let cost: f64 = residual(&trial).iter().map(|v| v * v).sum();
        if cost < r.norm_squared() {
            x = trial;
            lambda = (lambda * 0.3).max(1e-12);
            if dx.norm() < 1e-9 {
                break;
            }
        } else {
            lambda *= 10.;
        }
    }
    Ok(x)
}
