use fast_spektrafilm::color::decode;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command, time::SystemTime};

struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> anyhow::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "fast-spektrafilm-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn bake(&self, name: &str, config: Value) -> anyhow::Result<Vec<[f64; 3]>> {
        let config_path = self.0.join(format!("{name}.json"));
        let output_path = self.0.join(format!("{name}.cube"));
        std::fs::write(&config_path, serde_json::to_vec(&config)?)?;
        let result = Command::new(env!("CARGO_BIN_EXE_fast-spektrafilm"))
            .args(["--threads", "4", "--data-dir"])
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data"))
            .arg("bake")
            .arg(&output_path)
            .arg("--config")
            .arg(config_path)
            .args(["--resolution", "5"])
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let text = std::fs::read_to_string(output_path)?;
        anyhow::ensure!(text.lines().any(|line| line == "LUT_3D_SIZE 5"));
        let mut samples = Vec::new();
        for line in text.lines().skip(4) {
            let values = line
                .split_whitespace()
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()?;
            anyhow::ensure!(values.len() == 3 && values.iter().all(|v| v.is_finite()));
            samples.push([values[0], values[1], values[2]]);
        }
        Ok(samples)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn bake_preserves_cube_domain_with_framing_config() -> anyhow::Result<()> {
    let scratch = Scratch::new("bake-framing")?;
    let expected = scratch.bake("default", json!({}))?;
    assert_eq!(expected.len(), 125);
    for (name, config) in [
        (
            "crop",
            json!({"io": {"crop": true, "crop_size": [0.5, 0.5]}}),
        ),
        ("resize", json!({"io": {"upscale_factor": 2.0}})),
    ] {
        let actual = scratch.bake(name, config)?;
        assert_eq!(
            actual.len(),
            125,
            "{name}: sample count must match LUT_3D_SIZE"
        );
        assert_eq!(
            actual, expected,
            "{name}: framing must not deform cube samples"
        );
    }
    Ok(())
}

#[test]
fn bake_uses_exact_spectral_compute_with_acceleration_config() -> anyhow::Result<()> {
    let scratch = Scratch::new("bake-acceleration")?;
    let expected = scratch.bake("default", json!({}))?;
    for field in ["use_enlarger_lut", "use_scanner_lut"] {
        let mut config = json!({"settings": {"lut_resolution": 9}});
        config["settings"][field] = json!(true);
        let actual = scratch.bake(field, config)?;
        assert_eq!(
            actual, expected,
            "{field}: acceleration must not alter the baked transform"
        );
    }
    Ok(())
}

#[test]
fn bt2020_decode_matches_reference_near_toe() {
    // Golden values from colour-science 0.4.7 in the pinned reference environment.
    // Include both sides of its inverse-OETF breakpoint, not just a broad ramp.
    for (encoded, expected) in [
        (0.0, 0.0),
        (0.08, 0.017777777777777778),
        (0.081, 0.018000000000000002),
        (0.0811, 0.018022222222222224),
        (0.0812, 0.018044444444444443),
        (0.08124, 0.018053333333333334),
        (0.08125, 0.018000456255787358),
        (0.1, 0.02242795841579486),
        (1.0, 1.0),
    ] {
        let actual = decode("ITU-R BT.2020", encoded);
        assert!(
            (actual - expected).abs() < 1e-14,
            "BT.2020 decode({encoded}): {actual} != {expected}"
        );
    }
}
