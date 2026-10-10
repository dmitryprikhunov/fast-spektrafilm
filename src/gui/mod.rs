//! Native Slint GUI. All simulation and decoding run on one background worker.
mod backend;
#[cfg(test)]
mod tests;
use crate::Params;
use anyhow::{Context, Result, ensure};
use backend::{
    Preview, ROUTE_LABELS, ROUTES, Session, Source, Stock, profiles, read_params, save_params,
};
use slint::{ComponentHandle, ModelRc, Rgb8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

slint::include_modules!();

pub struct Options {
    pub data_dir: PathBuf,
    pub input: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub threads: usize,
}
struct State {
    params: Params,
    path: Option<PathBuf>,
    films: Vec<Stock>,
    papers: Vec<Stock>,
    revision: Arc<AtomicU64>,
    sender: mpsc::Sender<Request>,
    debounce: Timer,
}
struct Request {
    revision: u64,
    job: Job,
}
enum Job {
    Preview {
        path: Option<PathBuf>,
        params: Box<Params>,
    },
    Export {
        path: PathBuf,
        params: Box<Params>,
    },
}
enum Outcome {
    Preview {
        path: PathBuf,
        original: [usize; 2],
        preview: Preview,
    },
    Validated,
    Exported {
        path: PathBuf,
        dimensions: [usize; 2],
    },
}

fn model(values: impl Iterator<Item = String>) -> ModelRc<slint::SharedString> {
    Rc::new(VecModel::from(values.map(Into::into).collect::<Vec<_>>())).into()
}
fn label(stocks: &[Stock], id: &str) -> String {
    stocks
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| id.to_owned(), |s| s.label.clone())
}
fn controls(p: &Params, film: &[Stock], paper: &[Stock]) -> Controls {
    Controls {
        film: label(film, &p.film_profile).into(),
        paper: label(paper, &p.print_profile).into(),
        route: ROUTES
            .iter()
            .position(|route| *route == p.workflow.route)
            .map_or(p.workflow.route.as_str(), |i| ROUTE_LABELS[i])
            .into(),
        route_index: ROUTES
            .iter()
            .position(|v| *v == p.workflow.route)
            .map_or(-1, |i| i as i32),
        meter_index: [
            "center_weighted",
            "average",
            "median",
            "partial",
            "matrix",
            "multi_zone",
            "highlight_weighted",
        ]
        .iter()
        .position(|v| *v == p.camera.auto_exposure_method)
        .map_or(-1, |i| i as i32),
        output_index: [
            "sRGB",
            "Display P3",
            "Adobe RGB (1998)",
            "ProPhoto RGB",
            "ITU-R BT.2020",
            "ACES2065-1",
            "ACEScg",
        ]
        .iter()
        .position(|v| *v == p.io.output_color_space)
        .map_or(-1, |i| i as i32),
        exposure: p.camera.exposure_compensation_ev as f32,
        print_ev: p.enlarger.print_exposure.log2() as f32,
        yellow: p.enlarger.y_filter_shift as f32,
        magenta: p.enlarger.m_filter_shift as f32,
        film_format: p.camera.film_format_mm as f32,
        film_gamma: p.film_chemistry.gamma_factor as f32,
        print_gamma: p.print_chemistry.gamma_factor as f32,
        halation_amount: p.halation.halation_amount as f32,
        scanner_blur: p.scanner.lens_blur as f32,
        sharpen: p.scanner.unsharp_mask[1] as f32,
        auto_exposure: p.camera.auto_exposure,
        grain: p.grain.active,
        halation: p.halation.active,
        glare: p.print_glare.active,
        spatial: !p.debug.deactivate_spatial_effects,
        deterministic: p.debug.deactivate_stochastic_effects,
        encoded_output: p.io.output_cctf_encoding,
    }
}
fn sync(ui: &Darkroom, state: &State) {
    ui.set_controls(controls(&state.params, &state.films, &state.papers));
    ui.set_json_settings(
        serde_json::to_string_pretty(&state.params)
            .expect("Serializable settings")
            .into(),
    );
}
fn error(ui: &Darkroom, message: impl Into<slint::SharedString>) {
    ui.set_error(true);
    ui.set_status(message.into());
}
fn pixels(data: &[u8], size: [u32; 2]) -> slint::Image {
    slint::Image::from_rgb8(SharedPixelBuffer::<Rgb8Pixel>::clone_from_slice(
        data, size[0], size[1],
    ))
}
fn changed(ui: &Darkroom, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    s.revision.fetch_add(1, Ordering::SeqCst);
    sync(ui, &s);
    ui.set_stale(s.path.is_some());
    if ui.get_auto_preview() || ui.get_busy() || s.path.is_none() {
        let weak = ui.as_weak();
        let state = Rc::downgrade(state);
        s.debounce.start(
            TimerMode::SingleShot,
            Duration::from_millis(300),
            move || {
                if let (Some(ui), Some(state)) = (weak.upgrade(), state.upgrade()) {
                    submit(&ui, &state);
                }
            },
        );
    } else {
        ui.set_busy(false);
        ui.set_error(false);
        ui.set_status("Settings changed. Update the preview to see the result.".into());
    }
}
fn submit(ui: &Darkroom, state: &Rc<RefCell<State>>) {
    let s = state.borrow();
    s.debounce.stop();
    let revision = s.revision.fetch_add(1, Ordering::SeqCst) + 1;
    ui.set_busy(true);
    ui.set_error(false);
    ui.set_status(
        if s.path.is_some() {
            "Developing preview…"
        } else {
            "Checking settings…"
        }
        .into(),
    );
    if s.sender
        .send(Request {
            revision,
            job: Job::Preview {
                path: s.path.clone(),
                params: Box::new(s.params.clone()),
            },
        })
        .is_err()
    {
        ui.set_busy(false);
        error(ui, "Render worker stopped. Restart the application.");
    }
}
fn worker(
    mut session: Session,
    receiver: mpsc::Receiver<Request>,
    revision: Arc<AtomicU64>,
    weak: slint::Weak<Darkroom>,
    pool: rayon::ThreadPool,
) {
    while let Ok(request) = receiver.recv() {
        let exporting = matches!(request.job, Job::Export { .. });
        if !exporting && request.revision != revision.load(Ordering::SeqCst) {
            continue;
        }
        let result = pool.install(|| -> Result<Outcome> {
            match request.job {
                Job::Preview { path, params } => {
                    if let Some(path) = path {
                        if session.source.as_ref().is_none_or(|s| s.path != path) {
                            session.source = Some(
                                Source::load(&path)
                                    .with_context(|| format!("Open {}", path.display()))?,
                            );
                        }
                        let preview = session.render(&params)?;
                        let original = session.source.as_ref().unwrap().original;
                        Ok(Outcome::Preview {
                            path,
                            original,
                            preview,
                        })
                    } else {
                        session.validate_params(&params)?;
                        Ok(Outcome::Validated)
                    }
                }
                Job::Export { path, params } => {
                    let dimensions = session.export(&params, &path)?;
                    Ok(Outcome::Exported { path, dimensions })
                }
            }
        });
        let weak = weak.clone();
        let revision = revision.clone();
        let id = request.revision;
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            if !exporting && id != revision.load(Ordering::SeqCst) {
                return;
            }
            ui.set_busy(false);
            ui.set_exporting(false);
            match result {
                Ok(Outcome::Preview {
                    path,
                    original,
                    preview,
                }) => {
                    ui.set_filename(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                            .into(),
                    );
                    ui.set_dimensions(
                        format!(
                            "{} × {} original · {} px preview",
                            original[0],
                            original[1],
                            backend::PREVIEW_EDGE
                        )
                        .into(),
                    );
                    ui.set_before(pixels(&preview.before, preview.before_size));
                    ui.set_after(pixels(&preview.after, preview.after_size));
                    ui.set_render_info(
                        format!(
                            "{} × {} preview · {:.2} s · full frame",
                            preview.after_size[0], preview.after_size[1], preview.seconds
                        )
                        .into(),
                    );
                    ui.set_has_image(true);
                    ui.set_stale(false);
                    ui.set_error(false);
                    ui.set_status(
                        "Preview ready. Export reprocesses the original at full resolution.".into(),
                    );
                }
                Ok(Outcome::Validated) => {
                    ui.set_error(false);
                    ui.set_status("Settings checked. Open an image to begin.".into());
                }
                Ok(Outcome::Exported { path, dimensions }) => {
                    ui.set_error(false);
                    ui.set_status(
                        format!(
                            "Exported {} × {} to {}",
                            dimensions[0],
                            dimensions[1],
                            path.display()
                        )
                        .into(),
                    );
                }
                Err(e) => {
                    error(&ui, format!("{e:#}"));
                }
            }
        });
    }
}

pub fn run(options: Options) -> Result<()> {
    ensure!(options.threads > 0, "Thread count must be positive");
    let (films, papers) = profiles(&options.data_dir)?;
    let params = options
        .config
        .as_deref()
        .map(read_params)
        .transpose()?
        .unwrap_or_default();
    let session = Session::new(&options.data_dir)?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.threads)
        .build()?;
    let ui = Darkroom::new()?;
    let (sender, receiver) = mpsc::channel();
    let revision = Arc::new(AtomicU64::new(0));
    let state = Rc::new(RefCell::new(State {
        params,
        path: options.input,
        films,
        papers,
        revision: revision.clone(),
        sender,
        debounce: Timer::default(),
    }));
    ui.set_films(model(state.borrow().films.iter().map(|s| s.label.clone())));
    ui.set_papers(model(state.borrow().papers.iter().map(|s| s.label.clone())));
    sync(&ui, &state.borrow());
    let weak = ui.as_weak();
    std::thread::Builder::new()
        .name("darkroom-render".into())
        .spawn(move || worker(session, receiver, revision, weak, pool))?;

    bind_callbacks(&ui, &state);
    submit(&ui, &state);
    ui.run()?;
    Ok(())
}

fn bind_callbacks(ui: &Darkroom, state: &Rc<RefCell<State>>) {
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_open_image(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        let path = rfd::FileDialog::new()
            .set_title("Open camera RAW or RGB NPY")
            .add_filter(
                "Camera RAW / RGB NPY",
                &[
                    "cr2", "cr3", "nef", "nrw", "arw", "dng", "raf", "orf", "rw2", "pef", "3fr",
                    "fff", "mef", "mos", "raw", "npy",
                ],
            )
            .add_filter("All files", &["*"])
            .pick_file();
        if let Some(path) = path {
            s.borrow_mut().path = Some(path.clone());
            ui.set_filename(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            );
            ui.set_has_image(false);
            ui.set_stale(true);
            submit(&ui, &s);
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_number_edited(move |key, value| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        {
            let mut s = s.borrow_mut();
            set_number(&mut s.params, &key, f64::from(value));
        }
        changed(&ui, &s);
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_toggle_edited(move |key, value| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        {
            let mut s = s.borrow_mut();
            set_toggle(&mut s.params, &key, value);
        }
        changed(&ui, &s);
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_choice_edited(move |key, value| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        {
            let mut s = s.borrow_mut();
            match key.as_str() {
                "film" => {
                    if let Some(stock) = s.films.iter().find(|v| v.label == value.as_str()) {
                        s.params.film_profile = stock.id.clone();
                    }
                }
                "paper" => {
                    if let Some(stock) = s.papers.iter().find(|v| v.label == value.as_str()) {
                        s.params.print_profile = stock.id.clone();
                    }
                }
                "route" => {
                    if let Some(i) = ROUTE_LABELS.iter().position(|v| *v == value.as_str()) {
                        s.params.workflow.route = ROUTES[i].into();
                    }
                }
                "meter" => s.params.camera.auto_exposure_method = value.into(),
                "output" => s.params.io.output_color_space = value.into(),
                _ => {}
            }
        }
        changed(&ui, &s);
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_apply_json(move |text| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        match serde_json::from_str::<Params>(&text) {
            Ok(params) => {
                s.borrow_mut().params = params;
                changed(&ui, &s);
                submit(&ui, &s);
            }
            Err(e) => error(&ui, format!("Invalid settings: {e}")),
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_load_settings(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Settings", &["json"])
            .pick_file()
        {
            match read_params(&path) {
                Ok(params) => {
                    s.borrow_mut().params = params;
                    changed(&ui, &s);
                    submit(&ui, &s);
                }
                Err(e) => error(&ui, format!("{e:#}")),
            }
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_save_settings(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("spektrafilm-settings.json")
            .add_filter("Settings", &["json"])
            .save_file()
        {
            match save_params(&s.borrow().params, &path) {
                Ok(()) => {
                    ui.set_error(false);
                    ui.set_status(format!("Saved settings to {}", path.display()).into());
                }
                Err(e) => error(&ui, format!("Save settings: {e:#}")),
            }
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_reset_settings(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        s.borrow_mut().params = Params::default();
        changed(&ui, &s);
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_preview(move || {
        if let Some(ui) = weak.upgrade()
            && !ui.get_exporting()
        {
            submit(&ui, &s);
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_export_image(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_busy() || !ui.get_has_image() {
            return;
        }
        let stem = s
            .borrow()
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let (format, extension) = [
            ("Float TIFF", "tiff"),
            ("sRGB PNG (8-bit)", "png"),
            ("Float64 NPY", "npy"),
        ]
        .get(ui.get_export_format() as usize)
        .copied()
        .unwrap_or(("sRGB PNG (8-bit)", "png"));
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export original resolution · PNG is sRGB; TIFF/NPY use output settings")
            .set_file_name(format!("{stem}-developed.{extension}"))
            .add_filter(format, &[extension])
            .save_file()
        {
            let s = s.borrow();
            s.debounce.stop();
            ui.set_busy(true);
            ui.set_exporting(true);
            ui.set_error(false);
            ui.set_status("Developing original resolution for export…".into());
            let revision = s.revision.fetch_add(1, Ordering::SeqCst) + 1;
            if s.sender
                .send(Request {
                    revision,
                    job: Job::Export {
                        path,
                        params: Box::new(s.params.clone()),
                    },
                })
                .is_err()
            {
                ui.set_busy(false);
                ui.set_exporting(false);
                error(&ui, "Render worker stopped.");
            }
        }
    });
}

fn set_number(p: &mut Params, key: &str, value: f64) {
    match key {
        "exposure" => p.camera.exposure_compensation_ev = value,
        "print_ev" => p.enlarger.print_exposure = value.exp2(),
        "yellow" => p.enlarger.y_filter_shift = value,
        "magenta" => p.enlarger.m_filter_shift = value,
        "film_format" => p.camera.film_format_mm = value,
        "film_gamma" => p.film_chemistry.gamma_factor = value,
        "print_gamma" => p.print_chemistry.gamma_factor = value,
        "halation_amount" => p.halation.halation_amount = value,
        "scanner_blur" => p.scanner.lens_blur = value,
        "sharpen" => p.scanner.unsharp_mask[1] = value,
        _ => {}
    }
}
fn set_toggle(p: &mut Params, key: &str, value: bool) {
    match key {
        "auto_exposure" => p.camera.auto_exposure = value,
        "grain" => p.grain.active = value,
        "halation" => p.halation.active = value,
        "glare" => p.print_glare.active = value,
        "spatial" => p.debug.deactivate_spatial_effects = !value,
        "deterministic" => p.debug.deactivate_stochastic_effects = value,
        "encoded_output" => p.io.output_cctf_encoding = value,
        _ => {}
    }
}
