use crate::params::OutputGamut;
use anyhow::{Context, Result, ensure};
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};
pub type Matrix = [[f64; 3]; 3];
pub fn mat_vec(m: &Matrix, v: [f64; 3]) -> [f64; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}
pub fn mat_mul(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|i| {
        std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j])
    })
}
pub fn inverse(m: &Matrix) -> Matrix {
    let a = m[0][0];
    let b = m[0][1];
    let c = m[0][2];
    let d = m[1][0];
    let e = m[1][1];
    let f = m[1][2];
    let g = m[2][0];
    let h = m[2][1];
    let i = m[2][2];
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [
            (e * i - f * h) / det,
            (c * h - b * i) / det,
            (b * f - c * e) / det,
        ],
        [
            (f * g - d * i) / det,
            (a * i - c * g) / det,
            (c * d - a * f) / det,
        ],
        [
            (d * h - e * g) / det,
            (b * g - a * h) / det,
            (a * e - b * d) / det,
        ],
    ]
}
pub fn xy_xyz(xy: [f64; 2]) -> [f64; 3] {
    [xy[0] / xy[1], 1.0, (1.0 - xy[0] - xy[1]) / xy[1]]
}
pub fn adaptation(cat: &Matrix, source: [f64; 2], target: [f64; 2]) -> Matrix {
    let src = mat_vec(cat, xy_xyz(source));
    let dst = mat_vec(cat, xy_xyz(target));
    let scale = [
        [dst[0] / src[0], 0.0, 0.0],
        [0.0, dst[1] / src[1], 0.0],
        [0.0, 0.0, dst[2] / src[2]],
    ];
    mat_mul(&inverse(cat), &mat_mul(&scale, cat))
}
#[derive(Clone, Deserialize)]
pub struct ColorSpace {
    pub to_xyz: Matrix,
    pub from_xyz: Matrix,
    pub white: [f64; 2],
}
#[derive(Clone, Deserialize)]
pub struct Illuminant {
    pub spectrum: Vec<f64>,
    pub xy: [f64; 2],
}
#[derive(Deserialize)]
pub struct Tables {
    pub baseline_commit: String,
    pub cmfs: Vec<[f64; 3]>,
    pub spaces: BTreeMap<String, ColorSpace>,
    pub illuminants: BTreeMap<String, Illuminant>,
    pub cat02: Matrix,
    pub cat16: Matrix,
    pub oklab_m1: Matrix,
    pub oklab_m2: Matrix,
    pub jzazbz_m1: Matrix,
    pub jzazbz_m2: Matrix,
    pub dichroics: Vec<[f64; 3]>,
    pub camera_filters: BTreeMap<String, Option<Vec<f64>>>,
    pub input_hull: Vec<[f64; 2]>,
    pub locus: Vec<[f64; 2]>,
}
impl Tables {
    pub fn load(path: &Path) -> Result<Self> {
        Ok(serde_json::from_reader(std::fs::File::open(
            path.join("reference_tables.json"),
        )?)?)
    }
    pub fn space(&self, name: &str) -> Result<&ColorSpace> {
        self.spaces
            .get(name)
            .with_context(|| format!("Unknown color space: {name}"))
    }
    pub fn illuminant(&self, name: &str) -> Result<Illuminant> {
        if let Some(temperature) = name.strip_prefix("BB") {
            let t: f64 = temperature
                .parse()
                .context("Blackbody illuminants use BB followed by kelvin")?;
            ensure!(
                t.is_finite() && t > 0.0,
                "Blackbody temperature must be positive and finite"
            );
            let mut spectrum: Vec<_> = (0..81)
                .map(|i| {
                    let wavelength = (380.0 + 5.0 * i as f64) * 1e-9;
                    3.741771e-16
                        / (std::f64::consts::PI
                            * wavelength.powi(5)
                            * ((0.014388 / (wavelength * t)).exp() - 1.0))
                })
                .collect();
            let mean = spectrum.iter().sum::<f64>() / 81.0;
            ensure!(
                mean.is_finite() && mean > 0.0,
                "Blackbody temperature outside numerical range"
            );
            for v in &mut spectrum {
                *v /= mean;
            }
            let xyz: [f64; 3] = std::array::from_fn(|c| {
                spectrum
                    .iter()
                    .zip(&self.cmfs)
                    .map(|(v, cmf)| v * cmf[c])
                    .sum()
            });
            let sum: f64 = xyz.iter().sum();
            return Ok(Illuminant {
                spectrum,
                xy: [xyz[0] / sum, xyz[1] / sum],
            });
        }
        self.illuminants
            .get(name)
            .cloned()
            .with_context(|| format!("Unknown illuminant: {name}"))
    }
    pub fn rgb_to_xyz(&self, input: &str, white: [f64; 2]) -> Result<Matrix> {
        let cs = self.space(input)?;
        Ok(mat_mul(
            &adaptation(&self.cat16, cs.white, white),
            &cs.to_xyz,
        ))
    }
    pub fn xyz_to_rgb(&self, output: &str, white: [f64; 2]) -> Result<Matrix> {
        let cs = self.space(output)?;
        Ok(mat_mul(
            &cs.from_xyz,
            &adaptation(&self.cat02, white, cs.white),
        ))
    }
    pub fn rgb_to_rgb(&self, input: &str, output: &str) -> Result<Matrix> {
        let a = self.space(input)?;
        let b = self.space(output)?;
        Ok(mat_mul(
            &b.from_xyz,
            &mat_mul(&adaptation(&self.cat02, a.white, b.white), &a.to_xyz),
        ))
    }
}
pub fn encode(name: &str, v: f64) -> f64 {
    match name {
        "sRGB" | "Display P3" => {
            if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        }
        "ProPhoto RGB" => {
            if v < 1.0 / 512.0 {
                16.0 * v
            } else {
                v.powf(1.0 / 1.8)
            }
        }
        "Adobe RGB (1998)" => v.powf(256.0 / 563.0),
        "ITU-R BT.2020" => {
            if v < 0.018 {
                4.5 * v
            } else {
                1.099 * v.powf(0.45) - 0.099
            }
        }
        _ => v,
    }
}
pub fn decode(name: &str, v: f64) -> f64 {
    match name {
        "sRGB" | "Display P3" => {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        "ProPhoto RGB" => {
            if v < 1.0 / 32.0 {
                v / 16.0
            } else {
                v.powf(1.8)
            }
        }
        "Adobe RGB (1998)" => v.powf(563.0 / 256.0),
        "ITU-R BT.2020" => {
            // The pinned colour implementation tests against OETF(beta),
            // i.e. 1.099 * 0.018^0.45 - 0.099, rather than 4.5 * beta.
            if v < 0.081_247_944_035_140_46 {
                v / 4.5
            } else {
                ((v + 0.099) / 1.099).powf(1.0 / 0.45)
            }
        }
        _ => v,
    }
}
pub fn knee(d: f64, [threshold, limit, power]: [f64; 3]) -> f64 {
    if d <= threshold {
        return d;
    }
    let scale = limit - threshold;
    let x = (d - threshold) / scale;
    threshold + scale * x / (1.0 + x.powf(power)).powf(1.0 / power)
}
pub struct Compressor {
    space: ColorSpace,
    m1: Matrix,
    m2: Matrix,
    im1: Matrix,
    im2: Matrix,
    cmax: Arc<Vec<f64>>,
    spec: OutputGamut,
    jzazbz: crate::appearance::Jzazbz,
    cam16: crate::appearance::Cam16,
    lightness_bounds: [f64; 2],
    lightness_white: f64,
}
type GamutKey = (String, String, Vec<u64>);
type GamutTables = Mutex<BTreeMap<GamutKey, Arc<Vec<f64>>>>;
static GAMUT_TABLES: OnceLock<GamutTables> = OnceLock::new();
impl Compressor {
    pub fn new(tables: &Tables, space: &str, spec: &OutputGamut) -> Result<Self> {
        ensure!(
            ["off", "oklch", "oklrab", "aces_rgc", "jzazbz", "cam16ucs"]
                .contains(&spec.algorithm.as_str()),
            "Output compression supports off, oklch, oklrab, aces_rgc, jzazbz and cam16ucs"
        );
        let mut result = Self {
            space: tables.space(space)?.clone(),
            m1: tables.oklab_m1,
            m2: tables.oklab_m2,
            im1: inverse(&tables.oklab_m1),
            im2: inverse(&tables.oklab_m2),
            cmax: Arc::new(Vec::new()),
            spec: spec.clone(),
            jzazbz: crate::appearance::Jzazbz::new(tables.jzazbz_m1, tables.jzazbz_m2),
            cam16: crate::appearance::Cam16::new(tables.cat16, tables.space(space)?.white),
            lightness_bounds: match spec.algorithm.as_str() {
                "jzazbz" => [0.002, 0.18],
                "cam16ucs" => [1.0, 110.0],
                _ => [0.02, 1.0],
            },
            lightness_white: 1.0,
        };
        if ["off", "aces_rgc"].contains(&spec.algorithm.as_str()) {
            return Ok(result);
        }
        result.lightness_white = if spec.algorithm == "jzazbz" || spec.algorithm == "cam16ucs" {
            result.xyz_to_lab(xy_xyz(result.space.white))[0]
        } else {
            1.0
        };
        let signature = [
            result.space.to_xyz,
            result.space.from_xyz,
            result.m1,
            result.m2,
            tables.jzazbz_m1,
            tables.jzazbz_m2,
            tables.cat16,
        ]
        .into_iter()
        .flatten()
        .flatten()
        .map(f64::to_bits)
        .collect();
        let key = (space.to_owned(), spec.algorithm.clone(), signature);
        let cache = GAMUT_TABLES.get_or_init(|| Mutex::new(BTreeMap::new()));
        if let Some(table) = cache.lock().unwrap().get(&key) {
            result.cmax = table.clone();
            return Ok(result);
        }
        let table: Vec<_> = (0..64 * 720)
            .into_par_iter()
            .map(|index| {
                let i = index / 720;
                let j = index % 720;
                let [low, high] = result.lightness_bounds;
                let coordinate = low + (high - low) * i as f64 / 63.0;
                let l = if spec.algorithm == "oklrab" {
                    rebased_to_lightness(coordinate)
                } else {
                    coordinate
                };
                let h = -std::f64::consts::PI + 2.0 * std::f64::consts::PI * j as f64 / 720.0;
                let mut lo = 0.0;
                let mut hi = match spec.algorithm.as_str() {
                    "jzazbz" => 0.3,
                    "cam16ucs" => 150.0,
                    _ => 0.5,
                };
                for _ in 0..18 {
                    let mid = (lo + hi) * 0.5;
                    let xyz = result.lab_to_xyz([l, mid * h.cos(), mid * h.sin()]);
                    let rgb = mat_vec(&result.space.from_xyz, xyz);
                    if rgb.iter().all(|v| *v >= -1e-6 && *v <= 1.0 + 1e-6) {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                lo
            })
            .collect();
        result.cmax = Arc::new(table);
        cache.lock().unwrap().insert(key, result.cmax.clone());
        Ok(result)
    }
    fn lab_to_xyz(&self, lab: [f64; 3]) -> [f64; 3] {
        match self.spec.algorithm.as_str() {
            "jzazbz" => self.jzazbz.backward(lab),
            "cam16ucs" => self.cam16.backward(lab),
            _ => mat_vec(&self.im1, mat_vec(&self.im2, lab).map(|v| v.powi(3))),
        }
    }
    fn xyz_to_lab(&self, xyz: [f64; 3]) -> [f64; 3] {
        match self.spec.algorithm.as_str() {
            "jzazbz" => self.jzazbz.forward(xyz),
            "cam16ucs" => self.cam16.forward(xyz),
            _ => mat_vec(&self.m2, mat_vec(&self.m1, xyz).map(f64::cbrt)),
        }
    }
    pub fn compress(&self, rgb: [f64; 3]) -> [f64; 3] {
        if self.spec.algorithm == "off" {
            return rgb;
        }
        if self.spec.algorithm == "aces_rgc" {
            let ach = rgb.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            return if ach > 1e-12 {
                rgb.map(|v| ach * (1.0 - knee((ach - v) / ach, self.spec.knee)))
            } else {
                rgb
            };
        }
        let xyz = mat_vec(&self.space.to_xyz, rgb);
        let lab = self.xyz_to_lab(xyz);
        let l = self.spec.lightness_compression.map_or(lab[0], |k| {
            knee(lab[0] / self.lightness_white, k) * self.lightness_white
        });
        let c = lab[1].hypot(lab[2]);
        let h = lab[2].atan2(lab[1]);
        let coordinate = if self.spec.algorithm == "oklrab" {
            lightness_to_rebased(l)
        } else {
            l
        };
        let [low, high] = self.lightness_bounds;
        let li = (coordinate.clamp(low, high) - low) / (high - low) * 63.0;
        let lo = (li.floor() as usize).min(62);
        let lf = li - lo as f64;
        let hstep = 2.0 * std::f64::consts::PI / 720.0;
        let hj = (h + std::f64::consts::PI) / hstep;
        let hfloor = hj.floor();
        let j = (hfloor as i64).rem_euclid(720) as usize;
        let jf = hj - hfloor;
        let next = (j + 1) % 720;
        let maxc = (self.cmax[lo * 720 + j] * (1.0 - lf) * (1.0 - jf)
            + self.cmax[lo * 720 + next] * (1.0 - lf) * jf
            + self.cmax[(lo + 1) * 720 + j] * lf * (1.0 - jf)
            + self.cmax[(lo + 1) * 720 + next] * lf * jf)
            .max(1e-9);
        let nc = knee(c / maxc, self.spec.knee) * maxc;
        mat_vec(
            &self.space.from_xyz,
            self.lab_to_xyz([l, nc * h.cos(), nc * h.sin()]),
        )
    }
}
fn lightness_to_rebased(l: f64) -> f64 {
    let k3 = (1.0 + 0.206) / (1.0 + 0.03);
    let t = k3 * l - 0.206;
    0.5 * (t + (t * t + 4.0 * 0.03 * k3 * l).sqrt())
}
fn rebased_to_lightness(lr: f64) -> f64 {
    let k3 = (1.0 + 0.206) / (1.0 + 0.03);
    lr * (lr + 0.206) / (k3 * (lr + 0.03))
}
