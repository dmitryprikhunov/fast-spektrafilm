use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Params {
    pub film_profile: String,
    pub print_profile: String,
    pub special: Special,
    pub convert: ConvertFilm,
    pub raw: RawSettings,
    pub camera: Camera,
    pub enlarger: Enlarger,
    pub scanner: Scanner,
    pub io: Io,
    pub workflow: Workflow,
    pub debug: Debug,
    pub settings: Settings,
    pub dir_couplers: Couplers,
    pub halation: Halation,
    pub grain: Grain,
    pub print_glare: Glare,
    pub film_chemistry: Chemistry,
    pub print_chemistry: Chemistry,
    pub film_base: Base,
    pub print_base: Base,
}
impl Default for Params {
    fn default() -> Self {
        Self {
            film_profile: "kodak_portra_400".into(),
            print_profile: "kodak_portra_endura".into(),
            special: Special::default(),
            convert: ConvertFilm::default(),
            raw: RawSettings::default(),
            camera: Camera::default(),
            enlarger: Enlarger::default(),
            scanner: Scanner::default(),
            io: Io::default(),
            workflow: Workflow::default(),
            debug: Debug::default(),
            settings: Settings::default(),
            dir_couplers: Couplers::default(),
            halation: Halation::default(),
            grain: Grain::default(),
            print_glare: Glare::default(),
            film_chemistry: Chemistry::default(),
            print_chemistry: Chemistry::default(),
            film_base: Base::default(),
            print_base: Base::default(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Base {
    pub active: bool,
    pub scale: f64,
    pub tilt: f64,
    pub cyan: f64,
    pub magenta: f64,
    pub yellow: f64,
}
impl Default for Base {
    fn default() -> Self {
        Self {
            active: true,
            scale: 1.0,
            tilt: 0.0,
            cyan: 1.0,
            magenta: 1.0,
            yellow: 1.0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Chemistry {
    pub active: bool,
    pub development_time: Option<f64>,
    pub gamma_factor: f64,
    pub gamma_factor_fast: f64,
    pub gamma_factor_slow: f64,
    pub gamma_factor_red: f64,
    pub gamma_factor_green: f64,
    pub gamma_factor_blue: f64,
    pub developer_exhaustion: f64,
}
impl Default for Chemistry {
    fn default() -> Self {
        Self {
            active: true,
            development_time: None,
            gamma_factor: 1.0,
            gamma_factor_fast: 1.0,
            gamma_factor_slow: 1.0,
            gamma_factor_red: 1.0,
            gamma_factor_green: 1.0,
            gamma_factor_blue: 1.0,
            developer_exhaustion: 0.0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grain {
    pub active: bool,
    pub rms_granularity: Vec<f64>,
    pub density_min: Vec<f64>,
    pub uniformity: Vec<f64>,
    pub particle_scale_sublayers: Vec<f64>,
    pub blur: f64,
    pub mult_usm_sigma: f64,
    pub mult_usm_amount: f64,
    pub blur_dye_clouds_um: f64,
    pub micro_structure: [f64; 2],
}
impl Default for Grain {
    fn default() -> Self {
        Self {
            active: true,
            rms_granularity: vec![6.0, 8.0, 10.0],
            density_min: vec![0.03; 3],
            uniformity: vec![0.97; 3],
            particle_scale_sublayers: vec![1.0, 0.5, 0.25],
            blur: 0.89,
            mult_usm_sigma: 0.7,
            mult_usm_amount: 1.5,
            blur_dye_clouds_um: 2.0,
            micro_structure: [0.2, 30.0],
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Camera {
    pub diffusion_filter: DiffusionFilter,
    pub exposure_compensation_ev: f64,
    pub auto_exposure: bool,
    pub auto_exposure_method: String,
    pub lens_blur_um: f64,
    pub film_format_mm: f64,
    pub color_filter: String,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            diffusion_filter: DiffusionFilter::default(),
            exposure_compensation_ev: 0.0,
            auto_exposure: true,
            auto_exposure_method: "center_weighted".into(),
            lens_blur_um: 0.0,
            film_format_mm: 35.0,
            color_filter: "none".into(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Enlarger {
    pub diffusion_filter: DiffusionFilter,
    pub illuminant: String,
    pub print_exposure: f64,
    pub print_exposure_compensation: bool,
    pub normalize_print_exposure: bool,
    pub y_filter_shift: f64,
    pub m_filter_shift: f64,
    pub y_filter_neutral: f64,
    pub m_filter_neutral: f64,
    pub c_filter_neutral: f64,
    /// Retained for config compatibility. The pinned experimental printing
    /// stage does not apply this parameter to the image.
    pub lens_blur: f64,
    pub preflash_exposure: f64,
    pub preflash_y_filter_shift: f64,
    pub preflash_m_filter_shift: f64,
}
impl Default for Enlarger {
    fn default() -> Self {
        Self {
            diffusion_filter: DiffusionFilter::default(),
            illuminant: "TH-KG3".into(),
            print_exposure: 1.0,
            print_exposure_compensation: true,
            normalize_print_exposure: true,
            y_filter_shift: 0.0,
            m_filter_shift: 0.0,
            y_filter_neutral: 55.0,
            m_filter_neutral: 65.0,
            c_filter_neutral: 0.0,
            lens_blur: 0.0,
            preflash_exposure: 0.0,
            preflash_y_filter_shift: 0.0,
            preflash_m_filter_shift: 0.0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scanner {
    pub lens_blur: f64,
    pub white_correction: bool,
    pub black_correction: bool,
    pub white_level: f64,
    pub black_level: f64,
    pub unsharp_mask: [f64; 2],
}
impl Default for Scanner {
    fn default() -> Self {
        Self {
            lens_blur: 0.0,
            white_correction: false,
            black_correction: false,
            white_level: 0.98,
            black_level: 0.01,
            unsharp_mask: [0.7, 0.7],
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Io {
    /// Optional display-only monitor ICC profile; exports retain their output profile.
    pub monitor_profile: Option<String>,
    pub copy_metadata: bool,
    pub input_color_space: String,
    pub input_cctf_decoding: bool,
    pub output_color_space: String,
    pub output_cctf_encoding: bool,
    pub input_gamut_compress: InputGamut,
    pub output_gamut_compress: OutputGamut,
    pub crop: bool,
    pub crop_center: [f64; 2],
    pub crop_size: [f64; 2],
    pub upscale_factor: f64,
}
impl Default for Io {
    fn default() -> Self {
        Self {
            monitor_profile: None,
            copy_metadata: true,
            input_color_space: "ProPhoto RGB".into(),
            input_cctf_decoding: false,
            output_color_space: "sRGB".into(),
            output_cctf_encoding: true,
            input_gamut_compress: InputGamut::default(),
            output_gamut_compress: OutputGamut::default(),
            crop: false,
            crop_center: [0.5, 0.5],
            crop_size: [0.1, 0.1],
            upscale_factor: 1.0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InputGamut {
    pub active: bool,
    pub algorithm: String,
    pub boundary: String,
    pub hull_detail: f64,
    pub knee: [f64; 3],
}
impl Default for InputGamut {
    fn default() -> Self {
        Self {
            active: true,
            algorithm: "xy".into(),
            boundary: "inscribed_hull".into(),
            hull_detail: 5.0,
            knee: [0.815, 1.0, 1.2],
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OutputGamut {
    pub algorithm: String,
    pub knee: [f64; 3],
    pub lightness_compression: Option<[f64; 3]>,
}
impl Default for OutputGamut {
    fn default() -> Self {
        Self {
            algorithm: "oklch".into(),
            knee: [0.95, 1.0, 1.6],
            lightness_compression: Some([0.95, 1.0, 1.6]),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Workflow {
    pub route: String,
}
impl Default for Workflow {
    fn default() -> Self {
        Self {
            route: "input > film > print > scan".into(),
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Debug {
    pub deactivate_spatial_effects: bool,
    pub deactivate_stochastic_effects: bool,
    pub lut_mode: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub rgb_to_raw_method: String,
    pub apply_hanatos2025_adaptation_window: bool,
    pub apply_hanatos2025_adaptation_surface: bool,
    pub spectral_gaussian_blur: f64,
    pub use_convert_lut: bool,
    pub convert_lut_resolution: usize,
    pub use_enlarger_lut: bool,
    pub use_scanner_lut: bool,
    pub lut_resolution: usize,
    pub neutral_print_filters_from_database: bool,
    pub apply_coupler_presets: bool,
    pub apply_grain_presets: bool,
    pub use_fast_stats: bool,
    /// Fixed random seed for reproducible Rust stochastic renders.
    pub random_seed: Option<u32>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            rgb_to_raw_method: "hanatos2025".into(),
            apply_hanatos2025_adaptation_window: true,
            apply_hanatos2025_adaptation_surface: false,
            spectral_gaussian_blur: 0.0,
            use_convert_lut: false,
            convert_lut_resolution: 33,
            use_enlarger_lut: false,
            use_scanner_lut: false,
            lut_resolution: 17,
            neutral_print_filters_from_database: true,
            apply_coupler_presets: true,
            apply_grain_presets: true,
            use_fast_stats: true,
            random_seed: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Glare {
    pub active: bool,
    pub percent: f64,
    pub roughness: f64,
    pub blur: f64,
}
impl Default for Glare {
    fn default() -> Self {
        Self {
            active: true,
            percent: 0.03,
            roughness: 0.7,
            blur: 0.5,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Couplers {
    pub active: bool,
    pub amount: f64,
    pub inhibition_samelayer: f64,
    pub inhibition_interlayer: f64,
    pub gamma_samelayer_rgb: [f64; 3],
    pub gamma_interlayer_r_to_gb: [f64; 2],
    pub gamma_interlayer_g_to_rb: [f64; 2],
    pub gamma_interlayer_b_to_rg: [f64; 2],
    pub langmuir_donor_k_rgb: [f64; 3],
    pub langmuir_receiver_k_rgb: [f64; 3],
    pub diffusion_size_um: f64,
    pub diffusion_tail_um: f64,
    pub diffusion_tail_weight: f64,
}
impl Default for Couplers {
    fn default() -> Self {
        Self {
            active: true,
            amount: 1.0,
            inhibition_samelayer: 1.0,
            inhibition_interlayer: 1.0,
            gamma_samelayer_rgb: [0.341, 0.324, 0.273],
            gamma_interlayer_r_to_gb: [0.355, 0.305],
            gamma_interlayer_g_to_rb: [0.154, 0.358],
            gamma_interlayer_b_to_rg: [0.171, 0.225],
            langmuir_donor_k_rgb: [1.0; 3],
            langmuir_receiver_k_rgb: [1.0; 3],
            diffusion_size_um: 20.0,
            diffusion_tail_um: 200.0,
            diffusion_tail_weight: 0.03,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Halation {
    pub active: bool,
    pub boost_ev: f64,
    pub boost_range: f64,
    pub protect_ev: f64,
    pub scatter_amount: f64,
    pub scatter_spatial_scale: f64,
    pub halation_amount: f64,
    pub halation_spatial_scale: f64,
    pub scatter_core_um: [f64; 3],
    pub scatter_tail_um: [f64; 3],
    pub scatter_tail_weight: [f64; 3],
    pub halation_strength: [f64; 3],
    pub halation_first_sigma_um: [f64; 3],
    pub halation_n_bounces: usize,
    pub halation_bounce_decay: f64,
    pub halation_renormalize: bool,
}
impl Default for Halation {
    fn default() -> Self {
        Self {
            active: true,
            boost_ev: 0.0,
            boost_range: 0.3,
            protect_ev: 4.0,
            scatter_amount: 1.0,
            scatter_spatial_scale: 1.0,
            halation_amount: 1.0,
            halation_spatial_scale: 1.0,
            scatter_core_um: [2.2, 2.0, 1.6],
            scatter_tail_um: [9.3, 9.7, 9.1],
            scatter_tail_weight: [0.78, 0.65, 0.67],
            halation_strength: [0.05, 0.015, 0.0],
            halation_first_sigma_um: [65.0; 3],
            halation_n_bounces: 3,
            halation_bounce_decay: 0.5,
            halation_renormalize: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiffusionFilter {
    pub active: bool,
    pub filter_family: String,
    pub strength: f64,
    pub spatial_scale: f64,
    pub halo_warmth: f64,
    pub core_intensity: f64,
    pub core_size: f64,
    pub halo_intensity: f64,
    pub halo_size: f64,
    pub bloom_intensity: f64,
    pub bloom_size: f64,
}
impl Default for DiffusionFilter {
    fn default() -> Self {
        Self {
            active: false,
            filter_family: "black_pro_mist".into(),
            strength: 0.5,
            spatial_scale: 1.0,
            halo_warmth: 0.0,
            core_intensity: 1.0,
            core_size: 1.0,
            halo_intensity: 1.0,
            halo_size: 1.0,
            bloom_intensity: 1.0,
            bloom_size: 1.0,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConvertFilm {
    pub scan_illuminant: String,
    pub exposure_compensation_ev: f64,
    pub base_percentile: f64,
    pub calibration: [[f64; 3]; 3],
}
impl Default for ConvertFilm {
    fn default() -> Self {
        Self {
            scan_illuminant: "D55".into(),
            exposure_compensation_ev: 0.0,
            base_percentile: 99.0,
            calibration: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RawSettings {
    pub lens_correction: bool,
    pub white_balance: String,
    pub temperature: f64,
    pub tint: f64,
}
impl Default for RawSettings {
    fn default() -> Self {
        Self {
            lens_correction: false,
            white_balance: "as_shot".into(),
            temperature: 5500.,
            tint: 1.,
        }
    }
}
impl Workflow {
    pub fn scan_film(&self) -> bool {
        matches!(
            self.route.as_str(),
            "input > film > scan"
                | "input > convert-film > scan"
                | "input > convert-film > scan-minus-base"
        )
    }
    pub fn convert_film(&self) -> bool {
        self.route.contains("convert-film")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Special {
    pub film_channel_swap: [usize; 3],
    pub print_channel_swap: [usize; 3],
}
impl Default for Special {
    fn default() -> Self {
        Self {
            film_channel_swap: [0, 1, 2],
            print_channel_swap: [0, 1, 2],
        }
    }
}
