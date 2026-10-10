use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::File,
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub channels: usize,
    pub data: Vec<f64>,
}
impl Image {
    pub fn new(width: usize, height: usize, channels: usize, data: Vec<f64>) -> Result<Self> {
        ensure!(
            width > 0 && height > 0 && channels > 0,
            "Image dimensions must be positive"
        );
        ensure!(
            width
                .checked_mul(height)
                .and_then(|n| n.checked_mul(channels))
                == Some(data.len()),
            "Image shape does not match data length"
        );
        ensure!(
            data.iter().all(|v| v.is_finite()),
            "Image contains nonfinite values"
        );
        Ok(Self {
            width,
            height,
            channels,
            data,
        })
    }
    pub fn read_npy(path: &Path) -> Result<Self> {
        let (shape, data) = read_npy(path)?;
        ensure!(shape.len() == 3, "Expected H x W x C numpy array");
        Self::new(shape[1], shape[0], shape[2], data)
    }
    /// Select a center crop, retaining the original pixel values.
    pub fn center_crop(&self, size: usize) -> Result<Self> {
        ensure!(size > 0, "Crop size must be positive");
        let w = size.min(self.width);
        let h = size.min(self.height);
        let x = (self.width - w) / 2;
        let y = (self.height - h) / 2;
        let mut data = Vec::with_capacity(w * h * self.channels);
        for row in y..y + h {
            let start = (row * self.width + x) * self.channels;
            data.extend_from_slice(&self.data[start..start + w * self.channels]);
        }
        Self::new(w, h, self.channels, data)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.width > 0 && self.height > 0 && self.channels > 0,
            "Image dimensions must be positive"
        );
        ensure!(
            self.width
                .checked_mul(self.height)
                .and_then(|n| n.checked_mul(self.channels))
                == Some(self.data.len()),
            "Image shape does not match data length"
        );
        ensure!(
            self.data.iter().all(|v| v.is_finite()),
            "Image contains nonfinite values"
        );
        Ok(())
    }
    pub fn write_npy(&self, path: &Path) -> Result<()> {
        write_npy(path, &[self.height, self.width, self.channels], &self.data)
    }
    pub fn write_tiff(&self, path: &Path) -> Result<()> {
        ensure!(self.channels == 3, "TIFF output requires 3 channels");
        let mut enc = tiff::encoder::TiffEncoder::new(BufWriter::new(File::create(path)?))?;
        let img = enc.new_image::<tiff::encoder::colortype::RGB32Float>(
            u32::try_from(self.width)?,
            u32::try_from(self.height)?,
        )?;
        img.write_data(&self.data.iter().map(|v| *v as f32).collect::<Vec<_>>())?;
        Ok(())
    }
}

pub fn read_npy(path: &Path) -> Result<(Vec<usize>, Vec<f64>)> {
    let mut f =
        BufReader::new(File::open(path).with_context(|| format!("Read {}", path.display()))?);
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    ensure!(&magic[..6] == b"\x93NUMPY", "Invalid NPY signature");
    let header_len = match magic[6] {
        1 => {
            let mut n = [0u8; 2];
            f.read_exact(&mut n)?;
            u16::from_le_bytes(n) as usize
        }
        2 | 3 => {
            let mut n = [0u8; 4];
            f.read_exact(&mut n)?;
            u32::from_le_bytes(n) as usize
        }
        _ => bail!("Unsupported NPY version"),
    };
    ensure!(header_len < 1_000_000, "NPY header too large");
    let mut h = vec![0u8; header_len];
    f.read_exact(&mut h)?;
    let h = String::from_utf8(h)?;
    ensure!(
        h.contains("'fortran_order': False") || h.contains("\"fortran_order\": False"),
        "Fortran-order NPY is unsupported"
    );
    let start = h.find("shape").context("Missing shape")?;
    let start = h[start..].find('(').context("Missing shape tuple")? + start;
    let end = h[start..].find(')').context("Unterminated shape")? + start;
    let shape = h[start + 1..end]
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse::<usize>())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let n = shape
        .iter()
        .try_fold(1usize, |n, k| n.checked_mul(*k))
        .context("NPY shape overflow")?;
    let f64_type = h.contains("<f8") || h.contains("=f8");
    let f32_type = h.contains("<f4") || h.contains("=f4");
    let f16_type = h.contains("<f2") || h.contains("=f2");
    ensure!(
        f64_type || f32_type || f16_type,
        "Only little-endian floating NPY is supported"
    );
    let mut data = Vec::with_capacity(n);
    if f64_type {
        for _ in 0..n {
            let mut b = [0; 8];
            f.read_exact(&mut b)?;
            data.push(f64::from_le_bytes(b));
        }
    } else if f32_type {
        for _ in 0..n {
            let mut b = [0; 4];
            f.read_exact(&mut b)?;
            data.push(f32::from_le_bytes(b) as f64);
        }
    } else {
        for _ in 0..n {
            let mut b = [0; 2];
            f.read_exact(&mut b)?;
            data.push(half::f16::from_bits(u16::from_le_bytes(b)).to_f64());
        }
    }
    let mut tail = [0];
    ensure!(f.read(&mut tail)? == 0, "NPY has trailing data");
    Ok((shape, data))
}
pub fn write_npy(path: &Path, shape: &[usize], data: &[f64]) -> Result<()> {
    let shape_str = shape
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut header =
        format!("{{'descr': '<f8', 'fortran_order': False, 'shape': ({shape_str},), }}");
    let padding = (64 - ((10 + header.len() + 1) % 64)) % 64;
    header.push_str(&" ".repeat(padding));
    header.push('\n');
    let mut out = BufWriter::new(File::create(path)?);
    out.write_all(b"\x93NUMPY\x01\x00")?;
    out.write_all(&u16::try_from(header.len())?.to_le_bytes())?;
    out.write_all(header.as_bytes())?;
    for v in data {
        out.write_all(&v.to_le_bytes())?;
    }
    out.flush()?;
    Ok(())
}

unsafe extern "C" {
    fn sf_decode_raw(
        path: *const std::ffi::c_char,
        half_size: i32,
        camera_wb: i32,
        error: *mut i32,
    ) -> *mut std::ffi::c_void;
    fn sf_raw_width(p: *const std::ffi::c_void) -> u32;
    fn sf_raw_height(p: *const std::ffi::c_void) -> u32;
    fn sf_raw_colors(p: *const std::ffi::c_void) -> u32;
    fn sf_raw_bits(p: *const std::ffi::c_void) -> u32;
    fn sf_raw_bytes(p: *const std::ffi::c_void) -> u32;
    fn sf_raw_pixels(p: *const std::ffi::c_void) -> *const u8;
    fn libraw_dcraw_clear_mem(p: *mut std::ffi::c_void);
    fn libraw_version() -> *const std::ffi::c_char;
    fn libraw_strerror(code: i32) -> *const std::ffi::c_char;
}
pub fn raw_version() -> String {
    unsafe {
        std::ffi::CStr::from_ptr(libraw_version())
            .to_string_lossy()
            .into_owned()
    }
}
pub fn decode_raw(path: &Path, half_size: bool, camera_wb: bool) -> Result<Image> {
    decode_raw_crop(path, half_size, camera_wb, None)
}
pub fn decode_raw_crop(
    path: &Path,
    half_size: bool,
    camera_wb: bool,
    crop_size: Option<usize>,
) -> Result<Image> {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
    let mut error = 0;
    let p = unsafe {
        sf_decode_raw(
            path.as_ptr(),
            half_size as i32,
            camera_wb as i32,
            &mut error,
        )
    };
    if p.is_null() {
        bail!(
            "LibRaw: {}",
            unsafe { std::ffi::CStr::from_ptr(libraw_strerror(error)) }.to_string_lossy()
        );
    }
    struct Guard(*mut std::ffi::c_void);
    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe { libraw_dcraw_clear_mem(self.0) }
        }
    }
    let _guard = Guard(p);
    let (w, h, c, bits, bytes) = unsafe {
        (
            sf_raw_width(p) as usize,
            sf_raw_height(p) as usize,
            sf_raw_colors(p) as usize,
            sf_raw_bits(p),
            sf_raw_bytes(p) as usize,
        )
    };
    ensure!(
        c == 3 && bits == 16 && bytes == w * h * c * 2,
        "Unexpected LibRaw bitmap layout"
    );
    let pixels = unsafe { std::slice::from_raw_parts(sf_raw_pixels(p), bytes) };
    // Match reference rawpy's uint16 -> float32 -> normalized float32 path.
    let cw = crop_size.unwrap_or(w).min(w);
    let ch = crop_size.unwrap_or(h).min(h);
    ensure!(cw > 0 && ch > 0, "Crop size must be positive");
    let x0 = (w - cw) / 2;
    let y0 = (h - ch) / 2;
    let mut data = Vec::with_capacity(cw * ch * c);
    for y in y0..y0 + ch {
        let row = &pixels[(y * w + x0) * c * 2..(y * w + x0 + cw) * c * 2];
        for b in row.chunks_exact(2) {
            data.push((u16::from_ne_bytes([b[0], b[1]]) as f32 / 65535.0_f32) as f64);
        }
    }
    Image::new(cw, ch, c, data)
}
