//! Native Slint GUI. All simulation and decoding run on one background worker.
mod backend;
mod inspector;
#[cfg(test)]
mod tests;
use crate::Params;
use anyhow::{Context, Result};
use backend::{Preview, Session, Source, Stock, profiles, read_params, save_params};
use slint::{ComponentHandle, ModelRc, Rgb8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};
use std::{
    cell::RefCell,
    collections::HashSet,
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
    /// Zero lets Rayon use its automatic CPU/environment-based default.
    pub threads: usize,
}
struct State {
    params: Params,
    path: Option<PathBuf>,
    source_generation: u64,
    films: Vec<Stock>,
    papers: Vec<Stock>,
    filters: Vec<String>,
    sections: HashSet<String>,
    revision: Arc<AtomicU64>,
    sender: mpsc::Sender<Request>,
    debounce: Timer,
    dialog_timer: Timer,
    dialog_pending: bool,
}
struct Request {
    revision: u64,
    job: Job,
}
enum Job {
    Preview {
        path: Option<PathBuf>,
        params: Box<Params>,
        full: bool,
        source_generation: u64,
    },
    Calibrate {
        path: PathBuf,
        params: Box<Params>,
        device: bool,
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
        params: Box<Params>,
    },
    Validated(Box<Params>),
    Calibrated(Box<Params>),
    Exported {
        path: PathBuf,
        dimensions: [usize; 2],
    },
}

fn model(values: impl Iterator<Item = String>) -> ModelRc<slint::SharedString> {
    Rc::new(VecModel::from(values.map(Into::into).collect::<Vec<_>>())).into()
}
fn sync(ui: &Darkroom, state: &State) {
    ui.set_fields(Rc::new(VecModel::from(inspector::fields(state, ui.get_tab()))).into());
    ui.set_monitor_name(
        state
            .params
            .io
            .monitor_profile
            .as_deref()
            .unwrap_or("sRGB")
            .into(),
    );
}
fn initial_sections() -> HashSet<String> {
    [
        "0:Profiles",
        "0:Camera",
        "0:Enlarger",
        "1:Development",
        "2:Development",
        "3:Negative conversion",
        "4:Processing",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
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
    submit_size(ui, state, false);
}
fn submit_size(ui: &Darkroom, state: &Rc<RefCell<State>>, full: bool) {
    submit_request(ui, state, full, false);
}
fn submit_request(ui: &Darkroom, state: &Rc<RefCell<State>>, full: bool, reload: bool) {
    let mut s = state.borrow_mut();
    if reload {
        // Reopening a path is a source change even if the filename is unchanged.
        // Every subsequent request carries it, including when this one is skipped.
        s.source_generation = s.source_generation.wrapping_add(1);
    }
    s.debounce.stop();
    let revision = s.revision.fetch_add(1, Ordering::SeqCst) + 1;
    ui.set_busy(true);
    ui.set_error(false);
    ui.set_status(
        if s.path.is_some() {
            if full {
                "Scanning at full resolution…"
            } else {
                "Developing preview…"
            }
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
                full,
                source_generation: s.source_generation,
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
    let mut loaded_source_generation = 0;
    while let Ok(request) = receiver.recv() {
        let exporting = matches!(request.job, Job::Export { .. });
        if !exporting && request.revision != revision.load(Ordering::SeqCst) {
            continue;
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pool.install(|| -> Result<Outcome> {
                match request.job {
                    Job::Preview {
                        path,
                        params,
                        full,
                        source_generation,
                    } => {
                        if let Some(path) = path {
                            if source_generation != loaded_source_generation
                                || session
                                    .source
                                    .as_ref()
                                    .is_none_or(|s| !s.matches(&path, &params))
                            {
                                session.set_source(
                                    Source::load(&path, &session.data, &params)
                                        .with_context(|| format!("Open {}", path.display()))?,
                                );
                                loaded_source_generation = source_generation;
                            }
                            let preview = if full {
                                session.render_size(&params, true)?
                            } else {
                                session.render(&params)?
                            };
                            let original = session.source.as_ref().unwrap().original;
                            Ok(Outcome::Preview {
                                path,
                                original,
                                preview,
                                params: Box::new(session.resolved_params(&params)?),
                            })
                        } else {
                            session.validate_params(&params)?;
                            Ok(Outcome::Validated(Box::new(
                                session.resolved_params(&params)?,
                            )))
                        }
                    }
                    Job::Calibrate {
                        path,
                        params,
                        device,
                    } => {
                        if session
                            .source
                            .as_ref()
                            .is_none_or(|s| !s.matches(&path, &params))
                        {
                            session.set_source(Source::load(&path, &session.data, &params)?);
                        }
                        Ok(Outcome::Calibrated(Box::new(
                            session.calibrate(&params, device)?,
                        )))
                    }
                    Job::Export { path, params } => {
                        let dimensions = session.export(&params, &path)?;
                        Ok(Outcome::Exported { path, dimensions })
                    }
                }
            })
        }))
        .unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "Processing failed unexpectedly; adjust the settings and retry."
            ))
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
                    params,
                }) => {
                    ui.invoke_resolved(serde_json::to_string(&params).unwrap().into());
                    ui.set_filename(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                            .into(),
                    );
                    ui.set_dimensions(format!("{} × {}", original[0], original[1]).into());
                    ui.set_before(pixels(&preview.before, preview.before_size));
                    ui.set_after(pixels(&preview.after, preview.after_size));
                    ui.set_render_info(
                        format!(
                            "{} × {} · {:.2} s",
                            preview.after_size[0], preview.after_size[1], preview.seconds
                        )
                        .into(),
                    );
                    ui.set_has_image(true);
                    ui.set_stale(false);
                    ui.set_error(false);
                    ui.set_status("Ready".into());
                }
                Ok(Outcome::Calibrated(params)) => {
                    ui.invoke_calibrated(serde_json::to_string(&params).unwrap().into());
                }
                Ok(Outcome::Validated(params)) => {
                    ui.invoke_resolved(serde_json::to_string(&params).unwrap().into());
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

fn processing_pool(threads: usize) -> Result<rayon::ThreadPool> {
    Ok(rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()?)
}

pub fn run(options: Options) -> Result<()> {
    let (films, papers) = profiles(&options.data_dir)?;
    let params = options
        .config
        .as_deref()
        .map(read_params)
        .transpose()?
        .map(Ok)
        .unwrap_or_else(|| {
            let path = backend::default_params_path()?;
            if path.exists() {
                read_params(&path)
            } else {
                Ok(Params::default())
            }
        })?;
    let session = Session::new(&options.data_dir)?;
    let pool = processing_pool(options.threads)?;
    let ui = Darkroom::new()?;
    let (sender, receiver) = mpsc::channel();
    let revision = Arc::new(AtomicU64::new(0));
    let state = Rc::new(RefCell::new(State {
        params,
        path: options.input,
        source_generation: 0,
        films,
        papers,
        filters: crate::color::Tables::load(&options.data_dir)?
            .camera_filters
            .keys()
            .cloned()
            .collect(),
        sections: initial_sections(),
        revision: revision.clone(),
        sender,
        debounce: Timer::default(),
        dialog_timer: Timer::default(),
        dialog_pending: false,
    }));
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

// XDG portals use case-sensitive extension globs. Include both camera filename
// conventions explicitly instead of relying on the host chooser's defaults.
const RAW_EXTENSIONS: &[&str] = &[
    "3fr", "ari", "arw", "bay", "cap", "cr2", "cr3", "crw", "dcr", "dcs", "dng", "drf", "eip",
    "erf", "fff", "gpr", "iiq", "k25", "kdc", "mdc", "mef", "mos", "mrw", "nef", "nrw", "obm",
    "orf", "pef", "ptx", "pxn", "r3d", "raf", "raw", "rw2", "rwl", "rwz", "sr2", "srf", "srw",
    "sti", "x3f",
];
const RGB_EXTENSIONS: &[&str] = &["npy", "jpg", "jpeg", "png", "tif", "tiff", "exr", "webp"];
fn extensions_with_case(extensions: &[&str]) -> Vec<String> {
    extensions
        .iter()
        .flat_map(|ext| [ext.to_string(), ext.to_ascii_uppercase()])
        .collect()
}
fn image_dialog() -> rfd::FileDialog {
    let raw = extensions_with_case(RAW_EXTENSIONS);
    let rgb = extensions_with_case(RGB_EXTENSIONS);
    let all: Vec<_> = raw.iter().chain(&rgb).cloned().collect();
    rfd::FileDialog::new()
        .set_title("Open image")
        .add_filter("Images", &all)
        .add_filter("Camera RAW", &raw)
        .add_filter("RGB images", &rgb)
        .add_filter("All files", &["*"])
}

// Native file choosers can wait on a portal for a long time. Keep that wait off
// Slint's event loop and deliver only the selected path back to the UI thread.
fn choose_file(
    ui: &Darkroom,
    state: &Rc<RefCell<State>>,
    pick: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    selected: impl Fn(&Darkroom, &Rc<RefCell<State>>, PathBuf) + 'static,
) {
    if state.borrow().dialog_pending {
        return;
    }
    let (sender, receiver) = mpsc::channel();
    if let Err(e) = std::thread::Builder::new()
        .name("darkroom-dialog".into())
        .spawn(move || {
            let _ = sender.send(pick());
        })
    {
        error(ui, format!("Open file dialog: {e}"));
        return;
    }
    state.borrow_mut().dialog_pending = true;
    ui.set_dialog_open(true);
    let weak_ui = ui.as_weak();
    let weak_state = Rc::downgrade(state);
    state
        .borrow()
        .dialog_timer
        .start(TimerMode::Repeated, Duration::from_millis(30), move || {
            let (Some(ui), Some(state)) = (weak_ui.upgrade(), weak_state.upgrade()) else {
                return;
            };
            let path = match receiver.try_recv() {
                Ok(path) => path,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    error(&ui, "File dialog closed unexpectedly.");
                    None
                }
            };
            {
                let mut s = state.borrow_mut();
                s.dialog_timer.stop();
                s.dialog_pending = false;
            }
            ui.set_dialog_open(false);
            if let Some(path) = path {
                selected(&ui, &state, path);
            }
        });
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
        let dialog = image_dialog();
        choose_file(
            &ui,
            &s,
            move || dialog.pick_file(),
            |ui, s, path| {
                if ui.get_exporting() {
                    return;
                }
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
                submit_request(ui, s, false, true);
            },
        );
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_field_edited(move |key, value| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        let result = inspector::edit(&mut s.borrow_mut(), &key, &value);
        match result {
            Ok(()) => changed(&ui, &s),
            Err(e) => error(&ui, format!("{e:#}")),
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_section_toggled(move |key| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let mut s = s.borrow_mut();
        if !s.sections.remove(key.as_str()) {
            s.sections.insert(key.into());
        }
        sync(&ui, &s);
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_tab_selected(move |_| {
        if let Some(ui) = weak.upgrade() {
            sync(&ui, &s.borrow());
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_monitor(move |choose| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        if choose {
            let dialog = rfd::FileDialog::new()
                .add_filter("ICC profile", &extensions_with_case(&["icc", "icm"]));
            choose_file(
                &ui,
                &s,
                move || dialog.pick_file(),
                |ui, s, path| {
                    if ui.get_exporting() {
                        return;
                    }
                    s.borrow_mut().params.io.monitor_profile =
                        Some(path.to_string_lossy().into_owned());
                    changed(ui, s);
                },
            );
        } else {
            s.borrow_mut().params.io.monitor_profile = None;
            changed(&ui, &s);
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_calibrate(move |device| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_busy() || !ui.get_has_image() {
            return;
        }
        let s = s.borrow();
        s.debounce.stop();
        ui.set_busy(true);
        ui.set_exporting(true);
        ui.set_status("Fitting scan calibration…".into());
        let revision = s.revision.fetch_add(1, Ordering::SeqCst) + 1;
        if s.sender
            .send(Request {
                revision,
                job: Job::Calibrate {
                    path: s.path.clone().unwrap(),
                    params: Box::new(s.params.clone()),
                    device,
                },
            })
            .is_err()
        {
            ui.set_busy(false);
            ui.set_exporting(false);
            error(&ui, "Render worker stopped.");
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_resolved(move |text| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if let Ok(p) = serde_json::from_str(&text) {
            s.borrow_mut().params = p;
            sync(&ui, &s.borrow());
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_calibrated(move |text| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if let Ok(p) = serde_json::from_str(&text) {
            s.borrow_mut().params = p;
            changed(&ui, &s);
            submit(&ui, &s);
        }
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_scan(move || {
        if let Some(ui) = weak.upgrade()
            && !ui.get_busy()
        {
            submit_size(&ui, &s, true);
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
        let dialog =
            rfd::FileDialog::new().add_filter("Settings", &extensions_with_case(&["json"]));
        choose_file(
            &ui,
            &s,
            move || dialog.pick_file(),
            |ui, s, path| {
                if ui.get_exporting() {
                    return;
                }
                match read_params(&path) {
                    Ok(params) => {
                        s.borrow_mut().params = params;
                        changed(ui, s);
                        submit(ui, s);
                    }
                    Err(e) => error(ui, format!("{e:#}")),
                }
            },
        );
    });
    let weak = ui.as_weak();
    let s = state.clone();
    ui.on_save_default(move || {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.get_exporting() {
            return;
        }
        let result = (|| -> Result<()> {
            let path = backend::default_params_path()?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            save_params(&s.borrow().params, &path)
        })();
        match result {
            Ok(()) => {
                ui.set_error(false);
                ui.set_status("Startup settings saved.".into());
            }
            Err(e) => error(&ui, format!("{e:#}")),
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
        let dialog = rfd::FileDialog::new()
            .set_file_name("spektrafilm-settings.json")
            .add_filter("Settings", &["json"]);
        choose_file(
            &ui,
            &s,
            move || dialog.save_file(),
            |ui, s, path| match save_params(&s.borrow().params, &path) {
                Ok(()) => {
                    ui.set_error(false);
                    ui.set_status(format!("Saved settings to {}", path.display()).into());
                }
                Err(e) => error(ui, format!("Save settings: {e:#}")),
            },
        );
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
            ("PNG (16-bit)", "png"),
            ("Float64 NPY", "npy"),
            ("JPEG", "jpg"),
            ("Float EXR", "exr"),
        ]
        .get(ui.get_export_format() as usize)
        .copied()
        .unwrap_or(("PNG (16-bit)", "png"));
        let dialog = rfd::FileDialog::new()
            .set_title("Save image")
            .set_file_name(format!("{stem}-developed.{extension}"))
            .add_filter(format, &[extension]);
        choose_file(
            &ui,
            &s,
            move || dialog.save_file(),
            |ui, s, path| {
                // A pending automatic preview may have started while the dialog
                // was open. Queue the export behind it rather than losing the save.
                if ui.get_exporting() || !ui.get_has_image() {
                    return;
                }
                let s = s.borrow();
                s.debounce.stop();
                ui.set_busy(true);
                ui.set_exporting(true);
                ui.set_error(false);
                ui.set_status("Saving full-resolution image…".into());
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
                    error(ui, "Render worker stopped.");
                }
            },
        );
    });
}
