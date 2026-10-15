//! Isolated cached-crop timings and actual Rust allocator accounting.
//! Build first; run separately under /usr/bin/time. Counts exclude input cloning.
use fast_spektrafilm::{Image, Params, Simulator, Tap, color::Tables, convert::FilmConverter};
use serde_json::json;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
struct Counted;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
fn allocated(n: usize) {
    CALLS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(n, Ordering::Relaxed);
    let live = LIVE.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, n: usize) -> *mut u8 {
        let out = unsafe { System.realloc(p, layout, n) };
        if !out.is_null() {
            LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
            allocated(n);
        }
        out
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
fn measure<T, U>(
    mut prepare: impl FnMut() -> T,
    mut run: impl FnMut(T) -> anyhow::Result<U>,
) -> anyhow::Result<(U, serde_json::Value)> {
    let mut samples = Vec::new();
    let mut result = None;
    for iteration in 0..6 {
        drop(result.take());
        let input = prepare();
        let base = LIVE.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        let calls = CALLS.load(Ordering::Relaxed);
        let bytes = BYTES.load(Ordering::Relaxed);
        let start = Instant::now();
        let output = run(input)?;
        let seconds = start.elapsed().as_secs_f64();
        let allocation_calls = CALLS.load(Ordering::Relaxed) - calls;
        let allocated_bytes = BYTES.load(Ordering::Relaxed) - bytes;
        let peak_additional_live_bytes = PEAK.load(Ordering::Relaxed).saturating_sub(base);
        samples.push(json!({"warmup":iteration==0,"seconds":seconds,
            "allocation_calls":allocation_calls,"allocated_bytes":allocated_bytes,
            "peak_additional_live_bytes":peak_additional_live_bytes}));
        result = Some(output);
    }
    Ok((
        result.unwrap(),
        json!({"warmups":1,"measured_repeats":5,"samples":samples}),
    ))
}
fn main() -> anyhow::Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build_global()?;
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "review_performance CROP.npy MODE OUTPUT_DIRECTORY"
    );
    let image = Image::read_npy(&PathBuf::from(&args[1]))?;
    anyhow::ensure!(
        image.width.max(image.height) <= 512,
        "Use a cached crop, at most 512px"
    );
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    let folder = PathBuf::from(&args[3]);
    std::fs::create_dir_all(&folder)?;
    let mode = args[2].as_str();
    let mut p = Params::default();
    p.camera.auto_exposure = false;
    p.camera.film_format_mm = 1.5;
    p.debug.deactivate_stochastic_effects = true;
    p.settings.random_seed = Some(12345);
    p.io.copy_metadata = false;
    if mode == "inverse" {
        p.workflow.route = "input > convert-film > scan".into();
        p.debug.deactivate_spatial_effects = true;
    }
    if mode == "resize" {
        p.io.upscale_factor = 1.5;
    }
    let start = Instant::now();
    let mut sim = Simulator::new(&data, p.clone())?;
    let cold_setup = start.elapsed().as_secs_f64();
    let (output, report) = match mode {
        m if m == "setup" || m.starts_with("update") => {
            let ((), report) = measure(
                || p.clone(),
                |mut p| {
                    match mode {
                        "update-film" => p.film_profile = "fujifilm_pro_400h".into(),
                        "update-base" => p.film_base.scale = 0.9,
                        "update-chemistry" => p.film_chemistry.gamma_factor = 1.05,
                        "update-filter" => p.camera.diffusion_filter.active = true,
                        "update-gamut" => p.io.input_gamut_compress.knee[0] = 0.8,
                        "update-lut" => {
                            p.settings.use_enlarger_lut = true;
                            p.settings.lut_resolution = 9;
                        }
                        "update-dimensions" => p.io.upscale_factor = 1.1,
                        "update-seed" => p.settings.random_seed = Some(123),
                        "update-delivery" => {
                            p.workflow.saving_color_space = Some("Display P3".into())
                        }
                        _ => p.camera.exposure_compensation_ev = 0.125,
                    }
                    sim.update(&data, p)
                },
            )?;
            (sim.process(&image, Tap::RgbOut)?, report)
        }
        "fir" | "iir" => measure(
            || (),
            |()| {
                Ok(fast_spektrafilm::spatial::gaussian(
                    &image,
                    &[if mode == "fir" { 1.0 } else { 3.0 }],
                ))
            },
        )?,
        "diffusion" => {
            let filter = fast_spektrafilm::params::DiffusionFilter {
                active: true,
                ..Default::default()
            };
            measure(
                || (),
                |()| {
                    fast_spektrafilm::diffusion::apply(
                        &image,
                        &filter,
                        1500. / image.width.max(image.height) as f64,
                    )
                },
            )?
        }
        "inverse" => {
            let converter = FilmConverter::new(&Tables::load(&data)?, sim.film(), &p)?;
            measure(|| (), |()| Ok(converter.convert(&image)))?
        }
        "grain" => {
            let density = sim.process(&image, Tap::CmyFilm)?;
            let mut grain = sim.params().grain.clone();
            grain.active = true;
            measure(
                || (),
                |()| {
                    fast_spektrafilm::grain::apply(
                        &density,
                        sim.film(),
                        &grain,
                        1500. / image.width.max(image.height) as f64,
                        false,
                        12345,
                    )
                },
            )?
        }
        "preview" => measure(|| (), |()| fast_spektrafilm::preview::resize(&image, 97))?,
        "resize" => measure(
            || image.clone(),
            |image| sim.process_owned(image, Tap::RgbPre),
        )?,
        "staged" => measure(
            || image.clone(),
            |image| {
                Ok(sim
                    .process_profiled(image, Tap::RgbIn, Tap::RgbOut, 1.)?
                    .image)
            },
        )?,
        _ => measure(
            || image.clone(),
            |image| sim.process_owned(image, Tap::RgbOut),
        )?,
    };
    output.validate()?;
    output.write_npy(&folder.join(format!("{mode}.npy")))?;
    println!(
        "{}",
        json!({"mode":mode,"cold_setup_seconds":cold_setup,"threads":4,
        "shape":[image.height,image.width,image.channels],"film_format_mm":1.5,"executor":if mode=="staged"{"profiled"}else{"stage_or_fused"},"measurements":report})
    );
    Ok(())
}
