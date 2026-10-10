//! NumPy RandomState-compatible MT19937 and legacy distributions.
//! Distribution routines are translated from NumPy 2.5.3's BSD-licensed
//! random/src/{distributions,legacy}; see licenses/numpy-LICENSE.
// Preserve the source constants to make the distribution port auditable.
#![allow(clippy::excessive_precision)]
pub struct RandomState {
    state: [u32; 624],
    position: usize,
    normal_cache: Option<f64>,
}
impl RandomState {
    pub fn new(seed: u32) -> Self {
        let mut state = [0; 624];
        state[0] = seed;
        for i in 1..624 {
            state[i] = 1812433253u32
                .wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        Self {
            state,
            position: 624,
            normal_cache: None,
        }
    }
    fn next_u32(&mut self) -> u32 {
        if self.position == 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x80000000) | (self.state[(i + 1) % 624] & 0x7fffffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.position = 0;
        }
        let mut y = self.state[self.position];
        self.position += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c5680;
        y ^= (y << 15) & 0xefc60000;
        y ^= y >> 18;
        y
    }
    pub fn uniform(&mut self) -> f64 {
        let a = self.next_u32() >> 5;
        let b = self.next_u32() >> 6;
        ((a as f64) * 67108864.0 + b as f64) / 9007199254740992.0
    }
    pub fn normal(&mut self) -> f64 {
        if let Some(v) = self.normal_cache.take() {
            return v;
        }
        loop {
            let x1 = 2.0 * self.uniform() - 1.0;
            let x2 = 2.0 * self.uniform() - 1.0;
            let r = x1 * x1 + x2 * x2;
            if r >= 1.0 || r == 0.0 {
                continue;
            }
            let f = (-2.0 * r.ln() / r).sqrt();
            self.normal_cache = Some(f * x1);
            return f * x2;
        }
    }
    pub fn fast_poisson(&mut self, lambda: f64) -> i64 {
        if lambda <= 0.0 {
            0
        } else if lambda < 30.0 {
            let limit = (-lambda).exp();
            let mut product = 1.0;
            let mut k = 0;
            while product > limit {
                k += 1;
                product *= self.uniform();
            }
            k - 1
        } else {
            (lambda + lambda.sqrt() * self.normal())
                .round_ties_even()
                .max(0.0) as i64
        }
    }
    pub fn fast_binomial(&mut self, n: i64, p: f64) -> i64 {
        if p <= 0.0 {
            return 0;
        }
        if p >= 1.0 {
            return n;
        }
        if n < 25 {
            return (0..n).filter(|_| self.uniform() < p).count() as i64;
        }
        let mean = n as f64 * p;
        let variance = mean * (1.0 - p);
        if variance > 10.0 {
            (mean + variance.sqrt() * self.normal())
                .round_ties_even()
                .clamp(0.0, n as f64) as i64
        } else {
            let u = self.uniform();
            let mut cdf = 0.0;
            let mut probability = (1.0 - p).powf(n as f64);
            // Reference inversion remains identically zero after underflow;
            // its loop then returns n without consuming another random number.
            // Skip those potentially millions of zero-valued iterations.
            if probability == 0.0 {
                return if u > 0.0 { n } else { -1 };
            }
            let mut k = 0;
            while cdf < u && k <= n {
                cdf += probability;
                if k < n {
                    probability *= ((n - k) as f64 / (k + 1) as f64) * (p / (1.0 - p));
                }
                k += 1;
            }
            k - 1
        }
    }
    pub fn poisson(&mut self, lambda: f64) -> i64 {
        if lambda == 0.0 {
            return 0;
        }
        if lambda < 10.0 {
            let limit = (-lambda).exp();
            let mut product = 1.0;
            let mut x = 0;
            loop {
                product *= self.uniform();
                if product > limit {
                    x += 1;
                } else {
                    return x;
                }
            }
        }
        let b = 0.931 + 2.53 * lambda.sqrt();
        let a = -0.059 + 0.02483 * b;
        let inverse_alpha = 1.1239 + 1.1328 / (b - 3.4);
        let vr = 0.9277 - 3.6224 / (b - 2.0);
        loop {
            let u = self.uniform() - 0.5;
            let v = self.uniform();
            let us = 0.5 - u.abs();
            let k = ((2.0 * a / us + b) * u + lambda + 0.43).floor() as i64;
            if us >= 0.07 && v <= vr {
                return k;
            }
            if k < 0 || (us < 0.013 && v > us) {
                continue;
            }
            if v.ln() + inverse_alpha.ln() - (a / (us * us) + b).ln()
                <= -lambda + k as f64 * lambda.ln() - log_gamma(k as f64 + 1.0)
            {
                return k;
            }
        }
    }
    pub fn binomial(&mut self, n: i64, p: f64) -> i64 {
        let q = if p > 0.5 { 1.0 - p } else { p };
        let x = if q * n as f64 <= 30.0 {
            self.binomial_inversion(n, q)
        } else {
            self.binomial_btpe(n, q)
        };
        if p > 0.5 { n - x } else { x }
    }
    fn binomial_inversion(&mut self, n: i64, p: f64) -> i64 {
        let q = 1.0 - p;
        let np = n as f64 * p;
        let qn = (n as f64 * q.ln()).exp();
        let bound = (n as f64).min(np + 10.0 * (np * q + 1.0).sqrt()) as i64;
        let mut x = 0;
        let mut px = qn;
        let mut u = self.uniform();
        while u > px {
            x += 1;
            if x > bound {
                x = 0;
                px = qn;
                u = self.uniform();
            } else {
                u -= px;
                px = ((n - x + 1) as f64 * p * px) / (x as f64 * q);
            }
        }
        x
    }
    fn binomial_btpe(&mut self, n: i64, r: f64) -> i64 {
        let q = 1.0 - r;
        let fm = n as f64 * r + r;
        let m = fm.floor() as i64;
        let p1 = (2.195 * (n as f64 * r * q).sqrt() - 4.6 * q).floor() + 0.5;
        let xm = m as f64 + 0.5;
        let xl = xm - p1;
        let xr = xm + p1;
        let c = 0.134 + 20.5 / (15.3 + m as f64);
        let a = (fm - xl) / (fm - xl * r);
        let laml = a * (1.0 + a / 2.0);
        let a = (xr - fm) / (xr * q);
        let lamr = a * (1.0 + a / 2.0);
        let p2 = p1 * (1.0 + 2.0 * c);
        let p3 = p2 + c / laml;
        let p4 = p3 + c / lamr;
        let nrq = n as f64 * r * q;
        loop {
            let u = self.uniform() * p4;
            let mut v = self.uniform();
            let y;
            if u <= p1 {
                return (xm - p1 * v + u).floor() as i64;
            } else if u <= p2 {
                let x = xl + (u - p1) / c;
                v = v * c + 1.0 - (m as f64 - x + 0.5).abs() / p1;
                if v > 1.0 {
                    continue;
                }
                y = x.floor() as i64;
            } else if u <= p3 {
                y = (xl + v.ln() / laml).floor() as i64;
                if y < 0 || v == 0.0 {
                    continue;
                }
                v *= (u - p2) * laml;
            } else {
                y = (xr - v.ln() / lamr).floor() as i64;
                if y > n || v == 0.0 {
                    continue;
                }
                v *= (u - p3) * lamr;
            }
            let k = (y - m).abs() as f64;
            if k <= 20.0 || k >= nrq / 2.0 - 1.0 {
                let s = r / q;
                let a = s * (n + 1) as f64;
                let mut f = 1.0;
                if m < y {
                    for i in m + 1..=y {
                        f *= a / i as f64 - s;
                    }
                } else if m > y {
                    for i in y + 1..=m {
                        f /= a / i as f64 - s;
                    }
                }
                if v > f {
                    continue;
                }
            } else {
                let rho = (k / nrq) * ((k * (k / 3.0 + 0.625) + 0.16666666666666666) / nrq + 0.5);
                let t = -k * k / (2.0 * nrq);
                let big_a = v.ln();
                if big_a < t - rho {
                    return y;
                }
                if big_a > t + rho {
                    continue;
                }
                let x1 = y as f64 + 1.0;
                let f1 = m as f64 + 1.0;
                let z = n as f64 + 1.0 - m as f64;
                let w = n as f64 - y as f64 + 1.0;
                // Preserve RandomState's legacy correction, including 13680.
                let correction = |v: f64| {
                    let v2 = v * v;
                    (13680.0 - (462.0 - (132.0 - (99.0 - 140.0 / v2) / v2) / v2) / v2)
                        / v
                        / 166320.0
                };
                if big_a
                    > xm * (f1 / x1).ln()
                        + (n as f64 - m as f64 + 0.5) * (z / w).ln()
                        + (y - m) as f64 * (w * r / (x1 * q)).ln()
                        + correction(f1)
                        + correction(z)
                        + correction(x1)
                        + correction(w)
                {
                    continue;
                }
            }
            return y;
        }
    }
}
fn log_gamma(x: f64) -> f64 {
    let a = [
        8.333333333333333e-2,
        -2.777777777777778e-3,
        7.936507936507937e-4,
        -5.952380952380952e-4,
        8.417508417508418e-4,
        -1.917526917526918e-3,
        6.410256410256410e-3,
        -2.955065359477124e-2,
        1.796443723688307e-1,
        -1.39243221690590,
    ];
    if x == 1.0 || x == 2.0 {
        return 0.0;
    }
    let n = if x < 7.0 { (7.0 - x) as i64 } else { 0 };
    let mut x0 = x + n as f64;
    let x2 = (1.0 / x0) * (1.0 / x0);
    let mut gl = a[9];
    for k in (0..9).rev() {
        gl *= x2;
        gl += a[k];
    }
    gl = gl / x0 + 0.5 * 1.8378770664093453 + (x0 - 0.5) * x0.ln() - x0;
    if x < 7.0 {
        for _ in 1..=n {
            gl -= (x0 - 1.0).ln();
            x0 -= 1.0;
        }
    }
    gl
}
