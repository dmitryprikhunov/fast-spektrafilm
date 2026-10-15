//! Risk-based interactions extracted from the pinned Python on cached RAW crops.
use fast_spektrafilm::{Image, Params, Simulator, Tap, numerical};
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn tap(name: &str) -> Tap {
    match name {
        "log_e_film" => Tap::LogEFilm,
        "cmy_film" => Tap::CmyFilm,
        "log_e_print" => Tap::LogEPrint,
        "cmy_print" => Tap::CmyPrint,
        _ => Tap::RgbOut,
    }
}
#[test]
fn interactions_match_reference_and_executors() -> anyhow::Result<()> {
    let folder = root().join("tests/fixtures/review");
    let manifest: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(folder.join("manifest.json"))?)?;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(4).build()?;
    pool.install(|| -> anyhow::Result<()> {
        for case in manifest["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let dir = folder.join(name);
            let p: Params = serde_json::from_reader(std::fs::File::open(dir.join("config.json"))?)?;
            let spacing = p.camera.film_format_mm * 1000. / p.io.upscale_factor;
            let sim = Simulator::new(&root().join("data"), p)?;
            let input = Image::read_npy(&dir.join("input.npy"))?;
            let spacing = spacing / input.width.max(input.height) as f64;
            let contract = numerical::contract(case["policy"].as_str().unwrap());
            for name_tap in case["taps"].as_array().unwrap() {
                let target = tap(name_tap.as_str().unwrap());
                let expected = Image::read_npy(&dir.join(format!("{}.npy", target.name())))?;
                let actual = sim.process(&input, target)?;
                let stage_contract = if name.starts_with("optical_") && target == Tap::LogEFilm {
                    numerical::contract("optical_log_exposure")
                } else {
                    contract
                };
                let report = numerical::compare(&actual, &expected, stage_contract)?;
                eprintln!(
                    "{name}/{}: {}",
                    target.name(),
                    serde_json::to_string(&report)?
                );
                assert_eq!(report.failures, 0, "{name}/{}: {report:?}", target.name());
                if name == "grain_chemistry" && target == Tap::CmyFilm {
                    assert!(
                        actual
                            .data
                            .iter()
                            .zip(&expected.data)
                            .all(|(a, b)| a.to_bits() == b.to_bits())
                    );
                }
                let profiled = sim
                    .process_profiled(input.clone(), Tap::RgbIn, target, 1.)?
                    .image;
                assert_eq!(
                    numerical::compare(&profiled, &actual, numerical::contract("strict"))?.failures,
                    0,
                    "{name}: executors"
                );
                // Every applicable materialized tap can resume the same render.
                if !name.starts_with("inverse_nodes") && target != Tap::RgbOut {
                    let resumed = sim.process_from(actual, target, Tap::RgbOut, spacing)?;
                    let final_image = sim.process(&input, Tap::RgbOut)?;
                    assert_eq!(
                        numerical::compare(&resumed, &final_image, numerical::contract("strict"))?
                            .failures,
                        0,
                        "{name}: injection {}",
                        target.name()
                    );
                }
            }
        }
        Ok(())
    })
}
#[test]
fn conversion_seed_contract_across_executors_and_threads() -> anyhow::Result<()> {
    let dir = root().join("tests/fixtures/review/convert_glare");
    let mut p: Params = serde_json::from_reader(std::fs::File::open(dir.join("config.json"))?)?;
    let image = Image::read_npy(&dir.join("input.npy"))?;
    let sim = Simulator::new(&root().join("data"), p.clone())?;
    let mut outputs = Vec::new();
    for n in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(n).build()?;
        outputs.push(pool.install(|| sim.process(&image, Tap::RgbOut))?.data);
        outputs.push(
            pool.install(|| sim.process_profiled(image.clone(), Tap::RgbIn, Tap::RgbOut, 1.))?
                .image
                .data,
        );
    }
    assert!(outputs.windows(2).all(|v| v[0] == v[1]));
    p.settings.random_seed = None;
    let sim = Simulator::new(&root().join("data"), p)?;
    assert_ne!(
        sim.process(&image, Tap::RgbOut)?.data,
        sim.process(&image, Tap::RgbOut)?.data
    );
    assert_ne!(
        sim.process_profiled(image.clone(), Tap::RgbIn, Tap::RgbOut, 1.)?
            .image
            .data,
        sim.process_profiled(image, Tap::RgbIn, Tap::RgbOut, 1.)?
            .image
            .data
    );
    Ok(())
}

#[test]
fn gaussian_sigma_sweep_and_threads_match_reference() -> anyhow::Result<()> {
    let dir = root().join("tests/fixtures/review/filters");
    let input = Image::read_npy(&dir.join("input.npy"))?;
    let cases: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(dir.join("manifest.json"))?)?;
    for threads in [1, 2, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()?;
        for case in cases.as_array().unwrap() {
            let mut input = input.clone();
            let f32 = case["float32"].as_bool().unwrap();
            if f32 {
                input.data.iter_mut().for_each(|v| *v = *v as f32 as f64);
            }
            let sigma = case["sigma"].as_f64().unwrap();
            let actual = pool.install(|| {
                if f32 {
                    fast_spektrafilm::spatial::gaussian_f32(&input, &[sigma])
                } else {
                    fast_spektrafilm::spatial::gaussian(&input, &[sigma])
                }
            });
            let expected =
                Image::read_npy(&dir.join(format!("{}.npy", case["name"].as_str().unwrap())))?;
            let policy = if f32 {
                numerical::contract("grain_exact")
            } else {
                numerical::contract("strict")
            };
            let report = numerical::compare(&actual, &expected, policy)?;
            assert_eq!(
                report.failures, 0,
                "{} threads={threads}: {report:?}",
                case["name"]
            );
            if f32 {
                assert!(
                    actual
                        .data
                        .iter()
                        .zip(&expected.data)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                );
            }
        }
    }
    Ok(())
}
