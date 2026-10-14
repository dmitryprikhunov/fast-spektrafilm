mod common;
use common::compare;
use fast_spektrafilm::{
    Image, Params, Simulator, Tap, color::Tables, file_io, preview, print_balance,
};
use std::path::{Path, PathBuf};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
fn fixture(name: &str) -> Image {
    Image::read_npy(&root().join(format!("tests/fixtures/debt/{name}.npy"))).unwrap()
}
fn deterministic() -> Params {
    let mut p = Params::default();
    p.debug.deactivate_stochastic_effects = true;
    p.io.copy_metadata = false;
    p
}
#[test]
fn calibration_parameters_residuals_and_outputs_match_reference() -> anyhow::Result<()> {
    let cases: serde_json::Value = serde_json::from_reader(std::fs::File::open(
        root().join("tests/fixtures/debt/calibration.json"),
    )?)?;
    let data = root().join("data");
    let tables = Tables::load(&data)?;
    for (i, case) in cases.as_array().unwrap().iter().enumerate() {
        let mut p = deterministic();
        p.debug.deactivate_spatial_effects = true;
        p.camera.auto_exposure = false;
        p.workflow.route = "input > convert-film > print > scan".into();
        p.film_profile = case["stock"].as_str().unwrap().into();
        p.film_base.cyan = case["cmy"][0].as_f64().unwrap();
        p.film_base.magenta = case["cmy"][1].as_f64().unwrap();
        p.film_base.yellow = case["cmy"][2].as_f64().unwrap();
        p.convert.exposure_compensation_ev = case["ev"].as_f64().unwrap();
        let input = fixture(&format!("calibration_{i}_input"));
        let sim = Simulator::new(&data, p.clone())?;
        let converter = fast_spektrafilm::convert::FilmConverter::new(&tables, sim.film(), &p)?;
        let fit = converter.fit_calibration(&input)?;
        let expected: [[f64; 3]; 3] = serde_json::from_value(case["matrix"].clone())?;
        let error = fit
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0., f64::max);
        eprintln!("calibration {i}: coefficient max_abs={error:e}");
        assert!(error < 1e-4, "{i}: {error:e}");
        let residual = converter.calibration_residual(&input, fit)?;
        compare(
            &Image::new(1, residual.len(), 1, residual)?,
            &fixture(&format!("calibration_{i}_residual")),
            1e-5,
            "full residual",
        );
        p.convert.calibration = fit;
        let calibrated = Simulator::new(&data, p.clone())?;
        for tap in [Tap::CmyFilm, Tap::RgbOut] {
            compare(
                &calibrated.process(&input, tap)?,
                &fixture(&format!("calibration_{i}_{}", tap.name())),
                1e-4,
                tap.name(),
            );
        }
        let stock = fast_spektrafilm::profile::Profile::load(&data, &p.film_profile)?;
        let (base, ev) = fast_spektrafilm::convert::fit_base(
            &tables,
            &stock,
            &p,
            fast_spektrafilm::convert::sample_base(&input, 99.)?,
        )?;
        for (key, v) in [
            ("cyan", base.cyan),
            ("magenta", base.magenta),
            ("yellow", base.yellow),
        ] {
            assert!((v - case["base"][key].as_f64().unwrap()).abs() < 1e-5);
        }
        assert!((ev - case["base_ev"].as_f64().unwrap()).abs() < 1e-5);
        assert!(fast_spektrafilm::convert::fit_base(&tables, &stock, &p, [0.; 3]).is_err());
    }
    Ok(())
}
#[test]
fn preview_input_and_stages_match_pinned_reference() -> anyhow::Result<()> {
    let input = preview::resize(&fixture("preview_input"), 37)?;
    compare(&input, &fixture("preview_pre"), 1e-12, "preview resize");
    let mut p = deterministic();
    p.settings.preview_mode = true;
    p.settings.preview_max_size = 37;
    p.camera.film_format_mm = 1.5;
    let sim = Simulator::new(&root().join("data"), p)?;
    for tap in [
        Tap::LogEFilm,
        Tap::CmyFilm,
        Tap::LogEPrint,
        Tap::CmyPrint,
        Tap::RgbOut,
    ] {
        compare(
            &sim.process(&input, tap)?,
            &fixture(&format!("preview_{}", tap.name())),
            1e-9,
            tap.name(),
        );
    }
    assert!(preview::resize(&input, 0).is_err());
    Ok(())
}
#[test]
fn injection_and_profiled_stages_match_reference() -> anyhow::Result<()> {
    let mut p = deterministic();
    p.camera.auto_exposure = false;
    p.camera.film_format_mm = 1.5;
    p.camera.diffusion_filter.active = true;
    p.enlarger.diffusion_filter.active = true;
    let sim = Simulator::new(&root().join("data"), p)?;
    let input = fixture("injection_input");
    let report = sim.process_profiled(input.clone(), Tap::RgbIn, Tap::RgbOut, 1.)?;
    compare(
        &report.image,
        &sim.process(&input, Tap::RgbOut)?,
        1e-9,
        "profiled/fused",
    );
    assert_eq!(report.stages.len(), 6);
    assert!(report.total_seconds >= report.stages.values().sum::<f64>());
    for tap in [Tap::LogEFilm, Tap::CmyFilm, Tap::LogEPrint, Tap::CmyPrint] {
        let stage = fixture(&format!("injection_{}", tap.name()));
        compare(
            &sim.process_from(stage, tap, Tap::RgbOut, 1500. / 9.)?,
            &fixture(&format!("injection_{}_out", tap.name())),
            2e-6,
            tap.name(),
        );
    }
    assert!(
        sim.process_from(input.clone(), Tap::RgbOut, Tap::CmyFilm, 1.)
            .is_err()
    );
    assert!(
        sim.process_from(input, Tap::RgbIn, Tap::RgbOut, 0.)
            .is_err()
    );
    Ok(())
}
#[test]
fn neutral_balance_matches_reference() -> anyhow::Result<()> {
    let cases: serde_json::Value = serde_json::from_reader(std::fs::File::open(
        root().join("tests/fixtures/debt/balance.json"),
    )?)?;
    for case in cases.as_array().unwrap() {
        let mut p = deterministic();
        p.camera.auto_exposure = false;
        p.debug.deactivate_spatial_effects = true;
        p.film_profile = case["stock"].as_str().unwrap().into();
        p.film_base.cyan = case["cmy"][0].as_f64().unwrap();
        p.film_base.magenta = case["cmy"][1].as_f64().unwrap();
        p.film_base.yellow = case["cmy"][2].as_f64().unwrap();
        let shifts = print_balance::solve_neutral_filter_shifts(&root().join("data"), &p, 0.184)?;
        for (i, v) in shifts.iter().enumerate() {
            assert!(
                (v - case["shifts"][i].as_f64().unwrap()).abs() < 1e-4,
                "{}: {shifts:?}",
                p.film_profile
            );
        }
        p.enlarger.m_filter_shift = shifts[0];
        p.enlarger.y_filter_shift = shifts[1];
        p.io.output_cctf_encoding = false;
        p.io.output_gamut_compress.algorithm = "off".into();
        p.io.output_gamut_compress.lightness_compression = None;
        let rgb = Simulator::new(&root().join("data"), p)?
            .process(&Image::new(2, 2, 3, vec![0.184; 12])?, Tap::RgbOut)?;
        for (i, v) in rgb.data[..3].iter().enumerate() {
            assert!((v - case["rgb"][i].as_f64().unwrap()).abs() < 1e-6);
        }
    }
    Ok(())
}
#[test]
fn file_precision_and_saving_colour_match_reference() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let data = root().join("data");
    let mut p = deterministic();
    p.io.input_color_space = "sRGB".into();
    p.io.input_cctf_decoding = true;
    for (ext, bits) in [
        ("png", 8),
        ("tiff", 8),
        ("tiff", 16),
        ("tiff", 32),
        ("exr", 16),
        ("exr", 32),
    ] {
        let name = format!("file_{ext}{bits}");
        let upstream = root().join(format!("tests/fixtures/debt/{name}.{ext}"));
        compare(
            &file_io::read(&upstream, &data, &p, None)?,
            &fixture(&name),
            0.,
            &name,
        );
        let path = dir.path().join(format!("{name}.{ext}"));
        file_io::write_with_options(
            &fixture("file_input"),
            &path,
            &data,
            "sRGB",
            true,
            Some(bits),
            "zip",
        )?;
        compare(
            &file_io::read(&path, &data, &p, None)?,
            &fixture(&name),
            0.,
            &format!("export {name}"),
        );
    }
    let image = fixture("saving_input");
    compare(
        &file_io::convert_color(
            &image,
            &Tables::load(&data)?,
            "sRGB",
            true,
            "Display P3",
            true,
        )?,
        &fixture("saving_p3"),
        1e-12,
        "saving colour",
    );
    compare(
        &file_io::read(
            &root().join("tests/fixtures/debt/wide_p3.tiff"),
            &data,
            &p,
            None,
        )?,
        &fixture("wide_p3_srgb"),
        1e-3,
        "wide gamut ICC",
    );
    compare(
        &file_io::read(
            &root().join("tests/fixtures/debt/oriented.tiff"),
            &data,
            &p,
            None,
        )?,
        &fixture("oriented"),
        0.,
        "TIFF orientation",
    );
    assert!(
        file_io::write(
            &image,
            &dir.path().join("broken.png"),
            dir.path(),
            "sRGB",
            true
        )
        .is_err()
    );
    assert!(!dir.path().join("broken.png").exists());
    assert!(
        file_io::write_with_options(
            &image,
            &dir.path().join("bad.png"),
            &data,
            "sRGB",
            true,
            Some(32),
            "zip"
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn transactional_saves_preserve_source_aliases_and_existing_destinations() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("source.npy");
    let output = dir.path().join("output.npy");
    std::fs::write(&source, b"source")?;
    std::fs::write(&output, b"original")?;
    assert!(
        file_io::transactional_save(&source, &output, |temp| {
            std::fs::write(temp, b"partial")?;
            anyhow::bail!("encoding/metadata failure")
        })
        .is_err()
    );
    assert_eq!(std::fs::read(&output)?, b"original");
    assert!(file_io::validate_destination(&source, &source).is_err());
    let hard = dir.path().join("hard.npy");
    std::fs::hard_link(&source, &hard)?;
    assert!(file_io::validate_destination(&source, &hard).is_err());
    #[cfg(unix)]
    {
        let link = dir.path().join("alias.npy");
        std::os::unix::fs::symlink(&source, &link)?;
        assert!(file_io::validate_destination(&source, &link).is_err());
    }
    file_io::transactional_save(&source, &output, |temp| {
        std::fs::write(temp, b"complete")?;
        Ok(())
    })?;
    assert_eq!(std::fs::read(&output)?, b"complete");
    assert_eq!(std::fs::read(&source)?, b"source");
    Ok(())
}
#[test]
fn cli_metadata_failure_preserves_destination() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let source = root().join("tests/fixtures/core/metadata.png");
    let output = dir.path().join("existing.png");
    std::fs::write(&output, b"previous destination")?;
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_fast-spektrafilm"))
        .args(["--threads", "1", "process"])
        .arg(&source)
        .arg(&output)
        .args(["--deterministic", "--no-spatial", "--no-auto-exposure"])
        .env("SPEKTRAFILM_GEXIV2_LIBRARY", dir.path().join("missing.so"))
        .output()?;
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("missing.so"));
    assert_eq!(std::fs::read(&output)?, b"previous destination");
    Ok(())
}
#[test]
fn updates_invalidate_dependent_state_and_fail_atomically() -> anyhow::Result<()> {
    let data = root().join("data");
    let mut p = deterministic();
    p.debug.deactivate_spatial_effects = true;
    let input = fixture("injection_input");
    let mut sim = Simulator::new(&data, p.clone())?;
    let old = sim.process(&input, Tap::RgbOut)?;
    p.enlarger.m_filter_shift = 3.;
    p.camera.exposure_compensation_ev = 0.7;
    sim.update(&data, p.clone())?;
    let changed = sim.process(&input, Tap::RgbOut)?;
    assert_ne!(old.data, changed.data);
    compare(
        &changed,
        &Simulator::new(&data, p.clone())?.process(&input, Tap::RgbOut)?,
        0.,
        "update",
    );
    p.workflow.route = "invalid".into();
    assert!(sim.update(&data, p).is_err());
    compare(
        &changed,
        &sim.process(&input, Tap::RgbOut)?,
        0.,
        "failed update",
    );
    Ok(())
}
