//! Image/session operations, independent of the Slint event loop.
use crate::{
    Image, Params, Simulator, Tap,
    color::{Tables, decode, encode, mat_vec},
    image::{decode_raw, decode_raw_preview},
    profile::Info,
};
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Instant,
};

pub const PREVIEW_EDGE: usize = 768;
pub const ROUTES: [&str; 3] = [
    "input > film > print > scan",
    "input > film > scan",
    "input",
];
pub const ROUTE_LABELS: [&str; 3] = ["Film → Print → Scan", "Film → Scan", "Passthrough"];

#[derive(Clone)]
pub struct Stock {
    pub id: String,
    pub label: String,
}
pub fn profiles(data: &Path) -> Result<(Vec<Stock>, Vec<Stock>)> {
    #[derive(Deserialize)]
    struct Header {
        info: Info,
    }
    let mut film = Vec::new();
    let mut paper = Vec::new();
    for entry in std::fs::read_dir(data.join("profiles"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|v| v != "json") {
            continue;
        }
        let header: Header = serde_json::from_reader(File::open(&path)?)?;
        let stock = Stock {
            id: path
                .file_stem()
                .context("Missing stock ID")?
                .to_string_lossy()
                .into(),
            label: header.info.name,
        };
        match header.info.stage.as_str() {
            "filming" => film.push(stock),
            "printing" => paper.push(stock),
            _ => {}
        }
    }
    for list in [&mut film, &mut paper] {
        list.sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));
        // Some measurement variants have the same display name.
        let labels: Vec<_> = list.iter().map(|s| s.label.clone()).collect();
        for stock in list.iter_mut() {
            if labels.iter().filter(|label| **label == stock.label).count() > 1 {
                stock.label = format!("{} ({})", stock.label, stock.id);
            }
        }
    }
    ensure!(
        !film.is_empty() && !paper.is_empty(),
        "No film/print profiles found"
    );
    Ok((film, paper))
}

pub fn read_params(path: &Path) -> Result<Params> {
    serde_json::from_reader(File::open(path)?)
        .with_context(|| format!("Load settings {}", path.display()))
}
pub fn save_params(params: &Params, path: &Path) -> Result<()> {
    let temp = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    std::fs::write(temp.path(), serde_json::to_string_pretty(params)? + "\n")?;
    temp.persist(path)?;
    Ok(())
}
pub fn is_npy(path: &Path) -> bool {
    path.extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("npy"))
}

/// Cached in its original input space: NPY keeps the configured interpretation,
/// RAW keeps linear ACES2065-1. Input settings may change without decoding again.
pub struct Source {
    pub path: PathBuf,
    pub preview: Image,
    pub original: [usize; 2],
    pub raw: bool,
}
impl Source {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = !is_npy(path);
        let (preview, original) = if raw {
            decode_raw_preview(path, true, PREVIEW_EDGE)?
        } else {
            let image = Image::read_npy(path)?;
            ensure!(image.channels == 3, "Input must be RGB (3 channels)");
            let original = [image.width, image.height];
            (resize_preview(&image, PREVIEW_EDGE)?, original)
        };
        Ok(Self {
            path: path.into(),
            preview,
            original,
            raw,
        })
    }
    pub fn input(&self, params: &Params, tables: &Tables, full: bool) -> Result<Image> {
        let mut image = if full {
            if self.raw {
                decode_raw(&self.path, false, true)?
            } else {
                Image::read_npy(&self.path)?
            }
        } else {
            self.preview.clone()
        };
        if self.raw {
            let matrix = tables.rgb_to_rgb("ACES2065-1", &params.io.input_color_space)?;
            for p in image.data.chunks_exact_mut(3) {
                let mut v = mat_vec(&matrix, [p[0], p[1], p[2]]);
                if params.io.input_cctf_decoding {
                    v = v.map(|v| encode(&params.io.input_color_space, v));
                }
                p.copy_from_slice(&v);
            }
        }
        image.validate()?;
        Ok(image)
    }
}

pub fn resize_preview(image: &Image, edge: usize) -> Result<Image> {
    image.validate()?;
    ensure!(edge > 0, "Preview size must be positive");
    if image.width.max(image.height) <= edge {
        return Ok(image.clone());
    }
    let scale = edge as f64 / image.width.max(image.height) as f64;
    let w = ((image.width as f64 * scale).round() as usize).max(1);
    let h = ((image.height as f64 * scale).round() as usize).max(1);
    let c = image.channels;
    let mut data = vec![0.0; w * h * c];
    for y in 0..h {
        let y0 = y * image.height / h;
        let y1 = (y + 1) * image.height / h;
        for x in 0..w {
            let x0 = x * image.width / w;
            let x1 = (x + 1) * image.width / w;
            let count = ((x1 - x0) * (y1 - y0)) as f64;
            for sy in y0..y1 {
                for sx in x0..x1 {
                    for ch in 0..c {
                        data[(y * w + x) * c + ch] +=
                            image.data[(sy * image.width + sx) * c + ch] / count;
                    }
                }
            }
        }
    }
    Image::new(w, h, c, data)
}

/// Display and PNG export always use encoded sRGB. Float TIFF/NPY exports
/// retain the requested output space and transfer function, without clipping.
pub fn display_rgb(image: &Image, space: &str, encoded: bool, tables: &Tables) -> Result<Vec<u8>> {
    image.validate()?;
    ensure!(image.channels == 3, "Display requires RGB");
    let matrix = tables.rgb_to_rgb(space, "sRGB")?;
    let mut out = Vec::with_capacity(image.data.len());
    for p in image.data.chunks_exact(3) {
        let mut v = [p[0], p[1], p[2]];
        if encoded {
            v = v.map(|v| decode(space, v));
        }
        for value in mat_vec(&matrix, v) {
            let value = encode("sRGB", value);
            ensure!(
                value.is_finite(),
                "Display conversion produced a nonfinite value"
            );
            out.push((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    Ok(out)
}

pub struct Preview {
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub before_size: [u32; 2],
    pub after_size: [u32; 2],
    pub seconds: f64,
}
pub struct Session {
    data: PathBuf,
    tables: Tables,
    pub source: Option<Source>,
}
impl Session {
    pub fn new(data: &Path) -> Result<Self> {
        Ok(Self {
            data: data.into(),
            tables: Tables::load(data)?,
            source: None,
        })
    }
    pub fn validate_params(&self, params: &Params) -> Result<()> {
        Simulator::new(&self.data, params.clone())?;
        Ok(())
    }
    pub fn render(&self, params: &Params) -> Result<Preview> {
        let start = Instant::now();
        let source = self.source.as_ref().context("Open an image first")?;
        let input = source.input(params, &self.tables, false)?;
        let before = display_rgb(
            &input,
            &params.io.input_color_space,
            params.io.input_cctf_decoding,
            &self.tables,
        )?;
        let before_size = [u32::try_from(input.width)?, u32::try_from(input.height)?];
        let output =
            Simulator::new(&self.data, params.clone())?.process_owned(input, Tap::RgbOut)?;
        // Imported framing/upscale settings can enlarge the processed preview.
        let output = resize_preview(&output, PREVIEW_EDGE)?;
        let after = display_rgb(
            &output,
            &params.io.output_color_space,
            params.io.output_cctf_encoding,
            &self.tables,
        )?;
        Ok(Preview {
            before,
            after,
            before_size,
            after_size: [u32::try_from(output.width)?, u32::try_from(output.height)?],
            seconds: start.elapsed().as_secs_f64(),
        })
    }
    pub fn export(&self, params: &Params, path: &Path) -> Result<[usize; 2]> {
        let source = self.source.as_ref().context("Open an image first")?;
        ensure!(
            path != source.path,
            "Choose an output path different from the source"
        );
        if let (Ok(a), Ok(b)) = (path.canonicalize(), source.path.canonicalize()) {
            ensure!(a != b, "Output would overwrite the source");
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        ensure!(
            ["png", "tiff", "tif", "npy"].contains(&ext.as_str()),
            "Export extension must be .png, .tiff or .npy"
        );
        let sim = Simulator::new(&self.data, params.clone())?;
        let output = sim.process_owned(source.input(params, &self.tables, true)?, Tap::RgbOut)?;
        let temp = tempfile::NamedTempFile::new_in(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
        match ext.as_str() {
            "npy" => output.write_npy(temp.path())?,
            "tif" | "tiff" => output.write_tiff(temp.path())?,
            "png" => image::save_buffer_with_format(
                temp.path(),
                &display_rgb(
                    &output,
                    &params.io.output_color_space,
                    params.io.output_cctf_encoding,
                    &self.tables,
                )?,
                u32::try_from(output.width)?,
                u32::try_from(output.height)?,
                image::ColorType::Rgb8,
                image::ImageFormat::Png,
            )?,
            _ => bail!("Unsupported export format"),
        }
        temp.persist(path)?;
        Ok([output.width, output.height])
    }
}
