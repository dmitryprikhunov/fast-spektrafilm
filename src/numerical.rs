//! Shared regression contracts; Python tooling reads the same JSON policy.
use crate::Image;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};
#[derive(Clone, Deserialize)]
pub struct Contract {
    pub atol: f64,
    pub rtol: f64,
}
pub fn contract(name: &str) -> &'static Contract {
    static CONTRACTS: OnceLock<BTreeMap<String, Contract>> = OnceLock::new();
    &CONTRACTS
        .get_or_init(|| serde_json::from_str(include_str!("../numerical-contracts.json")).unwrap())
        [name]
}
#[derive(Debug, Serialize)]
pub struct Comparison {
    pub shape: [usize; 3],
    pub finite: bool,
    pub max_abs: f64,
    pub rmse: f64,
    pub failures: usize,
    pub max_tolerance_ratio: f64,
    pub worst_index: [usize; 3],
}
pub fn compare(actual: &Image, reference: &Image, policy: &Contract) -> Result<Comparison> {
    let shape = |i: &Image| [i.height, i.width, i.channels];
    ensure!(
        shape(actual) == shape(reference),
        "Shape mismatch: {:?} != {:?}",
        shape(actual),
        shape(reference)
    );
    actual.validate()?;
    reference.validate()?;
    ensure!(
        policy.atol.is_finite()
            && policy.atol >= 0.
            && policy.rtol.is_finite()
            && policy.rtol >= 0.,
        "Invalid numerical contract"
    );
    let mut report = Comparison {
        shape: shape(actual),
        finite: true,
        max_abs: 0.,
        rmse: 0.,
        failures: 0,
        max_tolerance_ratio: 0.,
        worst_index: [0; 3],
    };
    for (i, (&a, &b)) in actual.data.iter().zip(&reference.data).enumerate() {
        let error = (a - b).abs();
        let tolerance = policy.atol + policy.rtol * b.abs();
        let ratio = if error == 0. { 0. } else { error / tolerance };
        report.max_tolerance_ratio = report.max_tolerance_ratio.max(ratio);
        report.failures += usize::from(error > tolerance);
        if error > report.max_abs {
            report.max_abs = error;
            report.worst_index = [
                i / actual.channels / actual.width,
                i / actual.channels % actual.width,
                i % actual.channels,
            ];
        }
        report.rmse = error.hypot(report.rmse);
    }
    report.rmse /= (actual.data.len() as f64).sqrt();
    Ok(report)
}
