//! Optional Lensfun 0.3 C ABI. The native library supplies measured corrections;
//! Rust performs the same float32 bilinear, nearest-edge resampling as the source.
use crate::Image;
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr,
};
#[repr(C)]
struct Camera {
    maker: *const c_char,
    model: *const c_char,
    variant: *const c_char,
    mount: *const c_char,
    crop: f32,
    score: i32,
}
#[repr(C)]
struct Lens {
    maker: *const c_char,
    model: *const c_char,
    min_focal: f32,
    max_focal: f32,
    min_aperture: f32,
    max_aperture: f32,
    mounts: *const *const c_char,
    center_x: f32,
    center_y: f32,
    crop: f32,
    aspect: f32,
    kind: i32,
    calibration: [*const c_void; 6],
    score: i32,
}
#[derive(Clone, Debug)]
pub struct Metadata {
    pub make: String,
    pub model: String,
    pub lens_make: String,
    pub lens: String,
    pub focal: f32,
    pub aperture: f32,
}
unsafe extern "C" {
    fn sf_raw_metadata(
        path: *const c_char,
        make: *mut c_char,
        model: *mut c_char,
        lens_make: *mut c_char,
        lens: *mut c_char,
        focal: *mut f32,
        aperture: *mut f32,
    ) -> i32;
}
pub fn metadata(path: &Path) -> Result<Metadata> {
    let path = CString::new(path.as_os_str().as_encoded_bytes())?;
    let (mut make, mut model, mut lm, mut lens) = ([0; 128], [0; 128], [0; 128], [0; 128]);
    let (mut focal, mut aperture) = (0., 0.);
    let result = unsafe {
        sf_raw_metadata(
            path.as_ptr(),
            make.as_mut_ptr(),
            model.as_mut_ptr(),
            lm.as_mut_ptr(),
            lens.as_mut_ptr(),
            &mut focal,
            &mut aperture,
        )
    };
    ensure!(
        result == 0,
        "Cannot read RAW lens metadata (LibRaw {result})"
    );
    let string = |p: &[c_char]| unsafe {
        CStr::from_ptr(p.as_ptr())
            .to_string_lossy()
            .trim()
            .to_owned()
    };
    Ok(Metadata {
        make: string(&make),
        model: string(&model),
        lens_make: string(&lm),
        lens: string(&lens),
        focal,
        aperture,
    })
}
struct Guard {
    pointer: *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
}
impl Drop for Guard {
    fn drop(&mut self) {
        unsafe { (self.free)(self.pointer) }
    }
}
fn norm(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn model_score(candidate: &str, target: &str) -> (bool, bool, usize) {
    let c = norm(candidate);
    let t = norm(target);
    let compact = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let tc = compact(&t);
    (
        c == t,
        !tc.is_empty() && compact(&c).contains(&tc),
        t.split_whitespace()
            .filter(|word| c.split_whitespace().any(|v| v == *word))
            .count(),
    )
}
// Database strings may be NULL for unknown metadata.
unsafe fn string(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}
pub fn correct(image: &mut Image, m: &Metadata) -> Result<Option<String>> {
    image.validate()?;
    ensure!(image.channels == 3, "Lens correction requires RGB");
    if m.lens.is_empty() {
        return Ok(None);
    }
    // An override permits a user-local library, without modifying the OS.
    let path =
        std::env::var_os("SPEKTRAFILM_LENSFUN_LIBRARY").unwrap_or_else(|| "liblensfun.so.1".into());
    let lib=unsafe {Library::new(path)}.context("Lens correction requires Lensfun 0.3; set SPEKTRAFILM_LENSFUN_LIBRARY for a user-local library")?;
    unsafe {
        let new: libloading::Symbol<unsafe extern "C" fn() -> *mut c_void> =
            lib.get(b"lf_db_new\0")?;
        let destroy: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> =
            lib.get(b"lf_db_destroy\0")?;
        let load: libloading::Symbol<unsafe extern "C" fn(*mut c_void) -> i32> =
            lib.get(b"lf_db_load\0")?;
        let free: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> = lib.get(b"lf_free\0")?;
        let db = Guard {
            pointer: new(),
            free: *destroy,
        };
        ensure!(!db.pointer.is_null(), "Lensfun database allocation failed");
        if let Some(path) = std::env::var_os("SPEKTRAFILM_LENSFUN_DATABASE") {
            let load_dir: libloading::Symbol<
                unsafe extern "C" fn(*mut c_void, *const c_char) -> i32,
            > = lib.get(b"lf_db_load_directory\0")?;
            let path = CString::new(path.as_encoded_bytes())?;
            ensure!(
                load_dir(db.pointer, path.as_ptr()) != 0,
                "Cannot load Lensfun database directory"
            );
        } else {
            ensure!(load(db.pointer) == 0, "Cannot load Lensfun database");
        }
        let find_camera: libloading::Symbol<
            unsafe extern "C" fn(
                *const c_void,
                *const c_char,
                *const c_char,
                i32,
            ) -> *mut *const Camera,
        > = lib.get(b"lf_db_find_cameras_ext\0")?;
        let make = CString::new(m.make.as_str())?;
        let model = CString::new(m.model.as_str())?;
        let cameras = find_camera(db.pointer, make.as_ptr(), model.as_ptr(), 1);
        if cameras.is_null() {
            return Ok(None);
        }
        let _cameras = Guard {
            pointer: cameras.cast(),
            free: *free,
        };
        let camera = *cameras;
        if camera.is_null() {
            return Ok(None);
        }
        let find_lens: libloading::Symbol<
            unsafe extern "C" fn(
                *const c_void,
                *const Camera,
                *const c_char,
                *const c_char,
                i32,
            ) -> *mut *const Lens,
        > = lib.get(b"lf_db_find_lenses_hd\0")?;
        let maker = CString::new(m.lens_make.as_str())?;
        let name = CString::new(m.lens.as_str())?;
        let maker = if m.lens_make.is_empty() {
            ptr::null()
        } else {
            maker.as_ptr()
        };
        let mut candidates = Vec::new();
        for (maker, name, flags, broad) in [
            (maker, name.as_ptr(), 0, false),
            (maker, name.as_ptr(), 1, false),
            (ptr::null(), name.as_ptr(), 1, false),
            (ptr::null(), ptr::null(), 1, true),
        ] {
            let list = find_lens(db.pointer, camera, maker, name, flags);
            if list.is_null() {
                continue;
            }
            let _list = Guard {
                pointer: list.cast(),
                free: *free,
            };
            let mut index = 0;
            while !(*list.add(index)).is_null() {
                let l = *list.add(index);
                let label = string((*l).model);
                if !broad || model_score(&label, &m.lens) > (false, false, 0) {
                    candidates.push(l);
                }
                index += 1;
            }
            if !candidates.is_empty() {
                break;
            }
        }
        let focal_match =
            |l: *const Lens| m.focal > 0. && m.focal >= (*l).min_focal && m.focal <= (*l).max_focal;
        if candidates.iter().any(|l| focal_match(*l)) {
            candidates.retain(|l| focal_match(*l));
        }
        let score = |l: *const Lens| {
            let l = &*l;
            let label = string(l.model);
            let maker = string(l.maker);
            (
                model_score(&label, &m.lens),
                !m.lens_make.is_empty() && norm(&maker) == norm(&m.lens_make),
                focal_match(l),
                m.aperture > 0.
                    && m.aperture >= l.min_aperture
                    && (l.max_aperture <= 0. || m.aperture <= l.max_aperture),
                l.score,
            )
        };
        let Some(lens) = candidates.into_iter().max_by_key(|l| score(*l)) else {
            return Ok(None);
        };
        let new_mod: libloading::Symbol<
            unsafe extern "C" fn(*const Lens, f32, i32, i32) -> *mut c_void,
        > = lib.get(b"lf_modifier_new\0")?;
        let destroy_mod: libloading::Symbol<unsafe extern "C" fn(*mut c_void)> =
            lib.get(b"lf_modifier_destroy\0")?;
        let initialize: libloading::Symbol<
            unsafe extern "C" fn(
                *mut c_void,
                *const Lens,
                i32,
                f32,
                f32,
                f32,
                f32,
                i32,
                i32,
                i32,
            ) -> i32,
        > = lib.get(b"lf_modifier_initialize\0")?;
        let color: libloading::Symbol<
            unsafe extern "C" fn(*mut c_void, *mut c_void, f32, f32, i32, i32, i32, i32) -> i32,
        > = lib.get(b"lf_modifier_apply_color_modification\0")?;
        let geometry: libloading::Symbol<
            unsafe extern "C" fn(*mut c_void, f32, f32, i32, i32, *mut f32) -> i32,
        > = lib.get(b"lf_modifier_apply_subpixel_geometry_distortion\0")?;
        let (w, h) = (i32::try_from(image.width)?, i32::try_from(image.height)?);
        let modifier = Guard {
            pointer: new_mod(lens, (*camera).crop, w, h),
            free: *destroy_mod,
        };
        ensure!(
            !modifier.pointer.is_null(),
            "Lensfun modifier allocation failed"
        );
        initialize(
            modifier.pointer,
            lens,
            3,
            m.focal,
            m.aperture,
            1000.,
            0.,
            1,
            -1,
            0,
        );
        let mut pixels: Vec<f32> = image.data.iter().map(|v| *v as f32).collect();
        color(
            modifier.pointer,
            pixels.as_mut_ptr().cast(),
            0.,
            0.,
            w,
            h,
            4 | (5 << 4) | (6 << 8),
            w.checked_mul(12).context("Image row too large")?,
        );
        // Lensfun's SIMD coordinate path depends on row alignment. A single
        // full-frame call matches lensfunpy, including odd-width RGB images.
        let mut coords = vec![0f32; image.width * image.height * 6];
        let has_geometry = geometry(modifier.pointer, 0., 0., w, h, coords.as_mut_ptr()) != 0;
        for y in 0..image.height {
            if has_geometry {
                for x in 0..image.width {
                    for c in 0..3 {
                        let px = coords[(y * image.width + x) * 6 + c * 2].clamp(0., (w - 1) as f32)
                            as f64;
                        let py = coords[(y * image.width + x) * 6 + c * 2 + 1]
                            .clamp(0., (h - 1) as f32) as f64;
                        let x0 = px.floor() as usize;
                        let y0 = py.floor() as usize;
                        let dx = px - x0 as f64;
                        let dy = py - y0 as f64;
                        let at = |xx: usize, yy: usize| {
                            pixels[(yy.min(image.height - 1) * image.width
                                + xx.min(image.width - 1))
                                * 3
                                + c] as f64
                        };
                        image.data[(y * image.width + x) * 3 + c] =
                            ((at(x0, y0) * (1. - dx) + at(x0 + 1, y0) * dx) * (1. - dy)
                                + (at(x0, y0 + 1) * (1. - dx) + at(x0 + 1, y0 + 1) * dx) * dy)
                                as f32 as f64;
                    }
                }
            } else {
                for x in 0..image.width * 3 {
                    image.data[y * image.width * 3 + x] = pixels[y * image.width * 3 + x] as f64;
                }
            }
        }
        image.validate()?;
        Ok(Some(format!(
            "{} @ {}mm f/{}",
            string((*lens).model),
            m.focal,
            m.aperture
        )))
    }
}
