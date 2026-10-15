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

#[test]
fn numerical_contract_rejects_perturbation_and_invalid_arrays() -> anyhow::Result<()> {
    use fast_spektrafilm::{Image, numerical};
    let reference = Image::new(1, 1, 3, vec![0., -1e-12, 4.])?;
    let mut actual = reference.clone();
    actual.data[1] += 1e-6;
    let report = numerical::compare(&actual, &reference, numerical::contract("strict"))?;
    assert_eq!(report.failures, 1);
    assert_eq!(report.worst_index, [0, 0, 1]);
    actual.data[0] = f64::NAN;
    assert!(numerical::compare(&actual, &reference, numerical::contract("strict")).is_err());
    actual = reference.clone();
    actual.width = 2;
    assert!(numerical::compare(&actual, &reference, numerical::contract("strict")).is_err());
    Ok(())
}

#[test]
fn invalid_numeric_settings_and_nonfinite_outputs_are_errors() -> anyhow::Result<()> {
    use fast_spektrafilm::{Image, Params, Simulator, Tap};
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    for field in ["camera", "scanner", "optional"] {
        let mut p = Params::default();
        match field {
            "camera" => p.camera.exposure_compensation_ev = f64::NAN,
            "scanner" => p.scanner.lens_blur = f64::INFINITY,
            _ => p.film_chemistry.development_time = Some(f64::NAN),
        }
        assert!(Simulator::new(&data, p).is_err());
    }
    let mut p = Params::default();
    p.workflow.route = "input".into();
    p.io.input_cctf_decoding = true;
    p.io.input_color_space = "ProPhoto RGB".into();
    let sim = Simulator::new(&data, p)?;
    let image = Image::new(1, 1, 3, vec![f64::MAX; 3])?;
    assert!(sim.process(&image, Tap::RgbOut).is_err());
    assert!(
        sim.process_profiled(image, Tap::RgbIn, Tap::RgbOut, 1.)
            .is_err()
    );
    Ok(())
}

#[test]
fn cli_repetition_series_use_one_executor_and_explicit_warmups() -> anyhow::Result<()> {
    let scratch = Scratch::new("timing-series")?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config = scratch.0.join("config.json");
    std::fs::write(
        &config,
        serde_json::to_vec(&json!({
            "camera":{"auto_exposure":false},"io":{"copy_metadata":false},
            "debug":{"deactivate_spatial_effects":true,"deactivate_stochastic_effects":true}
        }))?,
    )?;
    for profiled in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fast-spektrafilm"));
        command
            .args(["--threads", "2", "process"])
            .arg(root.join("tests/fixtures/review/blur_0/input.npy"))
            .arg(scratch.0.join("output.npy"))
            .arg("--config")
            .arg(&config)
            .args(["--repeat", "3", "--warmup", "1"]);
        if profiled {
            command.arg("--stage-timings");
        }
        let result = command.output()?;
        anyhow::ensure!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let report: Value = serde_json::from_str(
            String::from_utf8_lossy(&result.stderr)
                .lines()
                .last()
                .unwrap(),
        )?;
        assert_eq!(
            report["executor"],
            if profiled { "profiled" } else { "fused" }
        );
        assert_eq!(report["warmup_seconds"].as_array().unwrap().len(), 1);
        let times = report["process_seconds"].as_array().unwrap();
        let stages = report["stage_samples_seconds"].as_array().unwrap();
        assert_eq!(times.len(), 3);
        assert_eq!(stages.len(), 3);
        for (seconds, stages) in times.iter().zip(stages) {
            if profiled {
                let stages = stages.as_object().unwrap();
                assert_eq!(stages.len(), 6);
                assert!(
                    stages.values().map(|v| v.as_f64().unwrap()).sum::<f64>()
                        <= seconds.as_f64().unwrap()
                );
            } else {
                assert!(stages.is_null());
            }
        }
    }
    Ok(())
}
