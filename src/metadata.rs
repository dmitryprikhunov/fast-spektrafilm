//! Content metadata carryover through GExiv2's stable C ABI. Pixel processing
//! stays in Rust; Exiv2 handles the many camera-specific metadata containers.
use anyhow::{Context, Result, ensure};
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::Path,
    sync::{Mutex, OnceLock},
};
static LOCK: Mutex<()> = Mutex::new(());
// GObject permanently registers class callbacks. Unloading and reloading its
// defining library leaves those callbacks dangling, so retain the libraries.
static LIBRARIES: OnceLock<std::result::Result<(Library, Library, Library), String>> =
    OnceLock::new();
type Object = *mut c_void;
type FileOperation = unsafe extern "C" fn(Object, *const c_char, *mut Object) -> c_int;
type SetTag = unsafe extern "C" fn(Object, *const c_char, *const c_char, *mut Object) -> c_int;
struct Metadata<'a> {
    library: &'a Library,
    object: Object,
    unref: unsafe extern "C" fn(Object),
}
impl Drop for Metadata<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.unref)(self.object);
        }
    }
}
impl Metadata<'_> {
    fn file(&self, operation: &[u8], path: &Path) -> Result<()> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())?;
        // Object and library outlive each call. Passing a null GError** asks
        // GLib not to allocate an error; the boolean return is always checked.
        unsafe {
            let call = self.library.get::<FileOperation>(operation)?;
            ensure!(
                call(self.object, path.as_ptr(), std::ptr::null_mut()) != 0,
                "Metadata operation failed for {}",
                path.to_string_lossy()
            );
        }
        Ok(())
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        let k = CString::new(key)?;
        let v = CString::new(value)?;
        unsafe {
            let set = self
                .library
                .get::<SetTag>(b"gexiv2_metadata_try_set_tag_string\0")?;
            ensure!(
                set(self.object, k.as_ptr(), v.as_ptr(), std::ptr::null_mut()) != 0,
                "Cannot write metadata tag {key}"
            );
        }
        Ok(())
    }
}
/// Copy EXIF, IPTC and XMP without changing the destination's encoded pixels or
/// embedded ICC. NPY/EXR have no Exiv2 metadata path, matching the reference.
pub fn copy(
    source: &Path,
    destination: &Path,
    dimensions: [usize; 2],
    space: &str,
    encoded: bool,
) -> Result<()> {
    if !["jpg", "jpeg", "png", "tif", "tiff"]
        .contains(&crate::file_io::extension(destination).as_str())
    {
        return Ok(());
    }
    // TIFF stores ICC in its EXIF IFD. Carrying a source EXIF tree into it would
    // otherwise replace that tag, unlike PNG/JPEG's separate ICC containers.
    let destination_icc =
        if ["tif", "tiff"].contains(&crate::file_io::extension(destination).as_str()) {
            crate::file_io::embedded_icc(destination)?
        } else {
            None
        };
    let _guard = LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Metadata lock poisoned"))?;
    let (exiv, glib, gobject) = if cfg!(target_os = "windows") {
        (
            "libgexiv2-2.dll",
            "libglib-2.0-0.dll",
            "libgobject-2.0-0.dll",
        )
    } else if cfg!(target_os = "macos") {
        (
            "libgexiv2.2.dylib",
            "libglib-2.0.0.dylib",
            "libgobject-2.0.0.dylib",
        )
    } else {
        ("libgexiv2.so.2", "libglib-2.0.so.0", "libgobject-2.0.so.0")
    };
    let configured = std::env::var_os("SPEKTRAFILM_GEXIV2_LIBRARY");
    let libraries = LIBRARIES.get_or_init(|| unsafe {
        (|| -> Result<_> {
            let library = Library::new(configured.as_deref().unwrap_or(std::ffi::OsStr::new(exiv)))
                .context(
                    "Metadata copying requires GExiv2 0.14; install it or turn off Copy metadata",
                )?;
            Ok((library, Library::new(glib)?, Library::new(gobject)?))
        })()
        .map_err(|e| format!("{e:#}"))
    });
    let (library, glib, gobject) = libraries.as_ref().map_err(|e| anyhow::anyhow!(e.clone()))?;
    // ABI declarations follow GExiv2 0.14 headers. Objects and strings are freed
    // per operation; the GObject libraries intentionally live for the process.
    unsafe {
        let new = library.get::<unsafe extern "C" fn() -> Object>(b"gexiv2_metadata_new\0")?;
        let unref = *gobject.get::<unsafe extern "C" fn(Object)>(b"g_object_unref\0")?;
        let object = new();
        ensure!(!object.is_null(), "Cannot allocate metadata object");
        let metadata = Metadata {
            library,
            object,
            unref,
        };
        // An unsupported or untagged input still receives fresh output tags.
        if metadata
            .file(b"gexiv2_metadata_open_path\0", source)
            .is_err()
        {
            metadata.file(b"gexiv2_metadata_open_path\0", destination)?;
        }
        // Remove the stale embedded thumbnail before saving the source content tags.
        let erase = library
            .get::<unsafe extern "C" fn(Object)>(b"gexiv2_metadata_erase_exif_thumbnail\0")?;
        erase(object);
        metadata.set("Exif.Image.Orientation", "1")?;
        metadata.set("Exif.Image.Software", "spektrafilm (Rust)")?;
        metadata.set("Exif.Photo.PixelXDimension", &dimensions[0].to_string())?;
        metadata.set("Exif.Photo.PixelYDimension", &dimensions[1].to_string())?;
        metadata.set(
            "Exif.Photo.ColorSpace",
            if space == "sRGB" && encoded {
                "1"
            } else {
                "65535"
            },
        )?;
        metadata.set(
            "Exif.Iop.InteroperabilityIndex",
            if encoded && space == "sRGB" {
                "R98"
            } else if encoded && space == "Adobe RGB (1998)" {
                "R03"
            } else {
                ""
            },
        )?;
        metadata.set(
            "Xmp.photoshop.ICCProfile",
            &format!("{space}{}", if encoded { "" } else { " (linear)" }),
        )?;
        let now = glib.get::<unsafe extern "C" fn() -> Object>(b"g_date_time_new_now_local\0")?;
        let format = glib.get::<unsafe extern "C" fn(Object, *const c_char) -> *mut c_char>(
            b"g_date_time_format\0",
        )?;
        let date_unref = glib.get::<unsafe extern "C" fn(Object)>(b"g_date_time_unref\0")?;
        let free = glib.get::<unsafe extern "C" fn(Object)>(b"g_free\0")?;
        let date = now();
        if !date.is_null() {
            let formatted = format(date, c"%Y:%m:%d %H:%M:%S".as_ptr());
            date_unref(date);
            if !formatted.is_null() {
                let value = CStr::from_ptr(formatted).to_string_lossy().into_owned();
                free(formatted.cast());
                metadata.set("Exif.Image.DateTime", &value)?;
            }
        }
        if let Some(profile) = destination_icc {
            metadata.set(
                "Exif.Image.InterColorProfile",
                &profile
                    .iter()
                    .map(u8::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            )?;
        }
        metadata.file(b"gexiv2_metadata_save_file\0", destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_roundtrip_preserves_content_and_output_icc() -> Result<()> {
        if std::env::var_os("SPEKTRAFILM_TEST_METADATA").is_none() {
            return Ok(());
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("artifacts/metadata-test");
        std::fs::create_dir_all(&dir)?;
        let source = root.join("tests/fixtures/core/metadata.png");
        let source_bytes = std::fs::read(&source)?;
        let im = crate::Image::new(2, 3, 3, vec![0.25; 18])?;
        for extension in ["png", "jpg", "tiff"] {
            let path = dir.join(format!("output.{extension}"));
            crate::file_io::write(&im, &path, &root.join("data"), "sRGB", true)?;
            let before = image::open(&path)?.to_rgb32f();
            copy(&source, &path, [2, 3], "sRGB", true)?;
            assert_eq!(image::open(&path)?.to_rgb32f(), before);
            assert_eq!(
                crate::file_io::embedded_icc(&path)?.unwrap(),
                crate::file_io::icc(&root.join("data"), "sRGB", true)?
            );
            // Independent tag reads from the written output, including both
            // non-EXIF containers and orientation/dimensions after rotation.
            unsafe {
                let library = Library::new("libgexiv2.so.2")?;
                let gobject = Library::new("libgobject-2.0.so.0")?;
                let glib = Library::new("libglib-2.0.so.0")?;
                let new =
                    library.get::<unsafe extern "C" fn() -> Object>(b"gexiv2_metadata_new\0")?;
                let metadata = Metadata {
                    library: &library,
                    object: new(),
                    unref: *gobject.get::<unsafe extern "C" fn(Object)>(b"g_object_unref\0")?,
                };
                ensure!(!metadata.object.is_null(), "Metadata allocation failed");
                metadata.file(b"gexiv2_metadata_open_path\0", &path)?;
                let get = library.get::<unsafe extern "C" fn(
                    Object,
                    *const c_char,
                    *mut Object,
                ) -> *mut c_char>(
                    b"gexiv2_metadata_try_get_tag_string\0"
                )?;
                let free = glib.get::<unsafe extern "C" fn(Object)>(b"g_free\0")?;
                for (key, expected) in [
                    ("Exif.Image.Artist", "Metadata fixture"),
                    ("Exif.Image.Orientation", "1"),
                    ("Exif.Photo.PixelXDimension", "2"),
                    ("Exif.Photo.PixelYDimension", "3"),
                    ("Exif.Photo.ColorSpace", "1"),
                    ("Iptc.Application2.Caption", "Synthetic caption"),
                    ("Xmp.dc.description", "Synthetic description"),
                ] {
                    let key = CString::new(key)?;
                    let value = get(metadata.object, key.as_ptr(), std::ptr::null_mut());
                    ensure!(!value.is_null(), "Missing tag {key:?}");
                    let text = CStr::from_ptr(value).to_string_lossy().into_owned();
                    free(value.cast());
                    assert!(text.contains(expected), "{key:?}: {text}");
                }
            }
        }
        assert_eq!(std::fs::read(source)?, source_bytes);
        Ok(())
    }
}
