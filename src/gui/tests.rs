use super::*;
use crate::file_io;
use crate::{
    Image, Simulator, Tap,
    color::Tables,
    image::{decode_raw, decode_raw_preview},
};
use anyhow::ensure;
use slint::platform::{
    EventLoopProxy, Platform, WindowAdapter,
    software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
};
use std::path::Path;

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("data")
}
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/portra_hdr")
}
fn deterministic() -> Params {
    let mut p = Params::default();
    p.io.copy_metadata = false;
    p.debug.deactivate_stochastic_effects = true;
    p.debug.deactivate_spatial_effects = true;
    p
}

#[test]
fn rotation_and_delivery_colour_keep_scan_cache_consistent() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = Image::new(9, 7, 3, (0..189).map(|v| v as f64 / 200.).collect())?;
    let path = dir.path().join("source.npy");
    input.write_npy(&path)?;
    let mut p = deterministic();
    p.io.input_rotation = 1;
    let mut session = Session::new(&data())?;
    session.set_source(Source::load(&path, &data(), &p)?);
    assert_eq!(session.source.as_ref().unwrap().original, [7, 9]);
    session.render_size(&p, true)?;
    let output = session.scan_output().unwrap().clone();
    let rotated = input.rotated(1)?;
    assert_eq!(
        Simulator::new(&data(), p.clone())?
            .process(&rotated, Tap::RgbOut)?
            .data,
        output.data
    );
    std::fs::remove_file(&path)?;
    p.workflow.saving_color_space = Some("Display P3".into());
    p.workflow.saving_cctf_encoding = true;
    let save = dir.path().join("delivery.npy");
    session.export(&p, &save)?;
    let expected = file_io::convert_color(
        &output,
        &Tables::load(&data())?,
        "sRGB",
        true,
        "Display P3",
        true,
    )?;
    assert_eq!(Image::read_npy(&save)?.data, expected.data);
    assert_eq!(session.scan_output().unwrap().data, output.data);
    p.io.input_rotation = 2;
    assert!(session.export(&p, &save).is_err());
    assert_eq!(Image::read_npy(&save)?.data, expected.data);
    Ok(())
}

#[test]
fn gui_calibration_uses_full_source_and_shared_cli_sampling() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut p = deterministic();
    p.settings.preview_max_size = 4;
    let input = Image::read_npy(
        &data()
            .parent()
            .unwrap()
            .join("tests/fixtures/core/captured.npy"),
    )?;
    let path = dir.path().join("source.npy");
    input.write_npy(&path)?;
    let mut session = Session::new(&data())?;
    session.set_source(Source::load(&path, &data(), &p)?);
    assert_ne!(session.source.as_ref().unwrap().preview.data, input.data);
    let expected = crate::convert::calibrate(&data(), input, &p, false)?;
    let actual = session.calibrate(&p, false)?;
    assert_eq!(
        serde_json::to_string(&actual)?,
        serde_json::to_string(&expected)?
    );
    Ok(())
}

/// Run in its own process with /usr/bin/time to measure the GUI's actual
/// session operations, including setup, display buffers and cache-hit saves.
#[test]
#[ignore = "explicit GUI workflow benchmark; cached crop by default"]
fn benchmark_preview_scan_save() -> Result<()> {
    let input = std::env::var_os("GUI_BENCH_INPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            data()
                .parent()
                .unwrap()
                .join("artifacts/crops/RAW_CANON_EOS_5DMARK3_256.npy")
        });
    let folder = std::env::var_os("GUI_BENCH_DIR")
        .map(PathBuf::from)
        .context("Set GUI_BENCH_DIR for the report")?;
    std::fs::create_dir_all(&folder)?;
    let mode = std::env::var("GUI_BENCH_MODE").unwrap_or_else(|_| "forward".into());
    let mut p = deterministic();
    p.debug.deactivate_spatial_effects = mode != "diffusion";
    p.camera.auto_exposure = false;
    if mode == "convert" {
        p.workflow.route = "input > convert-film > print > scan".into();
    }
    if mode == "diffusion" {
        p.camera.diffusion_filter.active = true;
        p.enlarger.diffusion_filter.active = true;
    }
    p.io.monitor_profile = std::env::var("GUI_BENCH_MONITOR").ok();
    let start = std::time::Instant::now();
    let mut session = Session::new(&data())?;
    let setup = start.elapsed().as_secs_f64();
    let start = std::time::Instant::now();
    session.set_source(Source::load(&input, &data(), &p)?);
    let load = start.elapsed().as_secs_f64();
    let preview = session.render(&p)?;
    let scan = session.render_size(&p, true)?;
    session
        .scan_output()
        .unwrap()
        .write_npy(&folder.join("scan.npy"))?;
    let start = std::time::Instant::now();
    session.export(&p, &folder.join("scan.tiff"))?;
    let save = start.elapsed().as_secs_f64();
    let start = std::time::Instant::now();
    session.export(&p, &folder.join("scan-again.tiff"))?;
    let cache_save = start.elapsed().as_secs_f64();
    let original = session.source.as_ref().unwrap().original;
    let report = serde_json::json!({"mode":mode,"save_format":"TIFF32 ZIP","input":input,"monitor_profile":p.io.monitor_profile,"threads":rayon::current_num_threads(),"shape":original,"session_setup_seconds":setup,"source_load_seconds":load,"preview_seconds":preview.seconds,"scan_seconds":scan.seconds,"save_seconds":save,"cache_hit_save_seconds":cache_save});
    std::fs::write(
        folder.join("workflow.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    eprintln!("{report}");
    Ok(())
}

#[test]
fn preview_respects_configured_edge() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("source.npy");
    Image::new(85, 63, 3, vec![0.184; 85 * 63 * 3])?.write_npy(&path)?;
    let mut p = deterministic();
    p.settings.preview_max_size = 37;
    let mut session = Session::new(&data())?;
    session.set_source(Source::load(&path, &data(), &p)?);
    let preview = session.render(&p)?;
    assert_eq!(preview.before_size, [37, 27]);
    assert_eq!(preview.after_size, [37, 27]);
    Ok(())
}

#[test]
fn export_matches_core_and_keeps_original_resolution() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = Image::new(800, 2, 3, vec![0.18; 800 * 2 * 3])?;
    let source = dir.path().join("INPUT.NPY");
    input.write_npy(&source)?;
    let params = deterministic();
    let mut session = Session::new(&data())?;
    session.source = Some(Source::load(&source, &data(), &params)?);
    let cached = session.source.as_ref().unwrap();
    assert_eq!(cached.original, [800, 2]);
    assert_eq!(cached.preview.width, 640);
    let preview = session.render(&params)?;
    assert_eq!(preview.after_size, [640, 1]);
    assert_eq!(session.render_size(&params, true)?.after_size, [800, 2]);
    let path = dir.path().join("result.npy");
    assert_eq!(session.export(&params, &path)?, [800, 2]);
    let exported = Image::read_npy(&path)?;
    let direct = Simulator::new(&data(), params.clone())?.process_owned(input, Tap::RgbOut)?;
    assert_eq!(exported.data, direct.data);
    assert_eq!(exported.width, 800);
    // A failed export neither truncates its source nor creates an output file.
    let bytes = std::fs::read(&source)?;
    assert!(session.export(&params, &source).is_err());
    assert_eq!(std::fs::read(&source)?, bytes);
    let unsupported = dir.path().join("result.bad");
    assert!(session.export(&params, &unsupported).is_err());
    assert!(!unsupported.exists());
    let mut invalid = params.clone();
    invalid.workflow.route = "unsupported".into();
    let preserved = std::fs::read(&path)?;
    assert!(session.export(&invalid, &path).is_err());
    assert_eq!(std::fs::read(&path)?, preserved);
    let png = dir.path().join("result.png");
    session.export(&params, &png)?;
    let png = image::open(png)?.to_rgb16();
    assert_eq!(png.dimensions(), (800, 2));
    assert_eq!(
        png.as_raw(),
        &direct
            .data
            .iter()
            .map(|v| (v.clamp(0., 1.) * 65535.) as u16)
            .collect::<Vec<_>>()
    );
    let tiff = dir.path().join("result.tiff");
    session.export(&params, &tiff)?;
    let mut decoder =
        tiff::decoder::Decoder::new(std::io::BufReader::new(std::fs::File::open(tiff)?))?;
    assert_eq!(decoder.dimensions()?, (800, 2));
    let tiff::decoder::DecodingResult::F32(values) = decoder.read_image()? else {
        panic!("Expected float TIFF");
    };
    assert_eq!(
        values,
        direct.data.iter().map(|v| *v as f32).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn scan_export_reuses_pixels_without_reading_source_again() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("photo.npy");
    let input = Image::read_npy(&fixture().join("input.npy"))?;
    input.write_npy(&source)?;
    let mut params = Params::default();
    params.io.copy_metadata = false;
    params.debug.deactivate_spatial_effects = true;
    // Leave stochastic grain enabled: export must preserve this exact scan.
    let mut session = Session::new(&data())?;
    session.set_source(Source::load(&source, &data(), &params)?);
    let scan = session.render_size(&params, true)?;
    let resolved = session.resolved_params(&params)?;
    session.render(&resolved)?; // A subsequent preview must not replace the scan.
    std::fs::remove_file(&source)?;
    // A matching Save must not even rebuild the simulator or reload its tables.
    let processing_data = std::mem::replace(&mut session.data, dir.path().join("missing-data"));
    let output = dir.path().join("scan.npy");
    session.export(&resolved, &output)?;
    let exported = Image::read_npy(&output)?;
    assert_eq!(
        backend::display_rgb(
            &exported,
            &params.io.output_color_space,
            params.io.output_cctf_encoding,
            &Tables::load(&data())?
        )?,
        scan.after
    );
    let mut display_only = resolved.clone();
    display_only.io.monitor_profile = Some("missing-monitor.icc".into());
    session.export(&display_only, &dir.path().join("second.npy"))?;
    assert_eq!(
        Image::read_npy(&dir.path().join("second.npy"))?.data,
        exported.data
    );
    session.data = processing_data;
    // A real processing edit requires a new scan, which cannot read a removed source.
    let mut edited = resolved;
    edited.camera.exposure_compensation_ev += 1.;
    assert!(
        session
            .export(&edited, &dir.path().join("changed.npy"))
            .is_err()
    );
    assert!(!dir.path().join("changed.npy").exists());
    Ok(())
}

#[test]
fn preview_export_and_source_changes_invalidate_scan() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("photo.npy");
    let input = Image::new(800, 2, 3, vec![0.18; 800 * 2 * 3])?;
    input.write_npy(&source)?;
    let mut params = deterministic();
    let mut session = Session::new(&data())?;
    session.set_source(Source::load(&source, &data(), &params)?);
    session.render(&params)?;
    let first = dir.path().join("first.npy");
    assert_eq!(session.export(&params, &first)?, [800, 2]);
    let original = Image::read_npy(&first)?;
    params.camera.exposure_compensation_ev += 1.;
    session.render(&params)?;
    let edited = dir.path().join("edited.npy");
    session.export(&params, &edited)?;
    let direct = Simulator::new(&data(), params.clone())?.process_owned(input, Tap::RgbOut)?;
    assert_eq!(Image::read_npy(&edited)?.data, direct.data);
    assert_ne!(original.data, direct.data);
    // Reloading even the same path must discard the previous source's scan.
    Image::new(4, 2, 3, vec![0.36; 4 * 2 * 3])?.write_npy(&source)?;
    session.set_source(Source::load(&source, &data(), &params)?);
    assert_eq!(
        session.export(&params, &dir.path().join("reloaded.npy"))?,
        [4, 2]
    );
    Ok(())
}

#[test]
fn failed_monitor_display_preserves_completed_scan() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("photo.npy");
    Image::read_npy(&fixture().join("input.npy"))?.write_npy(&source)?;
    let mut params = Params::default();
    params.io.copy_metadata = false;
    params.debug.deactivate_spatial_effects = true;
    let mut session = Session::new(&data())?;
    session.set_source(Source::load(&source, &data(), &params)?);
    session.render_size(&params, true)?;
    let saved = dir.path().join("saved.npy");
    session.export(&params, &saved)?;
    let original = Image::read_npy(&saved)?;
    params.io.monitor_profile = Some(dir.path().join("missing.icc").to_string_lossy().into());
    assert!(session.render_size(&params, true).is_err());
    std::fs::write(dir.path().join("missing.icc"), b"invalid ICC profile")?;
    assert!(session.render_size(&params, true).is_err());
    std::fs::remove_file(&source)?;
    session.export(&params, &saved)?;
    assert_eq!(Image::read_npy(&saved)?.data, original.data);
    Ok(())
}

#[test]
fn image_filters_include_uppercase_camera_extensions() {
    let extensions = extensions_with_case(RAW_EXTENSIONS);
    for ext in [
        "CR2", "CR3", "NEF", "ARW", "RAF", "DNG", "RW2", "IIQ", "CRW",
    ] {
        assert!(extensions.iter().any(|v| v == ext));
        assert!(extensions.iter().any(|v| v == &ext.to_ascii_lowercase()));
    }
    assert!(
        extensions_with_case(RGB_EXTENSIONS)
            .iter()
            .any(|v| v == "TIFF")
    );
}

#[test]
fn preview_preserves_full_frame_and_display_converts_colour_space() -> Result<()> {
    let image = Image::new(
        4,
        2,
        3,
        vec![
            0., 0., 0., 0., 0., 0., 1., 1., 1., 1., 1., 1., 0., 0., 0., 0., 0., 0., 1., 1., 1., 1.,
            1., 1.,
        ],
    )?;
    let preview = backend::resize_preview(&image, 2)?;
    assert_eq!((preview.width, preview.height), (2, 1));
    assert!(preview.data[0] > 0. && preview.data[0] < 0.1);
    assert!(preview.data[3] > 0.9 && preview.data[3] < 1.);
    let tables = Tables::load(&data())?;
    let linear = Image::new(1, 1, 3, vec![0.18; 3])?;
    assert_eq!(
        backend::display_rgb(&linear, "sRGB", false, &tables)?,
        vec![118; 3]
    );
    let encoded = Image::new(1, 1, 3, vec![118. / 255.; 3])?;
    assert_eq!(
        backend::display_rgb(&encoded, "sRGB", true, &tables)?,
        vec![118; 3]
    );
    let mut p = deterministic();
    p.io.output_color_space = "ITU-R BT.2020".into();
    p.io.output_cctf_encoding = false;
    let mut session = Session::new(&data())?;
    session.source = Some(Source::load(&fixture().join("input.npy"), &data(), &p)?);
    assert!(!session.render(&p)?.after.is_empty());
    Ok(())
}

#[test]
fn settings_round_trip_without_quantizing_hidden_fields() -> Result<()> {
    let mut p = deterministic();
    p.halation.scatter_core_um[0] = 2.123456789012345;
    p.camera.exposure_compensation_ev = 0.123456789012345;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("settings.json");
    save_params(&p, &path)?;
    let mut restored = read_params(&path)?;
    assert_eq!(serde_json::to_value(&p)?, serde_json::to_value(&restored)?);
    restored.enlarger.y_filter_shift = 12.5;
    restored.grain.active = false;
    assert_eq!(
        restored.camera.exposure_compensation_ev,
        p.camera.exposure_compensation_ev
    );
    assert_eq!(
        restored.halation.scatter_core_um,
        p.halation.scatter_core_um
    );
    assert_eq!(restored.enlarger.y_filter_shift, 12.5);
    assert!(!restored.grain.active);
    assert!(serde_json::from_str::<Params>(r#"{"unknown":1}"#).is_err());
    let (film, paper) = profiles(&data())?;
    assert!(film.iter().any(|s| s.id == p.film_profile));
    assert!(paper.iter().any(|s| s.id == p.print_profile));
    Ok(())
}

// This explicitly opt-in check uses a real RAW only in local QA. It does not
// require camera samples or a display server in the normal test suite.
#[test]
#[ignore = "requires explicit GUI_QA_RAW sample"]
fn raw_preview_matches_full_decode_reference_resize() -> Result<()> {
    let Some(path) = std::env::var_os("GUI_QA_RAW") else {
        anyhow::bail!("Set GUI_QA_RAW to a local camera sample");
    };
    let path = PathBuf::from(path);
    let (preview, original) = decode_raw_preview(&path, true, 640)?;
    let full = decode_raw(&path, false, true)?;
    assert_eq!(original, [full.width, full.height]);
    let expected = backend::resize_preview(&full, 640)?;
    assert_eq!(preview.data, expected.data);
    Ok(())
}

#[test]
fn offscreen_darkroom_callbacks_and_layout() -> Result<()> {
    type UiEvent = Box<dyn FnOnce() + Send>;
    struct Proxy(mpsc::Sender<UiEvent>);
    impl EventLoopProxy for Proxy {
        fn quit_event_loop(&self) -> std::result::Result<(), slint::EventLoopError> {
            Ok(())
        }
        fn invoke_from_event_loop(
            &self,
            event: UiEvent,
        ) -> std::result::Result<(), slint::EventLoopError> {
            self.0
                .send(event)
                .map_err(|_| slint::EventLoopError::EventLoopTerminated)
        }
    }
    struct Offscreen {
        window: Rc<MinimalSoftwareWindow>,
        events: mpsc::Sender<UiEvent>,
    }
    impl Platform for Offscreen {
        fn create_window_adapter(
            &self,
        ) -> std::result::Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(self.window.clone())
        }
        fn new_event_loop_proxy(&self) -> Option<Box<dyn EventLoopProxy>> {
            Some(Box::new(Proxy(self.events.clone())))
        }
    }
    let (events, event_receiver) = mpsc::channel();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Offscreen {
        window: window.clone(),
        events,
    }))?;
    let ui = Darkroom::new()?;
    let (films, papers) = profiles(&data())?;
    let (sender, receiver) = mpsc::channel();
    let state = Rc::new(RefCell::new(State {
        params: deterministic(),
        path: Some(fixture().join("input.npy")),
        source_generation: 0,
        films,
        papers,
        filters: Tables::load(&data())?
            .camera_filters
            .keys()
            .cloned()
            .collect(),
        sections: initial_sections(),
        revision: Arc::new(AtomicU64::new(0)),
        sender,
        debounce: Timer::default(),
        dialog_timer: Timer::default(),
        dialog_pending: false,
    }));
    sync(&ui, &state.borrow());
    bind_callbacks(&ui, &state);
    ui.set_auto_preview(false);
    // A blocked chooser must return control to Slint immediately. Deliver the
    // selection and cancellation through the same timer used by native dialogs.
    let (release, waiting) = mpsc::channel();
    let main_thread = std::thread::current().id();
    choose_file(
        &ui,
        &state,
        move || {
            assert_ne!(std::thread::current().id(), main_thread);
            waiting.recv().unwrap()
        },
        |ui, _, path| ui.set_status(path.to_string_lossy().into_owned().into()),
    );
    assert!(state.borrow().dialog_pending);
    assert!(ui.get_dialog_open());
    ui.invoke_tab_selected(1);
    ui.set_status("UI is responsive".into());
    assert_eq!(ui.get_status().as_str(), "UI is responsive");
    choose_file(&ui, &state, || panic!("Duplicate chooser"), |_, _, _| {});
    release.send(Some(PathBuf::from("selected.CR2")))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while state.borrow().dialog_pending {
        ensure!(
            std::time::Instant::now() < deadline,
            "Dialog completion timed out"
        );
        std::thread::sleep(Duration::from_millis(35));
        slint::platform::update_timers_and_animations();
    }
    assert_eq!(ui.get_status().as_str(), "selected.CR2");
    assert!(!ui.get_dialog_open());
    choose_file(
        &ui,
        &state,
        || None,
        |_, _, _| panic!("Cancellation selected a file"),
    );
    while state.borrow().dialog_pending {
        ensure!(
            std::time::Instant::now() < deadline,
            "Dialog cancellation timed out"
        );
        std::thread::sleep(Duration::from_millis(35));
        slint::platform::update_timers_and_animations();
    }
    assert_eq!(ui.get_status().as_str(), "selected.CR2");

    // Open every section and require one editor for every photo parameter.
    for tab in 0..5 {
        loop {
            let mut opened = false;
            let headers = inspector::fields(&state.borrow(), tab);
            for f in headers {
                if f.kind == 3 {
                    opened |= state.borrow_mut().sections.insert(f.key.to_string());
                }
            }
            if !opened {
                break;
            }
        }
    }
    let mut keys = HashSet::new();
    let mut missing_help = Vec::new();
    for tab in 0..5 {
        let mut fields = inspector::fields(&state.borrow(), tab);
        if tab == 0 {
            fields.push(inspector::workflow(&state.borrow()));
        }
        for field in fields {
            if field.kind != 3 {
                if field.tooltip.is_empty() {
                    missing_help.push(field.key.to_string());
                }
                assert!(
                    keys.insert(field.key.to_string()),
                    "Duplicate control {}",
                    field.key
                );
            }
        }
    }
    assert!(missing_help.is_empty(), "Missing help for {missing_help:?}");
    fn leaves(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        if path.starts_with("/special/") {
            out.push(path.into());
            return;
        }
        match v {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    leaves(v, &format!("{path}/{k}"), out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    leaves(v, &format!("{path}/{i}"), out);
                }
            }
            _ => out.push(path.into()),
        }
    }
    let mut expected = Vec::new();
    leaves(
        &serde_json::to_value(&state.borrow().params)?,
        "",
        &mut expected,
    );
    for key in expected {
        if ![
            "/io/monitor_profile",
            "/io/input_gamut_compress/algorithm",
            "/enlarger/lens_blur",
            "/print_base/tilt",
            "/debug/lut_mode",
        ]
        .contains(&key.as_str())
        {
            assert!(keys.contains(&key), "Missing editor for {key}");
        }
    }
    let advanced = inspector::fields(&state.borrow(), 3);
    assert_eq!(
        advanced
            .iter()
            .filter(|f| f.kind == 3)
            .map(|f| f.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Spectral upsampling",
            "Input gamut compress",
            "Output gamut compress",
            "Experimental",
            "Negative conversion"
        ]
    );
    assert!(
        advanced
            .iter()
            .any(|f| f.key == "/settings/rgb_to_raw_method")
    );
    assert!(advanced.iter().any(|f| f.key == "/enlarger/illuminant"));
    assert!(
        !inspector::fields(&state.borrow(), 4)
            .iter()
            .any(|f| f.key == "/settings/rgb_to_raw_method")
    );
    assert_eq!(
        presentation::tooltip("/camera/exposure_compensation_ev"),
        "Add a bias to the auto-exposure of the camera."
    );
    assert!(
        presentation::tooltip("/io/input_gamut_compress/hull_detail")
            .contains("larger means LESS smoothing")
    );
    assert_eq!(
        presentation::tooltip("/convert/calibration/0/1"),
        presentation::tooltip("/convert/calibration")
    );
    // Wheel editing follows the reference increments, clamps its editor bounds,
    // and resolves choices without losing precision in direct text entry.
    ui.invoke_field_stepped("/camera/exposure_compensation_ev".into(), 1);
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.25);
    ui.invoke_field_stepped("/raw/temperature".into(), 1);
    assert_eq!(state.borrow().params.raw.temperature, 5600.);
    ui.invoke_field_stepped("/io/crop_center/0".into(), 1000);
    assert_eq!(state.borrow().params.io.crop_center[0], 1.);
    ui.invoke_field_stepped("/camera/auto_exposure".into(), -1);
    assert!(!state.borrow().params.camera.auto_exposure);
    ui.invoke_field_stepped("/raw/white_balance".into(), -1);
    assert_eq!(state.borrow().params.raw.white_balance, "daylight");
    let revision = state.borrow().revision.load(Ordering::SeqCst);
    ui.invoke_field_edited("/raw/temperature".into(), "5600".into());
    assert_eq!(state.borrow().revision.load(Ordering::SeqCst), revision);
    state
        .borrow_mut()
        .params
        .io
        .output_gamut_compress
        .lightness_compression = Some([0.8, 1.2, 2.]);
    ui.invoke_field_stepped(
        "/io/output_gamut_compress/lightness_compression/enabled".into(),
        1,
    );
    assert_eq!(
        state
            .borrow()
            .params
            .io
            .output_gamut_compress
            .lightness_compression,
        Some([0.8, 1.2, 2.])
    );
    assert_eq!(state.borrow().revision.load(Ordering::SeqCst), revision);
    state.borrow_mut().params = deterministic();
    state.borrow_mut().sections = initial_sections();
    ui.invoke_field_edited("/camera/exposure_compensation_ev".into(), "1.25".into());
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 1.25);
    assert!(ui.get_stale());
    ui.invoke_field_edited("/grain/active".into(), "false".into());
    assert!(!state.borrow().params.grain.active);
    assert!(!state.borrow().params.settings.apply_grain_presets);
    ui.invoke_field_edited("/workflow/route".into(), "Film → Scan".into());
    assert_eq!(state.borrow().params.workflow.route, "input > film > scan");
    ui.invoke_field_edited("/camera/exposure_compensation_ev".into(), "bad".into());
    assert!(ui.get_error());
    assert_eq!(state.borrow().params.workflow.route, "input > film > scan");
    ui.invoke_preview();
    let request = receiver.try_recv()?;
    assert_eq!(
        request.revision,
        state.borrow().revision.load(Ordering::SeqCst)
    );
    assert!(matches!(request.job, Job::Preview { .. }));
    assert_eq!(ui.get_display_status(), "Developing preview…");
    ui.invoke_field_edited("/camera/exposure_compensation_ev".into(), "bad".into());
    assert!(ui.get_display_status().starts_with("Developing preview…"));
    assert!(ui.get_display_status().contains("Enter a number"));
    ui.set_error(false);
    ui.set_status("Startup settings saved.".into());
    assert_eq!(ui.get_display_status(), "Developing preview…");
    ui.set_busy(false);
    ui.invoke_calibrated(serde_json::to_string(&deterministic())?.into());
    assert!(!ui.get_error());
    assert_eq!(state.borrow().params.workflow.route, backend::ROUTES[0]);
    // Exercise the actual worker and UI handoff. Its first completed render
    // becomes obsolete while queued in the UI event loop, then a new request
    // succeeds after a decode failure.
    let weak = ui.as_weak();
    let revision = state.borrow().revision.clone();
    let session = Session::new(&data())?;
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build()?;
    let thread = std::thread::spawn(move || worker(session, receiver, revision, weak, pool));
    let obsolete = event_receiver.recv_timeout(Duration::from_secs(60))?;
    state.borrow().revision.fetch_add(1, Ordering::SeqCst);
    ui.set_status("Sentinel for obsolete result".into());
    obsolete();
    assert_eq!(ui.get_status().as_str(), "Sentinel for obsolete result");
    state.borrow_mut().path = Some(fixture().join("missing.npy"));
    submit(&ui, &state);
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    assert!(ui.get_error());
    assert!(!ui.get_busy());
    state.borrow_mut().path = Some(fixture().join("input.npy"));
    submit(&ui, &state);
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    assert!(!ui.get_error());
    assert!(ui.get_has_image());
    assert!(!ui.get_stale());
    assert_eq!(ui.get_display_status(), "Ready");
    ui.invoke_scan();
    assert!(ui.get_busy());
    assert_eq!(ui.get_display_status(), "Scanning at full resolution…");
    ui.set_status("Ready".into());
    assert_eq!(ui.get_display_status(), "Scanning at full resolution…");
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    assert!(!ui.get_busy());
    assert_eq!(ui.get_display_status(), "Ready");
    ui.set_auto_preview(true);
    ui.invoke_field_stepped("/camera/exposure_compensation_ev".into(), 1);
    assert!(ui.get_busy());
    assert_eq!(ui.get_display_status(), "Preview queued…");
    // Submit immediately to avoid waiting for the debounce in this worker test.
    submit(&ui, &state);
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    ui.set_auto_preview(false);
    // Reopening the same path must reload it even when a newer preview makes
    // the opening request obsolete before the worker can process it.
    let reload_dir = tempfile::tempdir()?;
    let reload_path = reload_dir.path().join("reopen.npy");
    Image::new(4, 2, 3, vec![0.18; 24])?.write_npy(&reload_path)?;
    let mut reload_session = Session::new(&data())?;
    reload_session.set_source(Source::load(&reload_path, &data(), &state.borrow().params)?);
    state.borrow_mut().path = Some(reload_path.clone());
    Image::new(5, 2, 3, vec![0.36; 30])?.write_npy(&reload_path)?;
    let (reload_sender, reload_receiver) = mpsc::channel();
    let original_sender = std::mem::replace(&mut state.borrow_mut().sender, reload_sender);
    // Queue both revisions before starting the worker to guarantee that the
    // opening request is discarded while the same-path source is cached.
    submit_request(&ui, &state, false, true);
    submit(&ui, &state);
    state.borrow_mut().sender = original_sender;
    let reload_weak = ui.as_weak();
    let reload_revision = state.borrow().revision.clone();
    let reload_pool = processing_pool(2)?;
    let reload_thread = std::thread::spawn(move || {
        worker(
            reload_session,
            reload_receiver,
            reload_revision,
            reload_weak,
            reload_pool,
        )
    });
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    reload_thread.join().expect("Reload worker exits");
    assert_eq!(ui.get_dimensions().as_str(), "5 × 2");
    state.borrow_mut().path = Some(fixture().join("input.npy"));
    // Numeric entry keeps float64 precision, and changing tabs cannot duplicate controls.
    ui.invoke_field_edited(
        "/camera/exposure_compensation_ev".into(),
        "0.123456789012345".into(),
    );
    assert_eq!(
        state.borrow().params.camera.exposure_compensation_ev,
        0.123456789012345
    );
    ui.invoke_calibrated(serde_json::to_string(&deterministic())?.into());
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.);
    ui.show()?;
    window.set_size(slint::PhysicalSize::new(1240, 850));
    let _ = ui.window().take_snapshot()?;
    // The tab strip is exercised through real pointer events.
    let position = slint::LogicalPosition::new(1150., 26.);
    window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    assert_eq!(ui.get_tab(), 4);
    ui.set_tab(0);
    ui.invoke_tab_selected(0);
    let mut session = Session::new(&data())?;
    // Optional QA uses the actual camera sample for screenshots, otherwise
    // a small checked-in numerical fixture keeps this test portable.
    let path = std::env::var_os("GUI_QA_RAW")
        .map(PathBuf::from)
        .unwrap_or_else(|| fixture().join("input.npy"));
    session.source = Some(Source::load(&path, &data(), &deterministic())?);
    let preview = session.render(&deterministic())?;
    ui.set_before(pixels(&preview.before, preview.before_size));
    ui.set_after(pixels(&preview.after, preview.after_size));
    ui.set_has_image(true);
    ui.set_stale(false);
    ui.set_busy(false);
    ui.set_filename(
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    let original = session.source.as_ref().unwrap().original;
    ui.set_dimensions(format!("{} × {}", original[0], original[1]).into());
    ui.set_status("Ready".into());
    ui.set_render_info(
        format!(
            "{} × {} · {:.2} s",
            preview.after_size[0], preview.after_size[1], preview.seconds
        )
        .into(),
    );
    ui.show()?;
    let qa_dir = std::env::var_os("GUI_QA_DIR").map(PathBuf::from);
    if let Some(dir) = &qa_dir {
        std::fs::create_dir_all(dir)?;
    }
    use slint::platform::{PointerEventButton, WindowEvent};
    fn move_to(window: &MinimalSoftwareWindow, x: f32, y: f32) {
        window.dispatch_event(WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(x, y),
        });
    }
    fn scroll(window: &MinimalSoftwareWindow, x: f32, y: f32, delta: f32) {
        move_to(window, x, y);
        window.dispatch_event(WindowEvent::PointerScrolled {
            position: slint::LogicalPosition::new(x, y),
            delta_x: 0.,
            delta_y: delta,
        });
    }
    // Real input events, including zoom from Fit, pointer anchoring, drag and
    // shared Compare navigation. Navigation never submits a processing request.
    window.set_size(slint::PhysicalSize::new(1240, 850));
    let navigation_photo = pixels(&vec![100; 400 * 300 * 3], [400, 300]);
    ui.set_before(navigation_photo.clone());
    ui.set_after(navigation_photo);
    ui.set_view_mode(1);
    ui.invoke_reset_view(1.);
    let _ = ui.window().take_snapshot()?;
    let revision = state.borrow().revision.load(Ordering::SeqCst);
    scroll(&window, 200., 200., 60.);
    assert!((ui.get_zoom() - 1.2).abs() < 1e-5);
    // Canvas center is (403,389); the same image point remains under (200,200).
    assert!((ui.get_pan_x() - 40.6).abs() < 0.1);
    assert!((ui.get_pan_y() - 37.8).abs() < 0.1);
    scroll(&window, 200., 200., -60.);
    assert!((ui.get_zoom() - 1.).abs() < 1e-5);
    assert!(ui.get_pan_x().abs() < 0.01 && ui.get_pan_y().abs() < 0.01);
    window.dispatch_event(WindowEvent::PointerPressed {
        position: slint::LogicalPosition::new(200., 200.),
        button: PointerEventButton::Left,
    });
    move_to(&window, 245., 225.);
    window.dispatch_event(WindowEvent::PointerReleased {
        position: slint::LogicalPosition::new(245., 225.),
        button: PointerEventButton::Left,
    });
    assert!((ui.get_pan_x() - 45.).abs() < 0.01);
    assert!((ui.get_pan_y() - 25.).abs() < 0.01);
    // Slint only initializes pressed-x/y for the left button. Other button
    // grabs must not use the coordinates left over from the preceding drag.
    let pan = (ui.get_pan_x(), ui.get_pan_y());
    for button in [PointerEventButton::Middle, PointerEventButton::Right] {
        window.dispatch_event(WindowEvent::PointerPressed {
            position: slint::LogicalPosition::new(300., 300.),
            button,
        });
        move_to(&window, 345., 325.);
        window.dispatch_event(WindowEvent::PointerReleased {
            position: slint::LogicalPosition::new(345., 325.),
            button,
        });
        assert_eq!((ui.get_pan_x(), ui.get_pan_y()), pan);
    }
    ui.set_view_mode(2);
    let _ = ui.window().take_snapshot()?;
    scroll(&window, 600., 200., 60.);
    assert!((ui.get_zoom() - 1.2).abs() < 1e-5);
    ui.invoke_reset_view(0.);
    assert_eq!((ui.get_pan_x(), ui.get_pan_y()), (0., 0.));
    ui.set_view_mode(1);
    let _ = ui.window().take_snapshot()?;
    scroll(&window, 200., 200., 60.);
    let fit_zoom = ui.get_zoom();
    assert!(fit_zoom > 0.);
    scroll(&window, 200., 200., -60.);
    assert!((ui.get_zoom() - fit_zoom / 1.2).abs() < 1e-5);
    assert_eq!(state.borrow().revision.load(Ordering::SeqCst), revision);
    ui.invoke_reset_view(0.);

    // Exercise real MAIN rows so successful edits refresh the same inspector.
    // Fractional wheel input must survive those refreshes, in both directions
    // and when another parameter is edited between events.
    ui.set_tab(0);
    state.borrow_mut().params = deterministic();
    state.borrow_mut().sections = initial_sections();
    sync(&ui, &state.borrow());
    let _ = ui.window().take_snapshot()?;
    scroll(&window, 1100., 280., 100.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.25);
    scroll(&window, 1100., 280., 20.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.5);
    scroll(&window, 1100., 280., -100.);
    let _ = ui.window().take_snapshot()?;
    scroll(&window, 1100., 280., -20.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.);
    scroll(&window, 1100., 280., 20.);
    scroll(&window, 1100., 350., 60.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.film_format_mm, 36.);
    scroll(&window, 1100., 280., 40.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.25);

    fn replace_editor_text(window: &MinimalSoftwareWindow, text: &str) {
        let position = slint::LogicalPosition::new(1100., 280.);
        move_to(window, position.x, position.y);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::KeyPressed {
            text: slint::platform::Key::Control.into(),
        });
        window.dispatch_event(WindowEvent::KeyPressed { text: "a".into() });
        window.dispatch_event(WindowEvent::KeyReleased { text: "a".into() });
        window.dispatch_event(WindowEvent::KeyReleased {
            text: slint::platform::Key::Control.into(),
        });
        window.dispatch_event(WindowEvent::KeyPressed { text: text.into() });
        window.dispatch_event(WindowEvent::KeyReleased { text: text.into() });
    }
    assert!(ui.invoke_field_edited("/camera/exposure_compensation_ev".into(), "1.25".into()));
    let _ = ui.window().take_snapshot()?;
    replace_editor_text(&window, "2.75");
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 1.25);
    scroll(&window, 1100., 280., 60.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 3.);
    scroll(&window, 1100., 280., 60.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 3.25);
    // Invalid pending text stays editable and must not step the old value or
    // submit processing. A corrected draft can be stepped immediately.
    replace_editor_text(&window, "bad");
    let revision = state.borrow().revision.load(Ordering::SeqCst);
    scroll(&window, 1100., 280., 60.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 3.25);
    assert_eq!(state.borrow().revision.load(Ordering::SeqCst), revision);
    assert!(ui.get_error());
    assert!(ui.get_status().contains("Enter a number"));
    replace_editor_text(&window, "4");
    scroll(&window, 1100., 280., 60.);
    let _ = ui.window().take_snapshot()?;
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 4.25);
    assert!(!ui.get_error());

    // Place one real editor at a known location to exercise wheel delivery and
    // delayed help without relying on which sections the user has expanded.
    ui.set_tab(1);
    state.borrow_mut().params = deterministic();
    state.borrow_mut().sections.insert("0:RAW".into());
    let camera_field = inspector::fields(&state.borrow(), 0)
        .into_iter()
        .find(|f| f.key == "/camera/exposure_compensation_ev")
        .unwrap();
    ui.set_fields(Rc::new(VecModel::from(vec![camera_field.clone()])).into());
    let _ = ui.window().take_snapshot()?;
    scroll(&window, 1100., 68., 20.);
    scroll(&window, 1100., 68., 20.);
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.);
    scroll(&window, 1100., 68., 20.);
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.25);
    let balance_field = inspector::fields(&state.borrow(), 0)
        .into_iter()
        .find(|f| f.key == "/raw/white_balance")
        .unwrap();
    ui.set_fields(Rc::new(VecModel::from(vec![balance_field])).into());
    let _ = ui.window().take_snapshot()?;
    scroll(&window, 1100., 68., -60.);
    assert_eq!(state.borrow().params.raw.white_balance, "daylight");
    let checkbox = inspector::fields(&state.borrow(), 0)
        .into_iter()
        .find(|f| f.key == "/camera/auto_exposure")
        .unwrap();
    ui.set_fields(Rc::new(VecModel::from(vec![checkbox])).into());
    let _ = ui.window().take_snapshot()?;
    if let Some(dir) = &qa_dir {
        let snapshot = ui.window().take_snapshot()?;
        image::save_buffer_with_format(
            dir.join("checkbox-qa.png"),
            snapshot.as_bytes(),
            1240,
            850,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
    }
    let check_position = slint::LogicalPosition::new(1030., 68.);
    move_to(&window, check_position.x, check_position.y);
    window.dispatch_event(WindowEvent::PointerPressed {
        position: check_position,
        button: PointerEventButton::Left,
    });
    window.dispatch_event(WindowEvent::PointerReleased {
        position: check_position,
        button: PointerEventButton::Left,
    });
    assert!(
        !state.borrow().params.camera.auto_exposure,
        "Normal checkbox clicks must pass through the wheel handler"
    );
    ui.set_fields(Rc::new(VecModel::from(vec![camera_field])).into());
    let _ = ui.window().take_snapshot()?;
    move_to(&window, 900., 68.);
    let before_help = ui.window().take_snapshot()?;
    std::thread::sleep(Duration::from_millis(550));
    slint::platform::update_timers_and_animations();
    let help = ui.window().take_snapshot()?;
    let changed_pixels = |before: &SharedPixelBuffer<slint::Rgba8Pixel>,
                          after: &SharedPixelBuffer<slint::Rgba8Pixel>| {
        let points: Vec<_> = before
            .as_slice()
            .iter()
            .zip(after.as_slice())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| (i as u32 % after.width(), i as u32 / after.width()))
            .collect();
        let min_x = points.iter().map(|p| p.0).min().unwrap_or(0);
        let max_x = points.iter().map(|p| p.0).max().unwrap_or(0);
        assert!(
            max_x - min_x < 500,
            "Tooltip must stay within a readable width"
        );
        points.len()
    };
    let short_changes = changed_pixels(&before_help, &help);
    assert!(short_changes > 200, "Tooltip must appear after hovering");
    if let Some(dir) = &qa_dir {
        image::save_buffer_with_format(
            dir.join("tooltip.png"),
            help.as_bytes(),
            1240,
            850,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
    }
    move_to(&window, 100., 100.);
    let hull_field = Field {
        key: "/io/input_gamut_compress/hull_detail".into(),
        label: "Hull detail".into(),
        value: "5.0".into(),
        tooltip: presentation::wrapped_tooltip("/io/input_gamut_compress/hull_detail").into(),
        wheel: true,
        depth: 1,
        ..Default::default()
    };
    ui.set_fields(Rc::new(VecModel::from(vec![hull_field])).into());
    let _ = ui.window().take_snapshot()?;
    move_to(&window, 1100., 68.);
    let before_long_help = ui.window().take_snapshot()?;
    std::thread::sleep(Duration::from_millis(550));
    slint::platform::update_timers_and_animations();
    let long_help = ui.window().take_snapshot()?;
    if let Some(dir) = &qa_dir {
        image::save_buffer_with_format(
            dir.join("tooltip-long.png"),
            long_help.as_bytes(),
            1240,
            850,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )?;
    }
    assert!(
        changed_pixels(&before_long_help, &long_help) > short_changes,
        "Long help must wrap and grow vertically"
    );
    move_to(&window, 100., 100.);
    ui.set_tab(0);
    ui.invoke_tab_selected(0);
    ui.set_before(pixels(&preview.before, preview.before_size));
    ui.set_after(pixels(&preview.after, preview.after_size));
    // Both compact rotation icons remain in the preview footer on every tab.
    for (width, height, x, y) in [(1240, 850, 501., 792.), (980, 700, 241., 642.)] {
        window.set_size(slint::PhysicalSize::new(width, height));
        ui.set_tab(2);
        ui.invoke_tab_selected(2);
        let _ = ui.window().take_snapshot()?;
        let position = slint::LogicalPosition::new(x, y);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(state.borrow().params.io.input_rotation, 3);
        let position = slint::LogicalPosition::new(x + 49., y);
        window.dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        window.dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(state.borrow().params.io.input_rotation, 0);
    }
    assert_eq!(ui.get_zoom(), 0.);
    state.borrow_mut().params = deterministic();
    ui.set_stale(false);
    ui.set_status("Ready".into());
    for (w, h, name, view, has_image, tab) in [
        (1240, 850, "developed", 1, true, 0),
        (1240, 850, "comparison", 2, true, 0),
        (980, 700, "compact", 1, true, 0),
        (1240, 850, "empty", 1, false, 0),
        (1240, 850, "film", 1, true, 1),
        (1240, 850, "print", 1, true, 2),
        (1240, 850, "advanced", 1, true, 3),
        (980, 700, "config", 1, true, 4),
    ] {
        ui.set_tab(tab);
        ui.invoke_tab_selected(tab);
        ui.set_has_image(has_image);
        ui.set_view_mode(view);
        window.set_size(slint::PhysicalSize::new(w, h));
        let snapshot = ui.window().take_snapshot()?;
        assert_eq!((snapshot.width(), snapshot.height()), (w, h));
        assert!(snapshot.as_slice().iter().any(|p| p.r != 0));
        if let Some(dir) = &qa_dir {
            image::save_buffer_with_format(
                dir.join(format!("{name}.png")),
                snapshot.as_bytes(),
                w,
                h,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )?;
        }
    }
    ui.hide()?;
    // Weak callback captures must release the state when the window closes.
    let count = Rc::strong_count(&state);
    assert!(count > 1);
    drop(ui);
    assert_eq!(Rc::strong_count(&state), 1);
    drop(state);
    thread
        .join()
        .expect("Worker exits when the UI releases its sender");
    Ok(())
}

#[test]
fn rgb_input_cache_and_monitor_are_independent_of_export() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("photo.png");
    image::RgbImage::from_raw(2, 1, vec![32, 64, 128, 200, 140, 80])
        .unwrap()
        .save(&source)?;
    let mut p = deterministic();
    p.workflow.route = "input".into();
    p.io.input_color_space = "sRGB".into();
    p.io.input_cctf_decoding = true;
    p.io.output_gamut_compress.algorithm = "off".into();
    let mut session = Session::new(&data())?;
    session.source = Some(Source::load(&source, &data(), &p)?);
    let original = session.render(&p)?;
    assert_eq!(original.before, vec![32, 64, 128, 200, 140, 80]);
    p.raw.temperature = 6100.;
    assert!(!session.source.as_ref().unwrap().matches(&source, &p));
    assert!(session.render(&p).is_err());
    session.source = Some(Source::load(&source, &data(), &p)?);
    let normal = dir.path().join("normal.npy");
    session.export(&p, &normal)?;
    p.io.monitor_profile = Some(
        data()
            .join("icc/ellelstone/sRGB-elle-V2-srgbtrc.icc")
            .to_string_lossy()
            .into(),
    );
    assert!(session.source.as_ref().unwrap().matches(&source, &p));
    let managed = session.render(&p)?;
    assert!(
        managed
            .before
            .iter()
            .zip(&original.before)
            .all(|(a, b)| a.abs_diff(*b) <= 1)
    );
    let monitor = dir.path().join("monitor.npy");
    session.export(&p, &monitor)?;
    assert_eq!(
        Image::read_npy(&normal)?.data,
        Image::read_npy(&monitor)?.data
    );
    p.io.monitor_profile = Some(dir.path().join("missing.icc").to_string_lossy().into());
    assert!(session.render(&p).is_err());
    // Display failure does not affect the exported processing values.
    session.export(&p, &monitor)?;
    assert_eq!(
        Image::read_npy(&normal)?.data,
        Image::read_npy(&monitor)?.data
    );
    Ok(())
}

#[test]
fn processing_pool_uses_automatic_or_explicit_thread_count() -> Result<()> {
    let automatic = processing_pool(0)?;
    let rayon_default = rayon::ThreadPoolBuilder::new().build()?;
    assert_eq!(
        automatic.current_num_threads(),
        rayon_default.current_num_threads()
    );
    if std::env::var_os("RAYON_NUM_THREADS").is_none()
        && std::env::var_os("RAYON_RS_NUM_CPUS").is_none()
    {
        assert_eq!(
            automatic.current_num_threads(),
            std::thread::available_parallelism()?.get()
        );
    }
    let explicit = processing_pool(2)?;
    assert_eq!(explicit.install(rayon::current_num_threads), 2);
    eprintln!(
        "Automatic GUI processing pool: {} threads",
        automatic.current_num_threads()
    );
    Ok(())
}
