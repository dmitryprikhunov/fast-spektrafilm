use fast_spektrafilm::{Image, Params, Simulator, Tap};
use std::path::Path;

#[test]
fn pinned_experimental_stages() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    pool.install(|| -> anyhow::Result<()> {
        for case in [
            "portra_hdr",
            "bw",
            "spatial_reversal",
            "matrix_meter",
            "grain",
            "scanner_print",
            "scanner_reversal",
            "spectral_surface",
            "oklrab",
            "jzazbz",
            "cam16ucs",
            "density_luts",
            "bw_density_luts",
            "fast_grain",
            "random_glare",
            "fast_grain_single_pixel",
            "framing_upscale",
            "framing_downscale",
        ] {
            let folder = root.join("tests/fixtures").join(case);
            let params: Params =
                serde_json::from_reader(std::fs::File::open(folder.join("config.json"))?)?;
            let sim = Simulator::new(&root.join("data"), params)?;
            let input = Image::read_npy(&folder.join("input.npy"))?;
            for tap in [
                Tap::RgbPre,
                Tap::LogEFilm,
                Tap::CmyFilm,
                Tap::LogEPrint,
                Tap::CmyPrint,
                Tap::RgbOut,
            ] {
                if tap == Tap::RgbPre && !case.starts_with("framing_") {
                    continue;
                }
                if case == "scanner_reversal" && [Tap::LogEPrint, Tap::CmyPrint].contains(&tap) {
                    continue;
                }
                let expected = Image::read_npy(&folder.join(format!("{}.npy", tap.name())))?;
                let actual = sim.process(&input, tap)?;
                assert_eq!(
                    (actual.width, actual.height, actual.channels),
                    (expected.width, expected.height, expected.channels)
                );
                for (i, (a, b)) in actual.data.iter().zip(&expected.data).enumerate() {
                    assert!(
                        a.is_finite() && (a - b).abs() <= 1e-9 + 1e-8 * b.abs(),
                        "{case}/{}[{i}]: actual={a}, reference={b}",
                        tap.name()
                    );
                    if tap == Tap::CmyFilm
                        && ["grain", "fast_grain", "fast_grain_single_pixel"].contains(&case)
                    {
                        assert_eq!(a.to_bits(), b.to_bits(), "{case}: grain bit pattern at {i}");
                    }
                }
            }
        }
        Ok(())
    })
}

#[test]
fn unsupported_config_is_explicit() {
    assert!(serde_json::from_str::<Params>(r#"{"camera":{"nonexistent_field":1}}"#).is_err());
    let mut params = Params::default();
    params.io.output_gamut_compress.algorithm = "unimplemented_algorithm".into();
    let error = Simulator::new(&Path::new(env!("CARGO_MANIFEST_DIR")).join("data"), params)
        .err()
        .expect("Unported algorithms must not be silently substituted");
    assert!(error.to_string().contains("supports"));
    let mut params = Params::default();
    params.camera.auto_exposure_method = "unknown_meter".into();
    assert!(
        Simulator::new(&Path::new(env!("CARGO_MANIFEST_DIR")).join("data"), params)
            .err()
            .unwrap()
            .to_string()
            .contains("autoexposure")
    );
    let mut params = Params::default();
    params.settings.apply_grain_presets = false;
    params.grain.density_min.clear();
    assert!(
        Simulator::new(&Path::new(env!("CARGO_MANIFEST_DIR")).join("data"), params)
            .err()
            .unwrap()
            .to_string()
            .contains("density-floor")
    );
}

#[test]
fn malformed_images_return_errors() {
    assert!(Image::new(0, 1, 3, Vec::new()).is_err());
    assert!(Image::new(1, 1, 3, vec![1.0, 2.0]).is_err());
    assert!(Image::new(1, 1, 3, vec![f64::NAN, 0.0, 0.0]).is_err());
}

#[test]
fn stochastic_seed_is_repeatable_across_thread_counts() -> anyhow::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let input = Image::read_npy(&root.join("tests/fixtures/grain/input.npy"))?;
    let mut params = Params::default();
    params.settings.random_seed = Some(12345);
    params.camera.auto_exposure = false;
    let sim = Simulator::new(&root.join("data"), params)?;
    let render = |threads| -> anyhow::Result<Image> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?;
        pool.install(|| sim.process(&input, Tap::RgbOut))
    };
    assert_eq!(render(1)?.data, render(4)?.data);
    assert_eq!(render(4)?.data, render(4)?.data);
    let unseeded = Simulator::new(&root.join("data"), Params::default())?;
    assert_ne!(
        unseeded.process(&input, Tap::RgbOut)?.data,
        unseeded.process(&input, Tap::RgbOut)?.data
    );
    Ok(())
}
