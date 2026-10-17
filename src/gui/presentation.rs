//! Presentation metadata copied from the pinned GPL-3.0 reference GUI.
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Spec {
    pub path: String,
    #[serde(default)]
    pub tooltip: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

pub fn specs() -> &'static [Spec] {
    static SPECS: OnceLock<Vec<Spec>> = OnceLock::new();
    SPECS.get_or_init(|| {
        #[derive(Deserialize)]
        struct Manifest {
            fields: Vec<Spec>,
        }
        serde_json::from_str::<Manifest>(include_str!("reference_parameters.json"))
            .expect("generated reference GUI metadata")
            .fields
    })
}

pub fn spec(key: &str) -> Option<&'static Spec> {
    specs().iter().find(|s| s.path == key).or_else(|| {
        // Vector components share their parent's tooltip, range and wheel step.
        let (parent, index) = key.rsplit_once('/')?;
        index.parse::<usize>().ok()?;
        spec(parent)
    })
}

pub fn tooltip(key: &str) -> String {
    // The reference uses one string for its matrix. Here each coefficient has
    // a numeric editor, so its string-parsing/fallback instructions don't apply.
    if key.starts_with("/convert/calibration") {
        return "3x3 device-correction matrix applied to the input before inversion, to undo scanner/camera colour rendering over the film dyes. Edit the individual numeric coefficients, or use Calibrate scanner to fit the matrix from the current image.".into();
    }
    if let Some(s) = spec(key)
        && !s.tooltip.is_empty()
    {
        return s.tooltip.clone();
    }
    let help = match key {
        "/camera/lens_blur_um" => {
            "Sigma of Gaussian blur for the camera lens, in micrometers on the film."
        }
        "/special/film_channel_swap" => {
            "Permute the film's cyan, magenta and yellow dye spectra. Sensitivity and development channels retain their order."
        }
        "/special/print_channel_swap" => {
            "Permute the print's cyan, magenta and yellow dye spectra. Sensitivity and development channels retain their order."
        }
        "/io/input_rotation" => {
            "Clockwise quarter turns applied before cropping, preview and scan."
        }
        "/io/copy_metadata" => {
            "Copy source EXIF, IPTC and XMP into supported output formats. Requires GExiv2."
        }
        "/io/output_bit_depth" => {
            "Bits per channel. Choose Automatic for PNG16, TIFF32 and EXR32. JPEG uses 8 and NPY uses 64."
        }
        "/io/output_compression" => "Lossless TIFF/EXR compression. Choose zip or none.",
        "/io/output_cctf_encoding" => {
            "Apply the output colour space's transfer function to the simulation output."
        }
        "/io/input_gamut_compress/boundary" => {
            "Use the inscribed hull or the spectral locus as the input compression boundary."
        }
        "/settings/preview_mode" => {
            "Preview and Scan select their processing mode automatically. This setting is retained in saved configurations."
        }
        "/settings/random_seed" => {
            "Optional fixed random seed for reproducible stochastic renders. Leave blank for a fresh seed."
        }
        "/settings/preview_max_size" => "max size of the long edge of the preview image in pixels",
        "/settings/apply_grain_presets" => {
            "Use the selected film stock's grain preset. Manual grain edits disable this option."
        }
        "/settings/apply_coupler_presets" => {
            "Use the selected film stock's DIR coupler preset. Manual coupler edits disable this option."
        }
        "/settings/neutral_print_filters_from_database" => {
            "Use neutral enlarger filters from the stock database. Manual neutral-filter edits disable this option."
        }
        "/settings/use_fast_stats" => "Use the fast statistical grain model.",
        "/settings/convert_lut_resolution" | "/settings/lut_resolution" => {
            "Number of samples per LUT axis. Higher values increase preparation time and memory."
        }
        "/settings/use_convert_lut" => "Use the conversion LUT for film conversion.",
        "/settings/use_enlarger_lut" => "Use the enlarger LUT for printing.",
        "/settings/use_scanner_lut" => "Use the scanner LUT for scan colour conversion.",
        "/debug/deactivate_spatial_effects" => "Disable spatial effects for diagnostic processing.",
        "/debug/deactivate_stochastic_effects" => {
            "Disable stochastic effects for diagnostic processing."
        }
        "/enlarger/normalize_print_exposure" => "Normalize the virtual enlarger's print exposure.",
        "/enlarger/y_filter_neutral"
        | "/enlarger/m_filter_neutral"
        | "/enlarger/c_filter_neutral" => {
            "Neutral enlarger filter position, in Kodak CC units. Manual edits disable database neutral filters."
        }
        "/io/output_gamut_compress/lightness_compression/enabled" => {
            "Enable output lightness compression."
        }
        _ if key.starts_with("/io/output_gamut_compress/lightness_compression/") => {
            "Lightness compression knee: threshold, limit and power."
        }
        _ => "",
    };
    help.into()
}

/// The native Slint tooltip sizes itself from its text. Insert word breaks so
/// long reference explanations stay readable on compact windows as well.
pub fn wrapped_tooltip(key: &str) -> String {
    let mut wrapped = String::new();
    let mut column = 0;
    for word in tooltip(key).split_whitespace() {
        let width = word.chars().count();
        if column != 0 {
            if column + 1 + width > 52 {
                wrapped.push('\n');
                column = 0;
            } else {
                wrapped.push(' ');
                column += 1;
            }
        }
        wrapped.push_str(word);
        column += width;
    }
    wrapped
}
