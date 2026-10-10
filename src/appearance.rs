//! Colour-science 0.4.7 appearance transforms used by the reference gamut mapper.
//! Equations ported from colour/models/jzazbz.py and appearance/cam16.py.
//! Original BSD license is preserved in licenses/colour-science-LICENSE.
use crate::color::{Matrix, inverse, mat_vec, xy_xyz};

fn spow(v: f64, p: f64) -> f64 {
    v.signum() * v.abs().powf(p)
}

pub(crate) struct Jzazbz {
    m1: Matrix,
    m2: Matrix,
    im1: Matrix,
    im2: Matrix,
}
impl Jzazbz {
    pub fn new(m1: Matrix, m2: Matrix) -> Self {
        Self {
            m1,
            m2,
            im1: inverse(&m1),
            im2: inverse(&m2),
        }
    }
    pub fn forward(&self, xyz: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = xyz.map(|v| v * 100.0);
        let mut jab = mat_vec(
            &self.m2,
            mat_vec(
                &self.m1,
                [1.15 * x - (1.15 - 1.0) * z, 0.66 * y - (0.66 - 1.0) * x, z],
            )
            .map(|v| {
                let p = spow(v / 10000.0, 2610.0 / 16384.0);
                spow(
                    (3424.0 / 4096.0 + 2413.0 / 128.0 * p) / (2392.0 / 128.0 * p + 1.0),
                    1.7 * 2523.0 / 32.0,
                )
            }),
        );
        jab[0] = (1.0 - 0.56) * jab[0] / (1.0 - 0.56 * jab[0]) - 1.6295499532821566e-11;
        jab
    }
    pub fn backward(&self, mut jab: [f64; 3]) -> [f64; 3] {
        jab[0] = (jab[0] + 1.6295499532821566e-11)
            / (1.0 - 0.56 + 0.56 * (jab[0] + 1.6295499532821566e-11));
        let [xp, yp, z] = mat_vec(
            &self.im1,
            mat_vec(&self.im2, jab).map(|v| {
                let p = spow(v, 1.0 / (1.7 * 2523.0 / 32.0));
                10000.0
                    * spow(
                        (p - 3424.0 / 4096.0).max(0.0) / (2413.0 / 128.0 - 2392.0 / 128.0 * p),
                        16384.0 / 2610.0,
                    )
            }),
        );
        let x = (xp + (1.15 - 1.0) * z) / 1.15;
        let y = (yp + (0.66 - 1.0) * x) / 0.66;
        [x / 100.0, y / 100.0, z / 100.0]
    }
}

pub(crate) struct Cam16 {
    matrix: Matrix,
    inverse: Matrix,
    adaptation: [f64; 3],
    fl: f64,
    nbb: f64,
    z: f64,
    aw: f64,
    chroma_scale: f64,
}
impl Cam16 {
    pub fn new(matrix: Matrix, white: [f64; 2]) -> Self {
        let white = xy_xyz(white).map(|v| v * 100.0);
        let rgbw = mat_vec(&matrix, white);
        let d = (1.0 - 1.0 / 3.6 * ((-64.0_f64 - 42.0) / 92.0).exp()).clamp(0.0, 1.0);
        let k = 1.0_f64 / (5.0 * 64.0 + 1.0);
        let fl = 0.2 * k.powi(4) * 5.0 * 64.0
            + 0.1 * (1.0 - k.powi(4)).powi(2) * (5.0_f64 * 64.0).cbrt();
        let nbb = 0.725 * (1.0_f64 / 0.2).powf(0.2);
        let mut result = Self {
            matrix,
            inverse: inverse(&matrix),
            adaptation: rgbw.map(|v| d * 100.0 / v + 1.0 - d),
            fl,
            nbb,
            z: 1.48 + 0.2_f64.sqrt(),
            aw: 0.0,
            chroma_scale: (1.64 - 0.29_f64.powf(0.2)).powf(0.73),
        };
        let rgbaw = std::array::from_fn(|c| result.response(rgbw[c] * result.adaptation[c]));
        result.aw = result.achromatic(rgbaw);
        result
    }
    fn response(&self, rgb: f64) -> f64 {
        let p = (self.fl * rgb.abs() / 100.0).powf(0.42);
        400.0 * rgb.signum() * p / (27.13 + p) + 0.1
    }
    fn achromatic(&self, [r, g, b]: [f64; 3]) -> f64 {
        (2.0 * r + g + 1.0 / 20.0 * b - 0.305) * self.nbb
    }
    pub fn forward(&self, xyz: [f64; 3]) -> [f64; 3] {
        let rgb = mat_vec(&self.matrix, xyz.map(|v| v * 100.0));
        let [r, g, b] = std::array::from_fn(|c| self.response(rgb[c] * self.adaptation[c]));
        let a = r - 12.0 * g / 11.0 + b / 11.0;
        let bb = (r + g - 2.0 * b) / 9.0;
        let h = bb.atan2(a).rem_euclid(std::f64::consts::TAU);
        let e = 0.25 * ((2.0 + h).cos() + 3.8);
        let j = 100.0 * spow(self.achromatic([r, g, b]) / self.aw, 0.69 * self.z);
        let denom = r + g + 21.0 * b / 20.0;
        let t = if denom == 0.0 {
            0.0
        } else {
            50000.0 / 13.0 * self.nbb * e * (a * a + bb * bb).sqrt() / denom
        };
        let c = spow(t, 0.9) * spow(j / 100.0, 0.5) * self.chroma_scale;
        let m = c * self.fl.powf(0.25);
        let mp = (0.0228 * m).ln_1p() / 0.0228;
        [1.7 * j / (1.0 + 0.007 * j), mp * h.cos(), mp * h.sin()]
    }
    pub fn backward(&self, [jp, ap, bp]: [f64; 3]) -> [f64; 3] {
        let j = -jp / (0.007 * jp - 1.7);
        let mp = ap.hypot(bp);
        let h = bp.atan2(ap).rem_euclid(std::f64::consts::TAU);
        let m = (0.0228 * mp).exp_m1() / 0.0228;
        let c = m / self.fl.powf(0.25);
        let t = spow(
            c / ((j.max(f64::EPSILON) / 100.0).sqrt() * self.chroma_scale),
            1.0 / 0.9,
        );
        let e = 0.25 * ((2.0 + h).cos() + 3.8);
        let aa = self.aw * spow(j / 100.0, 1.0 / (0.69 * self.z));
        let p1 = if t == 0.0 {
            0.0
        } else {
            50000.0 / 13.0 * self.nbb * e / t
        };
        let p2 = aa / self.nbb + 0.305;
        let p3 = 21.0 / 20.0;
        let num = p2 * (2.0 + p3) * (460.0 / 1403.0);
        let (a, b) = if t == 0.0 {
            (0.0, 0.0)
        } else if h.sin().abs() >= h.cos().abs() {
            let ratio = h.cos() / h.sin();
            let b = num
                / (p1 / h.sin() + (2.0 + p3) * (220.0 / 1403.0) * ratio - 27.0 / 1403.0
                    + p3 * (6300.0 / 1403.0));
            (b * ratio, b)
        } else {
            let ratio = h.sin() / h.cos();
            let a = num
                / (p1 / h.cos() + (2.0 + p3) * (220.0 / 1403.0)
                    - (27.0 / 1403.0 - p3 * (6300.0 / 1403.0)) * ratio);
            (a, a * ratio)
        };
        let rgba = [
            (460.0 * p2 + 451.0 * a + 288.0 * b) / 1403.0,
            (460.0 * p2 - 891.0 * a - 261.0 * b) / 1403.0,
            (460.0 * p2 - 220.0 * a - 6300.0 * b) / 1403.0,
        ];
        let rgb = std::array::from_fn(|i| {
            let v = rgba[i] - 0.1;
            v.signum() * 100.0 / self.fl * spow(27.13 * v.abs() / (400.0 - v.abs()), 1.0 / 0.42)
                / self.adaptation[i]
        });
        mat_vec(&self.inverse, rgb).map(|v| v / 100.0)
    }
}
