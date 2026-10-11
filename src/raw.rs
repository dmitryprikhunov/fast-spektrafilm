//! Temperature/tint adjustment in linear ACES, matching the RAW front-end.
use crate::{
    Image,
    color::{Tables, adaptation, mat_mul, mat_vec, xy_xyz},
    params::RawSettings,
};
use anyhow::{Result, ensure};
pub fn whitepoint(t: f64) -> Result<[f64; 2]> {
    ensure!(
        t.is_finite() && (1667.0..=25000.).contains(&t),
        "RAW temperature must be 1667–25000 K"
    );
    if t >= 4000. {
        let x = if t <= 7000. {
            -4.607e9 / t.powi(3) + 2.9678e6 / t.powi(2) + 99.11 / t + 0.244063
        } else {
            -2.0064e9 / t.powi(3) + 1.9018e6 / t.powi(2) + 247.48 / t + 0.237040
        };
        Ok([x, -3. * x * x + 2.87 * x - 0.275])
    } else {
        let x = -0.2661239e9 / t.powi(3) - 0.2343589e6 / t.powi(2) + 0.8776956e3 / t + 0.179910;
        let y = if t <= 2222. {
            -1.1063814 * x.powi(3) - 1.34811020 * x * x + 2.18555832 * x - 0.20219683
        } else {
            -0.9549476 * x.powi(3) - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
        };
        Ok([x, y])
    }
}
pub fn validate(p: &RawSettings) -> Result<()> {
    ensure!(
        ["as_shot", "daylight", "tungsten", "custom"].contains(&p.white_balance.as_str()),
        "Unknown RAW white balance"
    );
    ensure!(
        p.tint.is_finite() && p.tint > 0.,
        "RAW tint must be positive and finite"
    );
    if p.white_balance == "custom" {
        whitepoint(p.temperature)?;
    }
    Ok(())
}
pub fn adjust(image: &mut Image, tables: &Tables, p: &RawSettings) -> Result<()> {
    validate(p)?;
    let temperature = match p.white_balance.as_str() {
        "tungsten" => 2850.,
        "custom" => p.temperature,
        _ => return Ok(()),
    };
    let source = whitepoint(temperature)?;
    let target = whitepoint(6504.)?;
    let adapt = xy_xyz(source)
        .iter()
        .zip(xy_xyz(target))
        .any(|(a, b)| (*a - b).abs() > 1e-8 + 1e-5 * b.abs());
    let cs = tables.space("ACES2065-1")?;
    let matrix = mat_mul(
        &cs.from_xyz,
        &mat_mul(&adaptation(&tables.cat02, source, target), &cs.to_xyz),
    );
    let tint = if p.white_balance == "custom" && (p.tint - 1.).abs() > 1e-8 + 1e-5 {
        p.tint as f32
    } else {
        1.
    };
    for pixel in image.data.chunks_exact_mut(3) {
        if adapt {
            let v = mat_vec(&matrix, [pixel[0], pixel[1], pixel[2]]);
            for c in 0..3 {
                pixel[c] = v[c] as f32 as f64;
            }
        }
        if tint != 1. {
            pixel[1] = (pixel[1] as f32 * tint) as f64;
        }
    }
    Ok(())
}
