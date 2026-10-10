use super::*;
use crate::{
    Image, Simulator, Tap,
    color::Tables,
    image::{decode_raw, decode_raw_preview},
};
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
    p.debug.deactivate_stochastic_effects = true;
    p.debug.deactivate_spatial_effects = true;
    p
}

#[test]
fn export_matches_core_and_keeps_original_resolution() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let input = Image::new(800, 2, 3, vec![0.18; 800 * 2 * 3])?;
    let source = dir.path().join("INPUT.NPY");
    input.write_npy(&source)?;
    let params = deterministic();
    let mut session = Session::new(&data())?;
    session.source = Some(Source::load(&source)?);
    let cached = session.source.as_ref().unwrap();
    assert_eq!(cached.original, [800, 2]);
    assert_eq!(cached.preview.width, 768);
    let preview = session.render(&params)?;
    assert_eq!(preview.after_size, [768, 2]);
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
    let unsupported = dir.path().join("result.jpg");
    assert!(session.export(&params, &unsupported).is_err());
    assert!(!unsupported.exists());
    let mut invalid = params.clone();
    invalid.workflow.route = "unsupported".into();
    let preserved = std::fs::read(&path)?;
    assert!(session.export(&invalid, &path).is_err());
    assert_eq!(std::fs::read(&path)?, preserved);
    let png = dir.path().join("result.png");
    session.export(&params, &png)?;
    let png = image::open(png)?.to_rgb8();
    assert_eq!(png.dimensions(), (800, 2));
    assert_eq!(
        png.as_raw(),
        &backend::display_rgb(
            &direct,
            &params.io.output_color_space,
            params.io.output_cctf_encoding,
            &Tables::load(&data())?
        )?
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
    assert_eq!(preview.data, vec![0., 0., 0., 1., 1., 1.]);
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
    session.source = Some(Source::load(&fixture().join("input.npy"))?);
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
    set_number(&mut restored, "yellow", 12.5);
    set_toggle(&mut restored, "grain", false);
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
fn raw_preview_matches_full_decode_area_average() -> Result<()> {
    let Some(path) = std::env::var_os("GUI_QA_RAW") else {
        return Ok(());
    };
    let path = PathBuf::from(path);
    let (preview, original) = decode_raw_preview(&path, true, 768)?;
    let full = decode_raw(&path, false, true)?;
    assert_eq!(original, [full.width, full.height]);
    let expected = backend::resize_preview(&full, 768)?;
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
        films,
        papers,
        revision: Arc::new(AtomicU64::new(0)),
        sender,
        debounce: Timer::default(),
    }));
    ui.set_films(model(state.borrow().films.iter().map(|s| s.label.clone())));
    ui.set_papers(model(state.borrow().papers.iter().map(|s| s.label.clone())));
    sync(&ui, &state.borrow());
    bind_callbacks(&ui, &state);
    ui.set_auto_preview(false);
    ui.invoke_number_edited("exposure".into(), 1.25);
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 1.25);
    assert!(ui.get_stale());
    ui.invoke_toggle_edited("grain".into(), false);
    assert!(!state.borrow().params.grain.active);
    ui.invoke_choice_edited("route".into(), "Film → Scan".into());
    assert_eq!(state.borrow().params.workflow.route, "input > film > scan");
    ui.invoke_apply_json("{bad".into());
    assert!(ui.get_error());
    assert_eq!(state.borrow().params.workflow.route, "input > film > scan");
    ui.invoke_preview();
    let request = receiver.try_recv()?;
    assert_eq!(
        request.revision,
        state.borrow().revision.load(Ordering::SeqCst)
    );
    assert!(matches!(request.job, Job::Preview { .. }));
    ui.set_busy(false);
    ui.invoke_apply_json(serde_json::to_string(&deterministic())?.into());
    assert!(!ui.get_error());
    assert_eq!(state.borrow().params.workflow.route, ROUTES[0]);
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
    // Real pointer input must update the Rust setting, and an imported preset
    // must restore the control after it has been manipulated.
    ui.show()?;
    window.set_size(slint::PhysicalSize::new(1240, 850));
    let _ = ui.window().take_snapshot()?;
    let position = slint::LogicalPosition::new(260., 451.);
    window.dispatch_event(slint::platform::WindowEvent::PointerPressed {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    window.dispatch_event(slint::platform::WindowEvent::PointerReleased {
        position,
        button: slint::platform::PointerEventButton::Left,
    });
    assert!(state.borrow().params.camera.exposure_compensation_ev > 1.);
    ui.invoke_apply_json(serde_json::to_string(&deterministic())?.into());
    event_receiver.recv_timeout(Duration::from_secs(60))?();
    assert_eq!(ui.get_controls().exposure, 0.);
    assert_eq!(state.borrow().params.camera.exposure_compensation_ev, 0.);
    let mut session = Session::new(&data())?;
    // Optional QA uses the actual camera sample for screenshots, otherwise
    // a small checked-in numerical fixture keeps this test portable.
    let path = std::env::var_os("GUI_QA_RAW")
        .map(PathBuf::from)
        .unwrap_or_else(|| fixture().join("input.npy"));
    session.source = Some(Source::load(&path)?);
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
    ui.set_dimensions("Full frame · 768 px preview · original resolution export".into());
    ui.set_status("Preview ready. Export reprocesses the original at full resolution.".into());
    ui.set_render_info(
        format!(
            "{} × {} preview · {:.2} s · full frame",
            preview.after_size[0], preview.after_size[1], preview.seconds
        )
        .into(),
    );
    ui.show()?;
    let qa_dir = std::env::var_os("GUI_QA_DIR").map(PathBuf::from);
    if let Some(dir) = &qa_dir {
        std::fs::create_dir_all(dir)?;
    }
    for (w, h, name, view, has_image) in [
        (1240, 850, "developed", 1, true),
        (1240, 850, "comparison", 2, true),
        (980, 700, "compact", 1, true),
        (1240, 850, "empty", 1, false),
    ] {
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
