//! Inverse Beer–Lambert and calibration with pinned BLAS arithmetic.
//! Arithmetic reductions follow OpenBLAS (licenses/openblas.txt).
use crate::{
    Image, Params, base,
    color::{Tables, mat_vec},
    params::Base,
    profile::Profile,
};
use anyhow::{Result, ensure};
use nalgebra::{Matrix3, Vector3};
use rayon::prelude::*;

/// Shared GUI/CLI calibration contract: full decoded input, linear RGB,
/// and the pinned GUI's 40,000-pixel cap before the fitter's 1,500 samples.
pub fn calibrate(
    data: &std::path::Path,
    mut image: Image,
    p: &Params,
    device: bool,
) -> Result<Params> {
    image.validate()?;
    ensure!(image.channels == 3, "Calibration requires RGB");
    let mut p = p.clone();
    if p.io.input_cctf_decoding {
        image
            .data
            .iter_mut()
            .for_each(|v| *v = crate::color::decode(&p.io.input_color_space, *v));
    }
    let tables = Tables::load(data)?;
    if device {
        let n = image.width * image.height;
        if n > 40_000 {
            image = Image::new(
                40_000,
                1,
                3,
                (0..40_000)
                    .flat_map(|i| {
                        let idx = linspace_index(i, n, 40_000);
                        image.data[idx * 3..idx * 3 + 3].to_vec()
                    })
                    .collect(),
            )?;
        }
        let mut converter_params = p.clone();
        converter_params.workflow.route = "input > convert-film > scan".into();
        let sim = crate::Simulator::new(data, converter_params)?;
        p.convert.calibration =
            FilmConverter::new(&tables, sim.film(), &p)?.fit_calibration(&image)?;
    } else {
        let sample = sample_base(&image, p.convert.base_percentile)?;
        let mut stock = Profile::load(data, &p.film_profile)?;
        stock.swap_dyes(p.special.film_channel_swap)?;
        (p.film_base, p.convert.exposure_compensation_ev) = fit_base(&tables, &stock, &p, sample)?;
    }
    Ok(p)
}

pub struct FilmConverter {
    dye: Vec<[f32; 3]>,
    base: Vec<f32>,
    illum: Vec<f32>,
    cmfs: Vec<[f32; 3]>,
    weights: Vec<[[f32; 3]; 3]>,
    matrix: Matrix3<f32>,
    norm: f32,
    maximum: Vector3<f32>,
    center: Vector3<f32>,
    log_center: Vector3<f64>,
    seed_inverse: Matrix3<f64>,
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
            weights: (0..81)
                .map(|w| {
                    std::array::from_fn(|c| {
                        std::array::from_fn(|k| {
                            if film.dyes[w].iter().all(|v| v.is_finite())
                                && film.base[w].is_finite()
                            {
                                (film.dyes[w][c] * tables.cmfs[w][k]) as f32
                            } else {
                                0.
                            }
                        })
                    })
                })
                .collect(),
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
        let (rgb, j) = this.forward(this.center.cast(), true, 0);
        this.log_center = rgb.map(|v| v.max(1e-12).log10());
        let g = Matrix3::from_fn(|r, c| -j[(r, c)] / (rgb[r].max(1e-12) * std::f64::consts::LN_10));
        let svd = g.svd(true, true);
        let cutoff = svd.singular_values.max() * 1e-15;
        this.seed_inverse = svd.pseudo_inverse(cutoff).map_err(|e| anyhow::anyhow!(e))?;
        Ok(this)
    }
    // NumPy LN10 is a float64 scalar: exp, RGB and Jacobian promote to
    // double even with float32 tables. After the first Newton update, CMY
    // promotes too. Preserve this mixed precision rather than forcing f32.
    // Match the pinned Haswell OpenBLAS reductions. Mode 0 is one-row GEMV,
    // 1 is GEMM, 2 is its odd final row (the 9-column Jacobian has a 4-lane tail).
    // A one-ULP residual change alters the finite-difference LM fit substantially.
    fn forward(
        &self,
        x: Vector3<f64>,
        initial_f32: bool,
        batch: usize,
    ) -> (Vector3<f64>, Matrix3<f64>) {
        let mut xyz = Vector3::zeros();
        let mut lights = [0.; 81];
        let mut jac = Matrix3::zeros();
        let mut lanes = [Matrix3::<f64>::zeros(); 4];
        for (w, light) in lights.iter_mut().enumerate().take(self.dye.len()) {
            let d = if initial_f32 {
                let x = x.cast::<f32>();
                (self.base[w]
                    + if batch > 0 {
                        x[2].mul_add(
                            self.dye[w][2],
                            x[1].mul_add(self.dye[w][1], x[0] * self.dye[w][0]),
                        )
                    } else {
                        x[0] * self.dye[w][0] + x[1] * self.dye[w][1] + x[2] * self.dye[w][2]
                    }) as f64
            } else {
                self.base[w] as f64
                    + if batch > 0 {
                        x[2].mul_add(
                            self.dye[w][2] as f64,
                            x[1].mul_add(self.dye[w][1] as f64, x[0] * self.dye[w][0] as f64),
                        )
                    } else {
                        (0..3).map(|c| x[c] * self.dye[w][c] as f64).sum::<f64>()
                    }
            };
            let l = self.illum[w] as f64 * (-std::f64::consts::LN_10 * d).exp();
            let l = if l.is_nan() { 0. } else { l.min(1e12) };
            *light = l;
            for k in 0..3 {
                xyz[k] = if batch > 0 {
                    l.mul_add(self.cmfs[w][k] as f64, xyz[k])
                } else {
                    xyz[k] + l * self.cmfs[w][k] as f64
                };
                for c in 0..3 {
                    if batch == 2 && c * 3 + k < 8 {
                        let lane = if w < 80 { w % 4 } else { 0 };
                        lanes[lane][(k, c)] =
                            l.mul_add(self.weights[w][c][k] as f64, lanes[lane][(k, c)]);
                    } else {
                        jac[(k, c)] = if batch > 0 {
                            l.mul_add(self.weights[w][c][k] as f64, jac[(k, c)])
                        } else {
                            jac[(k, c)] + l * self.weights[w][c][k] as f64
                        };
                    }
                }
            }
        }
        if batch == 0 {
            xyz = Vector3::from_fn(|k, _| {
                let mut s = 0.;
                for w in (0..80).step_by(2) {
                    s += lights[w] * self.cmfs[w][k] as f64
                        + lights[w + 1] * self.cmfs[w + 1][k] as f64;
                }
                s + lights[80] * self.cmfs[80][k] as f64
            });
        }
        if batch == 2 {
            for c in 0..3 {
                for k in 0..3 {
                    if c * 3 + k < 8 {
                        jac[(k, c)] = (lanes[0][(k, c)] + lanes[1][(k, c)])
                            + (lanes[2][(k, c)] + lanes[3][(k, c)]);
                    }
                }
            }
        }
        let matrix = self.matrix.cast::<f64>();
        (
            if batch > 0 {
                Vector3::from_fn(|r, _| {
                    let v = xyz / self.norm as f64;
                    matrix[(r, 2)]
                        .mul_add(v[2], matrix[(r, 1)].mul_add(v[1], matrix[(r, 0)] * v[0]))
                })
            } else {
                matrix * (xyz / self.norm as f64)
            },
            matrix * (jac * (-std::f64::consts::LN_10 / self.norm as f64)),
        )
    }
    pub fn scan(&self, density: [f64; 3]) -> [f64; 3] {
        let v = self.forward(Vector3::from(density), false, 0).0;
        [v[0], v[1], v[2]]
    }
    fn bounded(&self, x: Vector3<f64>) -> Vector3<f64> {
        Vector3::from_fn(|c, _| x[c].clamp(0., self.maximum[c] as f64))
    }
    fn seed(&self, y: Vector3<f64>) -> Vector3<f64> {
        self.bounded(
            self.center.cast::<f64>()
                + self.seed_inverse
                    * (self.log_center
                        - y.map(|v| crate::convert_math::log10_f32((v as f32).max(1e-12)) as f64)),
        )
    }
    fn invert_vectors(&self, y: &[Vector3<f32>]) -> Vec<Vector3<f64>> {
        let mut x: Vec<_> = y
            .iter()
            .map(|v| self.seed(v.cast()).map(|v| v as f32 as f64))
            .collect();
        for iteration in 0..8 {
            let fj: Vec<_> = x
                .par_iter()
                .enumerate()
                .map(|(i, v)| {
                    self.forward(
                        *v,
                        iteration == 0,
                        if y.len() == 1 {
                            0
                        } else if y.len() % 2 == 1 && i == y.len() - 1 {
                            2
                        } else {
                            1
                        },
                    )
                })
                .collect();
            if fj
                .iter()
                .zip(y)
                .map(|((f, _), y)| (f - y.cast::<f64>()).abs().max())
                .fold(0.0f64, f64::max)
                < 1e-6
            {
                break;
            }
            x.par_iter_mut()
                .zip(fj)
                .zip(y)
                .for_each(|((x, (f, j)), y)| {
                    let h = Matrix3::from_fn(|r, c| {
                        (j[(0, r)] * j[(0, c)] + j[(2, r)] * j[(2, c)])
                            + j[(1, r)] * j[(1, c)]
                            + if r == c { 1e-7f32 as f64 } else { 0. }
                    });
                    if let Some(dx) = solve_pinned(
                        h,
                        &(Vector3::from_fn(|c, _| {
                            let r = f - y.cast::<f64>();
                            (j[(0, c)] * r[0] + j[(2, c)] * r[2]) + j[(1, c)] * r[1]
                        })),
                    ) {
                        *x = self.bounded(*x - dx);
                    }
                });
        }
        x
    }
    pub fn invert(&self, y: [f64; 3]) -> [f64; 3] {
        let x = self.invert_vectors(&[Vector3::from(y.map(|v| v as f32))])[0];
        [x[0], x[1], x[2]]
    }
    pub fn pretransform(&self, rgb: [f64; 3]) -> [f64; 3] {
        self.pretransform_pixel(rgb, false)
    }
    pub(crate) fn pretransform_pixel(&self, rgb: [f64; 3], batch: bool) -> [f64; 3] {
        let x = rgb.map(|v| v as f32);
        let v = if batch {
            Vector3::from_fn(|c, _| {
                x[2].mul_add(
                    self.calibration[(2, c)],
                    x[1].mul_add(self.calibration[(1, c)], x[0] * self.calibration[(0, c)]),
                )
            }) * self.gain
        } else {
            (self.calibration.transpose() * Vector3::from(x)) * self.gain
        };
        [v[0] as f64, v[1] as f64, v[2] as f64]
    }
    /// Solve one logical batch without applying device calibration or gain.
    /// LUT nodes must share convergence and total-batch BLAS classification.
    pub fn invert_image(&self, image: &Image) -> Image {
        let y: Vec<_> = image
            .data
            .chunks_exact(3)
            .map(|v| Vector3::new(v[0] as f32, v[1] as f32, v[2] as f32))
            .collect();
        let mut result = image.clone();
        for (out, x) in result.data.chunks_exact_mut(3).zip(self.invert_vectors(&y)) {
            out.copy_from_slice(x.as_slice());
        }
        result
    }
    pub fn convert(&self, image: &Image) -> Image {
        self.convert_owned(image.clone())
    }
    /// Reuse the source storage after the calibration pretransform.
    pub fn convert_owned(&self, mut image: Image) -> Image {
        let batch = image.width * image.height > 1;
        for v in image.data.chunks_exact_mut(3) {
            v.copy_from_slice(&self.pretransform_pixel([v[0], v[1], v[2]], batch));
        }
        let y: Vec<_> = image
            .data
            .chunks_exact(3)
            .map(|v| Vector3::new(v[0] as f32, v[1] as f32, v[2] as f32))
            .collect();
        for (out, x) in image.data.chunks_exact_mut(3).zip(self.invert_vectors(&y)) {
            out.copy_from_slice(x.as_slice());
        }
        image
    }
    pub fn gamut_bounds(&self) -> ([f64; 3], [f64; 3]) {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for a in 0..12 {
            for b in 0..12 {
                for c in 0..12 {
                    // np.linspace creates a float64 grid, multiplies by the
                    // float32 maximum, then stores float32 before batch forward.
                    let density = Vector3::from_fn(|k, _| {
                        (([a, b, c][k] as f64 * (1. / 11.)) * self.maximum[k] as f64) as f32 as f64
                    });
                    let v = self.forward(density, true, 1).0;
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
    fn calibration_pixels(image: &Image) -> Result<Vec<[f64; 3]>> {
        image.validate()?;
        ensure!(image.channels == 3, "Calibration requires RGB");
        let pixels: Vec<_> = image
            .data
            .chunks_exact(3)
            .map(|v| [v[0].max(0.), v[1].max(0.), v[2].max(0.)])
            .collect();
        let n = pixels.len().min(1500);
        let sampled: Vec<_> = (0..n)
            .map(|i| pixels[linspace_index(i, pixels.len(), n)])
            .collect();
        Ok(sampled)
    }
    fn residual_for(&self, sampled: &[[f64; 3]], x: &[f64]) -> Vec<f64> {
        let base_obs = sample_bright(sampled, 99., [0.2, 0.7, 0.1])
            .expect("validated nonempty calibration samples");
        let base_true = self.scan([0.; 3]);

        let corr: Vec<_> = sampled
            .iter()
            .map(|v| {
                std::array::from_fn::<_, 3, _>(|c| {
                    v[2].mul_add(x[6 + c], v[1].mul_add(x[3 + c], v[0] * x[c])) * self.gain as f64
                })
            })
            .collect();
        let inv = self.invert_vectors(
            &corr
                .iter()
                .map(|v| Vector3::from(v.map(|v| v.max(0.) as f32)))
                .collect::<Vec<_>>(),
        );
        let mut r = Vec::with_capacity(sampled.len() * 3 + 12);
        for (v, d) in corr.iter().zip(inv) {
            let f = self
                .forward(d, false, if corr.len() == 1 { 0 } else { 1 })
                .0;
            for c in 0..3 {
                r.push(v[c] - f[c]);
            }
        }
        for c in 0..3 {
            r.push(
                5. * ((0..3).map(|k| base_obs[k] * x[k * 3 + c]).sum::<f64>() * self.gain as f64
                    - base_true[c]),
            );
        }
        for (i, v) in x.iter().enumerate() {
            r.push(0.03 * (v - if i % 4 == 0 { 1. } else { 0. }));
        }
        r
    }
    pub fn calibration_residual(&self, image: &Image, matrix: [[f64; 3]; 3]) -> Result<Vec<f64>> {
        ensure!(
            matrix.iter().flatten().all(|v| v.is_finite()),
            "Nonfinite calibration matrix"
        );
        Ok(self.residual_for(
            &Self::calibration_pixels(image)?,
            &matrix.into_iter().flatten().collect::<Vec<_>>(),
        ))
    }
    pub fn fit_calibration(&self, image: &Image) -> Result<[[f64; 3]; 3]> {
        let sampled = Self::calibration_pixels(image)?;
        let residual = |x: &[f64]| self.residual_for(&sampled, x);
        let x = crate::optimization::least_squares(
            vec![1., 0., 0., 0., 1., 0., 0., 0., 1.],
            residual,
            300,
        )?;
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
    let x = crate::optimization::least_squares(
        vec![p.film_base.cyan, p.film_base.magenta, p.film_base.yellow],
        residual,
        300,
    )?;
    let ev = gain(forward(&x)).max(1e-9).log2();
    let mut b = p.film_base.clone();
    b.active = true;
    b.cyan = x[0];
    b.magenta = x[1];
    b.yellow = x[2];
    Ok((b, ev))
}

// OpenBLAS GETF2 uses a left-looking factorization: reduce each column before
// subtracting, then pivot and scale. Right-looking LU rounds differently.
fn solve_pinned(mut a: Matrix3<f64>, rhs: &Vector3<f64>) -> Option<Vector3<f64>> {
    let mut piv = [0usize; 3];
    for col in 0..3 {
        for i in 0..col {
            let t = a[(i, col)];
            a[(i, col)] = a[(piv[i], col)];
            a[(piv[i], col)] = t;
        }
        for row in 1..col {
            let v = (0..row).map(|k| a[(row, k)] * a[(k, col)]).sum::<f64>();
            a[(row, col)] -= v;
        }
        for row in col..3 {
            let v = (0..col).map(|k| a[(row, k)] * a[(k, col)]).sum::<f64>();
            a[(row, col)] -= v;
        }
        let mut p = col;
        for row in col + 1..3 {
            if a[(row, col)].abs() > a[(p, col)].abs() {
                p = row;
            }
        }
        piv[col] = p;
        if a[(p, col)] == 0. {
            return None;
        }
        for c in 0..=col {
            let t = a[(col, c)];
            a[(col, c)] = a[(p, c)];
            a[(p, c)] = t;
        }
        let inv = 1. / a[(col, col)];
        for row in col + 1..3 {
            a[(row, col)] *= inv;
        }
    }
    let mut b = *rhs;
    for i in 0..3 {
        let t = b[i];
        b[i] = b[piv[i]];
        b[piv[i]] = t;
    }
    for i in 0..3 {
        for j in i + 1..3 {
            b[j] -= a[(j, i)] * b[i];
        }
    }
    for i in (0..3).rev() {
        b[i] /= a[(i, i)];
        for j in 0..i {
            b[j] -= a[(j, i)] * b[i];
        }
    }
    Some(b)
}

// NumPy linspace multiplies a rounded float64 step, then truncates; integer
// division picks different pixels for some 40,000-point sampling grids.
fn linspace_index(i: usize, total: usize, count: usize) -> usize {
    if count == 1 {
        0
    } else if i + 1 == count {
        total - 1
    } else {
        (i as f64 * ((total - 1) as f64 / (count - 1) as f64)) as usize
    }
}
#[cfg(test)]
mod sampling_tests {
    #[test]
    fn calibration_sampling_matches_numpy_linspace() {
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/debt/sampling.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let total = case["total"].as_u64().unwrap() as usize;
            let count = case["count"].as_u64().unwrap() as usize;
            for pair in case["samples"].as_array().unwrap() {
                let i = pair[0].as_u64().unwrap() as usize;
                assert_eq!(
                    super::linspace_index(i, total, count),
                    pair[1].as_u64().unwrap() as usize,
                    "{total}/{count}/{i}"
                );
            }
        }
    }
}
