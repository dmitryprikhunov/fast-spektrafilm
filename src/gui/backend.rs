//! Image/session operations, independent of the Slint event loop.
use crate::{
    Image, Params, Simulator, Tap,
    color::{Tables, decode, encode, mat_vec},
    file_io,
    profile::Info,
};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    fs::File,
    path::{Path, PathBuf},
    time::Instant,
};

pub const PREVIEW_EDGE: usize = 768;
pub const ROUTES: [&str; 6] = [
    "input > film > print > scan",
    "input > film > scan",
    "input",
    "input > convert-film > print > scan",
    "input > convert-film > scan",
    "input > convert-film > scan-minus-base",
];
pub const ROUTE_LABELS: [&str; 6] = [
    "Film → Print → Scan",
    "Film → Scan",
    "Passthrough",
    "Negative → Print → Scan",
    "Negative → Scan",
    "Negative → Scan (remove base)",
];

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
            label: header.info.name.replace("Professional ", ""),
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

pub fn default_params_path() -> Result<PathBuf> {
    let directory = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")))
        .context("Cannot locate the user configuration directory")?;
    Ok(directory.join("fast-spektrafilm/default.json"))
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
/// Cache includes the input interpretation and RAW processing settings. Editing
/// any of these requires decoding again; output-only edits reuse the preview.
pub struct Source {
    pub path: PathBuf,
    pub preview: Image,
    pub original: [usize; 2],
    key: String,
}
impl Source {
    fn key(p: &Params) -> String {
        serde_json::to_string(&(&p.raw, &p.io.input_color_space, p.io.input_cctf_decoding)).unwrap()
    }
    pub fn matches(&self, path: &Path, p: &Params) -> bool {
        self.path == path && self.key == Self::key(p)
    }
    pub fn load(path: &Path, data: &Path, p: &Params) -> Result<Self> {
        // Keep the common RAW preview bounded. Custom white balance / lens
        // correction run before reduction through the shared full decoder.
        let (image, original) = if !file_io::is_rgb_file(path)
            && file_io::extension(path) != "npy"
            && p.raw.white_balance == "as_shot"
            && !p.raw.lens_correction
            && !p.io.input_cctf_decoding
        {
            let (mut image, original) = crate::image::decode_raw_preview(path, true, PREVIEW_EDGE)?;
            let matrix = Tables::load(data)?.rgb_to_rgb("ACES2065-1", &p.io.input_color_space)?;
            for pixel in image.data.chunks_exact_mut(3) {
                pixel.copy_from_slice(&mat_vec(&matrix, [pixel[0], pixel[1], pixel[2]]));
            }
            (image, original)
        } else {
            let image = file_io::read(path, data, p, None)?;
            let original = [image.width, image.height];
            (image, original)
        };
        ensure!(image.channels == 3, "Input must be RGB (3 channels)");
        Ok(Self {
            path: path.into(),
            original,
            preview: resize_preview(&image, PREVIEW_EDGE)?,
            key: Self::key(p),
        })
    }
    pub fn input(&self, params: &Params, data: &Path, full: bool) -> Result<Image> {
        if full {
            file_io::read(&self.path, data, params, None)
        } else {
            ensure!(
                self.key == Self::key(params),
                "Input settings changed; reload the preview"
            );
            Ok(self.preview.clone())
        }
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

/// Convert the processing representation to encoded sRGB for display.
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
struct Scan {
    path: PathBuf,
    keys: [String; 2],
    output: Image,
}

pub struct Session {
    pub data: PathBuf,
    tables: Tables,
    pub source: Option<Source>,
    scan: Option<Scan>,
}
impl Session {
    pub fn new(data: &Path) -> Result<Self> {
        Ok(Self {
            data: data.into(),
            tables: Tables::load(data)?,
            source: None,
            scan: None,
        })
    }
    pub fn set_source(&mut self, source: Source) {
        self.scan = None;
        self.source = Some(source);
    }
    fn processing_key(params: &Params) -> Result<String> {
        let mut params = params.clone();
        params.io.monitor_profile = None;
        params.io.copy_metadata = false;
        Ok(serde_json::to_string(&params)?)
    }
    fn scan_keys(&self, params: &Params) -> Result<[String; 2]> {
        // The UI resolves stock presets after rendering. Accept both the request
        // and its resolved settings without constructing a simulator on Save.
        Ok([
            Self::processing_key(params)?,
            Self::processing_key(&self.resolved_params(params)?)?,
        ])
    }
    fn ensure_scan(&mut self, params: &Params) -> Result<()> {
        let source = self.source.as_ref().context("Open an image first")?;
        let key = Self::processing_key(params)?;
        if self
            .scan
            .as_ref()
            .is_some_and(|scan| scan.path == source.path && scan.keys.contains(&key))
        {
            return Ok(());
        }
        self.scan = None;
        let output = Simulator::new(&self.data, params.clone())?
            .process_owned(source.input(params, &self.data, true)?, Tap::RgbOut)?;
        self.scan = Some(Scan {
            path: source.path.clone(),
            keys: self.scan_keys(params)?,
            output,
        });
        Ok(())
    }
    pub fn resolved_params(&self, p: &Params) -> Result<Params> {
        let sim = Simulator::new(&self.data, p.clone())?;
        let mut out = p.clone();
        if p.settings.apply_grain_presets {
            crate::grain::apply_presets(&self.data, sim.film(), &mut out.grain)?;
        }
        if p.settings.apply_coupler_presets {
            crate::pipeline::apply_coupler_presets(&self.data, sim.film(), &mut out.dir_couplers)?;
        }
        if p.settings.neutral_print_filters_from_database {
            out.enlarger.y_filter_neutral = sim.params().enlarger.y_filter_neutral;
            out.enlarger.m_filter_neutral = sim.params().enlarger.m_filter_neutral;
            out.enlarger.c_filter_neutral = sim.params().enlarger.c_filter_neutral;
        }
        Ok(out)
    }
    pub fn validate_params(&self, params: &Params) -> Result<()> {
        Simulator::new(&self.data, params.clone())?;
        Ok(())
    }
    pub fn render(&mut self, params: &Params) -> Result<Preview> {
        self.render_size(params, false)
    }
    fn display(
        &self,
        image: &Image,
        space: &str,
        encoded: bool,
        monitor: Option<&str>,
    ) -> Result<Vec<u8>> {
        if let Some(path) = monitor.filter(|p| !p.is_empty()) {
            let mut image = image.clone();
            let matrix = self.tables.rgb_to_rgb(space, "sRGB")?;
            for pixel in image.data.chunks_exact_mut(3) {
                let rgb = [pixel[0], pixel[1], pixel[2]]
                    .map(|v| if encoded { decode(space, v) } else { v });
                pixel.copy_from_slice(&mat_vec(&matrix, rgb).map(|v| encode("sRGB", v)));
            }
            file_io::transform_icc(
                &mut image,
                &file_io::icc(&self.data, "sRGB", true)?,
                &std::fs::read(path)?,
            )?;
            Ok(image
                .data
                .iter()
                .map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
                .collect())
        } else {
            display_rgb(image, space, encoded, &self.tables)
        }
    }
    pub fn render_size(&mut self, params: &Params, full: bool) -> Result<Preview> {
        let start = Instant::now();
        if full {
            self.scan = None; // Release the old full image before allocating its replacement.
        }
        let source = self.source.as_ref().context("Open an image first")?;
        let input = source.input(params, &self.data, full)?;
        let before = self.display(
            &input,
            &params.io.input_color_space,
            params.io.input_cctf_decoding,
            params.io.monitor_profile.as_deref(),
        )?;
        let before_size = [u32::try_from(input.width)?, u32::try_from(input.height)?];
        let output =
            Simulator::new(&self.data, params.clone())?.process_owned(input, Tap::RgbOut)?;
        // Imported framing/upscale settings can enlarge the processed preview.
        let output = if full {
            output
        } else {
            resize_preview(&output, PREVIEW_EDGE)?
        };
        let after = self.display(
            &output,
            &params.io.output_color_space,
            params.io.output_cctf_encoding,
            params.io.monitor_profile.as_deref(),
        )?;
        let after_size = [u32::try_from(output.width)?, u32::try_from(output.height)?];
        if full {
            self.scan = Some(Scan {
                path: source.path.clone(),
                keys: self.scan_keys(params)?,
                output,
            });
        }
        Ok(Preview {
            before,
            after,
            before_size,
            after_size,
            seconds: start.elapsed().as_secs_f64(),
        })
    }
    pub fn calibrate(&self, p: &Params, device: bool) -> Result<Params> {
        let mut p = p.clone();
        let mut image = self
            .source
            .as_ref()
            .context("Open a scanned negative first")?
            .input(&p, &self.data, false)?;
        if p.io.input_cctf_decoding {
            image
                .data
                .iter_mut()
                .for_each(|v| *v = decode(&p.io.input_color_space, *v));
        }
        if device {
            let sim = Simulator::new(&self.data, p.clone())?;
            p.convert.calibration =
                crate::convert::FilmConverter::new(&self.tables, sim.film(), &p)?
                    .fit_calibration(&image)?;
        } else {
            let sample = crate::convert::sample_base(&image, p.convert.base_percentile)?;
            let mut stock = crate::profile::Profile::load(&self.data, &p.film_profile)?;
            stock.swap_dyes(p.special.film_channel_swap)?;
            (p.film_base, p.convert.exposure_compensation_ev) =
                crate::convert::fit_base(&self.tables, &stock, &p, sample)?;
        }
        Ok(p)
    }
    pub fn export(&mut self, params: &Params, path: &Path) -> Result<[usize; 2]> {
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
            ["png", "jpg", "jpeg", "exr", "tiff", "tif", "npy"].contains(&ext.as_str()),
            "Choose PNG, JPEG, TIFF, EXR or NPY"
        );
        let source_path = source.path.clone();
        self.ensure_scan(params)?;
        let output = &self.scan.as_ref().unwrap().output;
        let temp = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile_in(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new(".")),
            )?;
        file_io::write(
            output,
            temp.path(),
            &self.data,
            &params.io.output_color_space,
            params.io.output_cctf_encoding,
        )?;
        if params.io.copy_metadata {
            crate::metadata::copy(
                &source_path,
                temp.path(),
                [output.width, output.height],
                &params.io.output_color_space,
                params.io.output_cctf_encoding,
            )?;
        }
        temp.persist(path)?;
        Ok([output.width, output.height])
    }
}
