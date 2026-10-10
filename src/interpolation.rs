/// np.interp's endpoint and right-biased duplicate semantics.
pub fn linear(x: f64, axis: &[f64], values: &[f64]) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < axis[0] {
        return values[0];
    }
    if x >= axis[axis.len() - 1] {
        return values[values.len() - 1];
    }
    let hi = axis.partition_point(|v| *v <= x);
    if hi == 0 {
        return values[0];
    }
    let lo = hi - 1;
    values[lo] + (x - axis[lo]) / (axis[hi] - axis[lo]) * (values[hi] - values[lo])
}

pub fn mitchell(t: f64) -> f64 {
    let x = t.abs();
    let b = 1.0 / 3.0;
    let c = 1.0 / 3.0;
    if x < 1.0 {
        ((12.0 - 9.0 * b - 6.0 * c) * x.powi(3)
            + (-18.0 + 12.0 * b + 6.0 * c) * x.powi(2)
            + (6.0 - 2.0 * b))
            / 6.0
    } else if x < 2.0 {
        ((-b - 6.0 * c) * x.powi(3)
            + (6.0 * b + 30.0 * c) * x.powi(2)
            + (-12.0 * b - 48.0 * c) * x
            + (8.0 * b + 24.0 * c))
            / 6.0
    } else {
        0.0
    }
}

fn reflect(i: isize, size: usize) -> usize {
    if i < 0 {
        (-i) as usize
    } else if i >= size as isize {
        (2 * (size - 1) as isize - i) as usize
    } else {
        i as usize
    }
}

#[derive(Clone)]
pub struct Lut2 {
    pub size: usize,
    pub channels: usize,
    pub data: Vec<f64>,
}
impl Lut2 {
    pub fn cubic(&self, tc: [f64; 2]) -> Vec<f64> {
        let mut out = vec![0.0; self.channels];
        self.cubic_into(tc, &mut out);
        out
    }
    pub fn cubic_into(&self, tc: [f64; 2], out: &mut [f64]) {
        out.fill(0.0);
        let coord = tc.map(|v| (v * (self.size - 1) as f64).clamp(0.0, (self.size - 1) as f64));
        let base = coord.map(|v| (v.floor() as usize).min(self.size - 2));
        let wx = std::array::from_fn::<_, 4, _>(|i| {
            mitchell(coord[0] - base[0] as f64 + 1.0 - i as f64)
        });
        let wy = std::array::from_fn::<_, 4, _>(|i| {
            mitchell(coord[1] - base[1] as f64 + 1.0 - i as f64)
        });
        let mut sum = 0.0;
        for (i, xweight) in wx.iter().enumerate() {
            let x = reflect(base[0] as isize - 1 + i as isize, self.size);
            for (j, yweight) in wy.iter().enumerate() {
                let y = reflect(base[1] as isize - 1 + j as isize, self.size);
                let weight = xweight * yweight;
                sum += weight;
                let offset = (x * self.size + y) * self.channels;
                for (c, v) in out.iter_mut().enumerate() {
                    *v += weight * self.data[offset + c];
                }
            }
        }
        for v in out {
            *v /= sum;
        }
    }
    pub fn bilinear_into(&self, tc: [f64; 2], out: &mut [f64]) {
        let p = tc.map(|v| (v * (self.size - 1) as f64).clamp(0.0, (self.size - 1) as f64));
        let lo = p.map(|v| (v.floor() as usize).min(self.size - 2));
        let f = [p[0] - lo[0] as f64, p[1] - lo[1] as f64];
        for (c, v) in out.iter_mut().enumerate() {
            let at = |x, y| self.data[(x * self.size + y) * self.channels + c];
            *v = at(lo[0], lo[1]) * (1.0 - f[0]) * (1.0 - f[1])
                + at(lo[0] + 1, lo[1]) * f[0] * (1.0 - f[1])
                + at(lo[0], lo[1] + 1) * (1.0 - f[0]) * f[1]
                + at(lo[0] + 1, lo[1] + 1) * f[0] * f[1];
        }
    }
}

pub fn tri_to_quad(xy: [f64; 2]) -> [f64; 2] {
    [
        (1.0 - xy[0]).powi(2).clamp(0.0, 1.0),
        (xy[1] / (1.0 - xy[0]).max(1e-10)).clamp(0.0, 1.0),
    ]
}
