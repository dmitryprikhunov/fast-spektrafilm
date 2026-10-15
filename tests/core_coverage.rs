use fast_spektrafilm::{
    Image, Params, Simulator, Tap, color::Tables, convert, diffusion, profile::Profile,
};
use std::path::{Path, PathBuf};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}
fn fixture(name: &str) -> Image {
    Image::read_npy(
        &root()
            .join("tests/fixtures/core")
            .join(format!("{name}.npy")),
    )
    .unwrap()
}
mod common;
use common::{compare, strict};
#[test]
fn optical_filters_match_reference() -> anyhow::Result<()> {
    for family in ["glimmerglass", "black_pro_mist", "pro_mist", "cinebloom"] {
        let p = serde_json::from_reader(std::fs::File::open(
            root().join(format!("tests/fixtures/core/diffusion_{family}.json")),
        )?)?;
        compare(
            &diffusion::apply(&fixture("input"), &p, 55.)?,
            &fixture(&format!("diffusion_{family}")),
            fast_spektrafilm::numerical::contract("diffusion").atol,
            family,
        );
    }
    Ok(())
}
fn params() -> Params {
    let mut p = Params::default();
    p.camera.auto_exposure = false;
    p.debug.deactivate_spatial_effects = true;
    p.debug.deactivate_stochastic_effects = true;
    p.workflow.route = "input > convert-film > scan".into();
    p
}
#[test]
fn inverse_scan_and_routes_match_reference() -> anyhow::Result<()> {
    let p = params();
    let data = root().join("data");
    let sim = Simulator::new(&data, p.clone())?;
    let converter = convert::FilmConverter::new(&Tables::load(&data)?, sim.film(), &p)?;
    strict(
        &converter.convert(&fixture("captured")),
        &fixture("convert"),
        "inverse captured",
    );
    strict(
        &converter.convert(&fixture("input")),
        &fixture("convert_arbitrary"),
        "inverse arbitrary",
    );
    for (route, name) in [
        ("input > convert-film > scan", "scan"),
        ("input > convert-film > scan-minus-base", "scan-minus-base"),
        ("input > convert-film > print > scan", "scan_print"),
    ] {
        let mut p = params();
        p.workflow.route = route.into();
        let sim = Simulator::new(&data, p)?;
        strict(
            &sim.process(&fixture("captured"), Tap::RgbOut)?,
            &fixture(name),
            route,
        );
        assert!(sim.process(&fixture("captured"), Tap::LogEFilm).is_err());
    }
    Ok(())
}
#[test]
fn base_fit_matches_reference() -> anyhow::Result<()> {
    let data = root().join("data");
    let p = params();
    let stock = Profile::load(&data, &p.film_profile)?;
    let (b, ev) = convert::fit_base(
        &Tables::load(&data)?,
        &stock,
        &p,
        convert::sample_base(&fixture("captured"), 99.)?,
    )?;
    let expected: serde_json::Value = serde_json::from_reader(std::fs::File::open(
        root().join("tests/fixtures/core/base.json"),
    )?)?;
    for (name, v) in [
        ("cyan", b.cyan),
        ("magenta", b.magenta),
        ("yellow", b.yellow),
    ] {
        let error = (v - expected["base"][name].as_f64().unwrap()).abs();
        eprintln!("base fit {name}: max_abs={error:e}");
        assert!(
            error < fast_spektrafilm::numerical::contract("base_fit_coefficients").atol,
            "{name}: {v}"
        );
    }
    let error = (ev - expected["exposure"].as_f64().unwrap()).abs();
    eprintln!("base fit exposure: max_abs={error:e}");
    assert!(error < fast_spektrafilm::numerical::contract("base_fit_coefficients").atol);
    Ok(())
}
#[test]
fn color_managed_file_roundtrip() -> anyhow::Result<()> {
    let data = root().join("data");
    let p = params();
    let mut p = p;
    p.io.input_color_space = "sRGB".into();
    p.io.input_cctf_decoding = true;
    for ext in ["png", "tiff", "jpg"] {
        let path =
            std::env::temp_dir().join(format!("spektrafilm-{}-icc.{ext}", std::process::id()));
        fast_spektrafilm::file_io::write(&fixture("input"), &path, &data, "sRGB", true)?;
        let read = fast_spektrafilm::file_io::read(&path, &data, &p, None)?;
        if ext != "jpg" {
            compare(&read, &fixture("input"), 2e-4, ext);
        } else {
            assert_eq!(read.data.len(), fixture("input").data.len());
        }
        std::fs::remove_file(path)?;
    }
    Ok(())
}
#[test]
fn optical_filter_stage_order_matches_reference() -> anyhow::Result<()> {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    pool.install(|| -> anyhow::Result<()> {
        for stage in ["camera", "enlarger"] {
            let mut p = Params::default();
            p.camera.auto_exposure = false;
            p.debug.deactivate_stochastic_effects = true;
            p.camera.film_format_mm = 0.5;
            let f = if stage == "camera" {
                &mut p.camera.diffusion_filter
            } else {
                &mut p.enlarger.diffusion_filter
            };
            f.active = true;
            f.strength = 0.7;
            let sim = Simulator::new(&root().join("data"), p)?;
            for tap in [
                Tap::LogEFilm,
                Tap::CmyFilm,
                Tap::LogEPrint,
                Tap::CmyPrint,
                Tap::RgbOut,
            ] {
                compare(
                    &sim.process(&fixture("input"), tap)?,
                    &fixture(&format!("{stage}_{}", tap.name())),
                    fast_spektrafilm::numerical::contract("optical").atol,
                    &format!("{stage}/{}", tap.name()),
                );
            }
        }
        Ok(())
    })
}
#[test]
fn temperature_and_tint_match_reference() -> anyhow::Result<()> {
    let tables = Tables::load(&root().join("data"))?;
    for t in [1900., 2850., 5500., 9000.] {
        let mut im = fixture("input");
        im.data.iter_mut().for_each(|v| *v = *v as f32 as f64);
        let p = fast_spektrafilm::params::RawSettings {
            white_balance: "custom".into(),
            temperature: t,
            tint: 1.13,
            ..Default::default()
        };
        fast_spektrafilm::raw::adjust(&mut im, &tables, &p)?;
        compare(
            &im,
            &fixture(&format!("raw_{}", t as u32)),
            2e-7,
            "RAW white balance",
        );
    }
    Ok(())
}
#[test]
#[ignore = "requires explicit Lensfun library and pinned database"]
fn lensfun_matches_reference_when_available() -> anyhow::Result<()> {
    if std::env::var_os("SPEKTRAFILM_LENSFUN_LIBRARY").is_none() {
        anyhow::bail!("Set SPEKTRAFILM_LENSFUN_LIBRARY and SPEKTRAFILM_LENSFUN_DATABASE");
    }
    let mut im = fixture("lens_input");
    let m = fast_spektrafilm::lens::Metadata {
        make: "Canon".into(),
        model: "Canon EOS 5D Mark III".into(),
        lens_make: "Canon".into(),
        lens: "Canon EF 24-105mm f/4L IS USM".into(),
        focal: 24.,
        aperture: 4.,
    };
    let matched = fast_spektrafilm::lens::correct(&mut im, &m)?;
    eprintln!("Matched lens: {matched:?}");
    assert!(matched.is_some());
    let expected = if let Some(path) = std::env::var_os("SPEKTRAFILM_LENSFUN_REFERENCE") {
        Image::read_npy(Path::new(&path))?
    } else {
        fixture("lens_output")
    };
    compare(&im, &expected, 2e-6, "Lensfun");
    Ok(())
}
#[test]
fn all_grain_classes_and_shapes_match_reference() -> anyhow::Result<()> {
    for stock in ["kodak_portra_400", "fujifilm_provia_100f", "kodak_doublex"] {
        for (h, w) in [(1, 9), (9, 1), (8, 11)] {
            let name = format!("grain_{stock}_{h}_{w}");
            let image = fixture(&format!("{name}_input"));
            let mut p = Params {
                film_profile: stock.into(),
                ..Default::default()
            };
            p.camera.auto_exposure = false;
            p.print_glare.active = false;
            p.settings.random_seed = Some(12345);
            let sim = Simulator::new(&root().join("data"), p)?;
            for threads in [1, 2, 4] {
                let actual = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()?
                    .install(|| sim.process(&image, Tap::CmyFilm))?;
                assert_eq!(
                    actual.data,
                    fixture(&name).data,
                    "{name}/{threads}: exact grain"
                );
                compare(
                    &actual,
                    &fixture(&name),
                    1e-9,
                    &format!("{name}/{threads} threads"),
                );
            }
        }
    }
    Ok(())
}
#[test]
fn device_calibration_matches_reference_parameters() -> anyhow::Result<()> {
    let p = params();
    let tables = Tables::load(&root().join("data"))?;
    let sim = Simulator::new(&root().join("data"), p.clone())?;
    let c = convert::FilmConverter::new(&tables, sim.film(), &p)?;
    let fit = c.fit_calibration(&fixture("captured"))?;
    let expected: [[f64; 3]; 3] = serde_json::from_reader(std::fs::File::open(
        root().join("tests/fixtures/core/calibration.json"),
    )?)?;
    let error = fit
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0., f64::max);
    eprintln!("calibration matrix max_abs={error:e}, fit={fit:?}");
    assert!(
        error < fast_spektrafilm::numerical::contract("calibration_coefficients").atol,
        "Calibration coefficient error {error:e}"
    );
    let target = c.scan([0.; 3]);
    let obs = convert::sample_base(&fixture("captured"), 99.)?;
    let before: f64 = (0..3).map(|i| (obs[i] - target[i]).powi(2)).sum();
    let after: f64 = (0..3)
        .map(|i| ((0..3).map(|k| obs[k] * fit[k][i]).sum::<f64>() - target[i]).powi(2))
        .sum();
    eprintln!("calibration clear-base residual: {before:e} -> {after:e}");
    assert!(after < before * 0.05);
    Ok(())
}
#[test]
fn inverse_lut_and_blackbody_match_reference() -> anyhow::Result<()> {
    let mut p = params();
    p.workflow.route = "input > convert-film > print > scan".into();
    p.settings.use_convert_lut = true;
    p.settings.convert_lut_resolution = 9;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    pool.install(|| -> anyhow::Result<()> {
        let sim = Simulator::new(&root().join("data"), p)?;
        strict(
            &sim.process(&fixture("captured"), Tap::RgbOut)?,
            &fixture("convert_lut"),
            "inverse LUT",
        );
        Ok(())
    })?;
    let lamp = Tables::load(&root().join("data"))?.illuminant("BB4725")?;
    compare(
        &Image::new(81, 1, 1, lamp.spectrum)?,
        &fixture("blackbody"),
        1e-12,
        "blackbody",
    );
    Ok(())
}

#[test]
fn dye_permutation_matches_pinned_gui() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = Image::read_npy(&root.join("tests/fixtures/core/input.npy"))?;
    let expected = Image::read_npy(&root.join("tests/fixtures/core/dye_swap.npy"))?;
    let mut p = Params::default();
    p.camera.auto_exposure = false;
    p.debug.deactivate_spatial_effects = true;
    p.debug.deactivate_stochastic_effects = true;
    p.special.film_channel_swap = [0, 2, 1];
    p.special.print_channel_swap = [2, 0, 1];
    let output = Simulator::new(&root.join("data"), p.clone())?.process(&input, Tap::RgbOut)?;
    let max = output
        .data
        .iter()
        .zip(&expected.data)
        .map(|(a, b)| (a - b).abs())
        .fold(0f64, f64::max);
    assert!(max < 1e-8, "Dye permutation maximum error {max}");
    p.special.film_channel_swap = [0, 0, 2];
    assert!(Simulator::new(&root.join("data"), p).is_err());
    Ok(())
}
