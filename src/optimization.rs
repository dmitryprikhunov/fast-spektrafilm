//! SciPy 1.18.1 MINPACK LMDER arithmetic, with its default two-point Jacobian.
//! Rust translation of scipy/optimize/src/minpack.c (licenses/scipy.txt).
//! Copyright (c) 2001-2002 Enthought, Inc. 2003, SciPy Developers.
//! Preserve expression and reduction order: calibration is sensitive to ULPs.
use anyhow::{Result, ensure};

fn norm(x: &[f64]) -> f64 {
    let giant = 2f64.powf(63.5);
    let dwarf = 0.5 / giant;
    let agiant = giant / x.len() as f64;
    let (mut s1, mut s2, mut s3, mut xmax, mut xmin) = (0., 0., 0., 0., 0.);
    for &x in x {
        let a = x.abs();
        if a <= dwarf || a >= agiant {
            if a > dwarf {
                if a > xmax {
                    s1 = 1. + s1 * (xmax / a).powi(2);
                    xmax = a;
                } else {
                    s1 += (a / xmax).powi(2);
                }
            } else if a > xmin {
                s3 = 1. + s3 * (xmin / a).powi(2);
                xmin = a;
            } else if a != 0. {
                s3 += (a / xmin).powi(2);
            }
        } else {
            s2 += a * a;
        }
    }
    if s1 != 0. {
        xmax * (s1 + (s2 / xmax) / xmax).sqrt()
    } else if s2 != 0. {
        if s2 >= xmin {
            (s2 * (1. + (xmin / s2) * (xmin * s3))).sqrt()
        } else {
            (xmin * (s2 / xmin + xmin * s3)).sqrt()
        }
    } else {
        xmin * s3.sqrt()
    }
}

// Column-major pivoted Householder QR. Returns diagonal, original norms, pivots.
fn qr(a: &mut [f64], m: usize, n: usize) -> (Vec<f64>, Vec<f64>, Vec<usize>) {
    let ac: Vec<_> = (0..n).map(|j| norm(&a[j * m..(j + 1) * m])).collect();
    let mut rd = ac.clone();
    let mut wa = ac.clone();
    let mut piv: Vec<_> = (0..n).collect();
    for j in 0..n {
        let mut kmax = j;
        for k in j..n {
            if rd[k] > rd[kmax] {
                kmax = k;
            }
        }
        if kmax != j {
            for i in 0..m {
                a.swap(i + m * j, i + m * kmax);
            }
            rd[kmax] = rd[j];
            wa[kmax] = wa[j];
            piv.swap(j, kmax);
        }
        let mut ajnorm = norm(&a[m * j + j..m * (j + 1)]);
        if ajnorm == 0. {
            rd[j] = -ajnorm;
            continue;
        }
        if a[m * j + j] < 0. {
            ajnorm = -ajnorm;
        }
        for i in j..m {
            a[i + m * j] /= ajnorm;
        }
        a[j + m * j] += 1.;
        for k in j + 1..n {
            let mut sum = 0.;
            for i in j..m {
                sum += a[i + m * j] * a[i + m * k];
            }
            let temp = sum / a[j + m * j];
            for i in j..m {
                a[i + m * k] -= temp * a[i + m * j];
            }
            if rd[k] != 0. {
                let temp = a[j + m * k] / rd[k];
                rd[k] *= (1. - temp * temp).max(0.).sqrt();
                if 0.05 * (rd[k] / wa[k]).powi(2) <= f64::EPSILON {
                    rd[k] = norm(&a[(j + 1) + m * k..m * (k + 1)]);
                    wa[k] = rd[k];
                }
            }
        }
        rd[j] = -ajnorm;
    }
    (rd, ac, piv)
}

// Eliminate sqrt(par)*D by Givens rotations, preserving the upper R triangle.
fn qrsolve(
    r: &mut [f64],
    ld: usize,
    piv: &[usize],
    diag: &[f64],
    qtb: &[f64],
) -> (Vec<f64>, Vec<f64>) {
    let n = piv.len();
    let mut x = vec![0.; n];
    let mut sd = vec![0.; n];
    let mut wa = qtb.to_vec();
    for j in 0..n {
        for i in j..n {
            r[i + j * ld] = r[j + i * ld];
        }
        x[j] = r[j + j * ld];
    }
    for j in 0..n {
        if diag[piv[j]] != 0. {
            sd[j..].fill(0.);
            sd[j] = diag[piv[j]];
            let mut q = 0.;
            for k in j..n {
                if sd[k] == 0. {
                    continue;
                }
                let (sin, cos) = if r[k + ld * k].abs() < sd[k].abs() {
                    let cot = r[k + ld * k] / sd[k];
                    let sin = 0.5 / (0.25 + 0.25 * cot.powi(2)).sqrt();
                    (sin, sin * cot)
                } else {
                    let tan = sd[k] / r[k + ld * k];
                    let cos = 0.5 / (0.25 + 0.25 * tan.powi(2)).sqrt();
                    (cos * tan, cos)
                };
                r[k + ld * k] *= cos;
                r[k + ld * k] += sin * sd[k];
                let temp = cos * wa[k] + sin * q;
                q = -sin * wa[k] + cos * q;
                wa[k] = temp;
                for i in k + 1..n {
                    let temp = cos * r[i + ld * k] + sin * sd[i];
                    sd[i] = -sin * r[i + ld * k] + cos * sd[i];
                    r[i + ld * k] = temp;
                }
            }
        }
        sd[j] = r[j + ld * j];
        r[j + ld * j] = x[j];
    }
    let nsing = sd.iter().position(|v| *v == 0.).unwrap_or(n);
    wa[nsing..].fill(0.);
    for j in (0..nsing).rev() {
        let mut sum = 0.;
        for i in j + 1..nsing {
            sum += r[i + j * ld] * wa[i];
        }
        wa[j] = (wa[j] - sum) / sd[j];
    }
    for j in 0..n {
        x[piv[j]] = wa[j];
    }
    (x, sd)
}

fn lmpar(
    r: &mut [f64],
    ld: usize,
    piv: &[usize],
    diag: &[f64],
    qtb: &[f64],
    delta: f64,
    par: &mut f64,
) -> Vec<f64> {
    let n = piv.len();
    let mut wa1 = qtb.to_vec();
    let mut x = vec![0.; n];
    let nsing = (0..n).find(|&j| r[j + j * ld] == 0.).unwrap_or(n);
    wa1[nsing..].fill(0.);
    for j in (0..nsing).rev() {
        wa1[j] /= r[j + j * ld];
        let temp = wa1[j];
        for i in 0..j {
            wa1[i] -= r[i + j * ld] * temp;
        }
    }
    for j in 0..n {
        x[piv[j]] = wa1[j];
    }
    let mut wa2: Vec<_> = (0..n).map(|j| diag[j] * x[j]).collect();
    let mut dxnorm = norm(&wa2);
    let mut fp = dxnorm - delta;
    if fp <= 0.1 * delta {
        *par = 0.;
        return x;
    }
    let mut parl = 0.;
    if nsing == n {
        for j in 0..n {
            let l = piv[j];
            wa1[j] = diag[l] * (wa2[l] / dxnorm);
        }
        for j in 0..n {
            let mut sum = 0.;
            for i in 0..j {
                sum += r[i + j * ld] * wa1[i];
            }
            wa1[j] = (wa1[j] - sum) / r[j + j * ld];
        }
        let temp = norm(&wa1);
        parl = ((fp / delta) / temp) / temp;
    }
    for j in 0..n {
        let mut sum = 0.;
        for i in 0..=j {
            sum += r[i + j * ld] * qtb[i];
        }
        wa1[j] = sum / diag[piv[j]];
    }
    let gnorm = norm(&wa1);
    let mut paru = gnorm / delta;
    if paru == 0. {
        paru = f64::MIN_POSITIVE / delta.min(0.1);
    }
    *par = par.max(parl).min(paru);
    if *par == 0. {
        *par = gnorm / dxnorm;
    }
    for iter in 1..=10 {
        if *par == 0. {
            *par = f64::MIN_POSITIVE.max(paru * 0.001);
        }
        let temp = par.sqrt();
        for j in 0..n {
            wa1[j] = temp * diag[j];
        }
        let (xx, sd) = qrsolve(r, ld, piv, &wa1, qtb);
        x = xx;
        for j in 0..n {
            wa2[j] = diag[j] * x[j];
        }
        dxnorm = norm(&wa2);
        let temp = fp;
        fp = dxnorm - delta;
        if fp.abs() <= 0.1 * delta || (parl == 0. && fp <= temp && temp < 0.) || iter == 10 {
            return x;
        }
        for j in 0..n {
            let l = piv[j];
            wa1[j] = diag[l] * (wa2[l] / dxnorm);
        }
        for j in 0..n {
            wa1[j] /= sd[j];
            let temp = wa1[j];
            for i in j + 1..n {
                wa1[i] -= r[i + j * ld] * temp;
            }
        }
        let temp = norm(&wa1);
        let correction = ((fp / delta) / temp) / temp;
        if fp > 0. {
            parl = parl.max(*par);
        }
        if fp < 0. {
            paru = paru.min(*par);
        }
        *par = parl.max(*par + correction);
    }
    x
}

pub(crate) fn least_squares(
    mut x: Vec<f64>,
    residual: impl Fn(&[f64]) -> Vec<f64>,
    max_nfev: usize,
) -> Result<Vec<f64>> {
    let n = x.len();
    let mut f = residual(&x);
    let m = f.len();
    ensure!(
        n > 0 && m >= n && max_nfev > 0 && f.iter().all(|v| v.is_finite()),
        "Invalid calibration residual"
    );
    let mut fnorm = norm(&f);
    let mut par = 0.;
    let mut iter = 1;
    let mut nfev = 1;
    let mut delta = 0.;
    let mut xnorm = 0.;
    let mut diag = vec![0.; n];
    let mut wa3 = vec![0.; n];
    loop {
        let mut a = vec![0.; m * n];
        for j in 0..n {
            let mut trial = x.clone();
            let h = f64::EPSILON.sqrt() * if x[j] < 0. { -1. } else { 1. } * x[j].abs().max(1.);
            trial[j] += h;
            let h = trial[j] - x[j];
            let v = residual(&trial);
            ensure!(
                v.len() == m && v.iter().all(|v| v.is_finite()),
                "Nonfinite calibration Jacobian"
            );
            for i in 0..m {
                a[i + m * j] = (v[i] - f[i]) / h;
            }
        }
        let (rd, ac, piv) = qr(&mut a, m, n);
        if iter == 1 {
            for j in 0..n {
                diag[j] = if ac[j] == 0. { 1. } else { ac[j] };
                wa3[j] = diag[j] * x[j];
            }
            xnorm = norm(&wa3);
            delta = 100. * xnorm;
            if delta == 0. {
                delta = 100.;
            }
        }
        let mut wa4 = f.clone();
        let mut qtf = vec![0.; n];
        for j in 0..n {
            if a[j + m * j] != 0. {
                let mut sum = 0.;
                for i in j..m {
                    sum += a[i + m * j] * wa4[i];
                }
                let temp = -sum / a[j + m * j];
                for i in j..m {
                    wa4[i] += a[i + m * j] * temp;
                }
            }
            a[j + m * j] = rd[j];
            qtf[j] = wa4[j];
        }
        let mut gnorm: f64 = 0.;
        if fnorm != 0. {
            for j in 0..n {
                let l = piv[j];
                if ac[l] == 0. {
                    continue;
                }
                let mut sum = 0.;
                for i in 0..=j {
                    sum += a[i + m * j] * (qtf[i] / fnorm);
                }
                gnorm = gnorm.max((sum / ac[l]).abs());
            }
        }
        if gnorm <= 1e-8 {
            break;
        }
        for j in 0..n {
            diag[j] = diag[j].max(ac[j]);
        }
        loop {
            let mut step = lmpar(&mut a, m, &piv, &diag, &qtf, delta, &mut par);
            let mut trial = vec![0.; n];
            for j in 0..n {
                step[j] = -step[j];
                trial[j] = x[j] + step[j];
                wa3[j] = diag[j] * step[j];
            }
            let pnorm = norm(&wa3);
            if iter == 1 {
                delta = delta.min(pnorm);
            }
            let new_f = residual(&trial);
            nfev += 1;
            ensure!(
                new_f.len() == m && new_f.iter().all(|v| v.is_finite()),
                "Calibration diverged"
            );
            let fnorm1 = norm(&new_f);
            let actred = if 0.1 * fnorm1 < fnorm {
                1. - (fnorm1 / fnorm).powi(2)
            } else {
                -1.
            };
            for j in 0..n {
                wa3[j] = 0.;
                let temp = step[piv[j]];
                for i in 0..=j {
                    wa3[i] += a[i + m * j] * temp;
                }
            }
            let temp1 = norm(&wa3) / fnorm;
            let temp2 = (par.sqrt() * pnorm) / fnorm;
            let prered = temp1 * temp1 + temp2 * temp2 / 0.5;
            let dirder = -(temp1 * temp1 + temp2 * temp2);
            let ratio = if prered != 0. { actred / prered } else { 0. };
            if ratio <= 0.25 {
                let mut temp = if actred < 0. {
                    (0.5 * dirder) / (dirder + 0.5 * actred)
                } else {
                    0.5
                };
                if 0.1 * fnorm1 >= fnorm || temp < 0.1 {
                    temp = 0.1;
                }
                delta = temp * delta.min(pnorm / 0.1);
                par /= temp;
            } else if par == 0. || ratio >= 0.75 {
                delta = pnorm / 0.5;
                par *= 0.5;
            }
            if ratio >= 0.0001 {
                x = trial;
                f = new_f;
                for j in 0..n {
                    wa3[j] = diag[j] * x[j];
                }
                xnorm = norm(&wa3);
                fnorm = fnorm1;
                iter += 1;
            }
            let converged = actred.abs() <= 1e-8 && prered <= 1e-8 && 0.5 * ratio <= 1.;
            if converged
                || delta <= 1e-8 * xnorm
                || nfev >= max_nfev
                || (actred.abs() <= f64::EPSILON && prered <= f64::EPSILON && 0.5 * ratio <= 1.)
                || delta <= f64::EPSILON * xnorm
                || gnorm <= f64::EPSILON
            {
                ensure!(
                    x.iter().all(|v| v.is_finite()),
                    "Nonfinite fitted parameters"
                );
                return Ok(x);
            }
            if ratio >= 0.0001 {
                break;
            }
        }
    }
    Ok(x)
}
