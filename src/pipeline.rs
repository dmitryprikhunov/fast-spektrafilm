// Indexing makes the donor/receiver and wavelength/channel axes explicit.
#![allow(clippy::needless_range_loop)]
use crate::{
    Image,
    color::{Compressor, Matrix, Tables, decode, encode, mat_vec},
    interpolation::{linear, tri_to_quad},
    params::{Couplers, Params},
    profile::Profile,
    spectral::{FilmResponse, integrate},
};
use anyhow::{Result, bail, ensure};
use rayon::prelude::*;
use std::{path::Path, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Tap {
    RgbIn,
    RgbPre,
    LogEFilm,
    CmyFilm,
    LogEPrint,
    CmyPrint,
    RgbOut,
}
impl Tap {
    pub fn name(self) -> &'static str {
        match self {
            Self::RgbIn => "rgb_in",
            Self::RgbPre => "rgb_pre",
            Self::LogEFilm => "log_e_film",
            Self::CmyFilm => "cmy_film",
            Self::LogEPrint => "log_e_print",
            Self::CmyPrint => "cmy_print",
            Self::RgbOut => "rgb_out",
        }
    }
}

/// Timings belong to one call, so concurrent library callers never share state.
pub struct ProcessReport {
    pub image: Image,
    pub stages: std::collections::BTreeMap<String, f64>,
    pub total_seconds: f64,
}

struct CouplerModel {
    matrix: [[f64; 3]; 3],
    max: [f64; 3],
    k: [f64; 3],
    reference: [f64; 3],
    receiver_k: [f64; 3],
    receiver_ref: [f64; 3],
    positive: bool,
    curves: Vec<Vec<f64>>,
    active: bool,
}
impl CouplerModel {
    fn new(film: &Profile, params: &Couplers) -> Self {
        let ch = film.channels();
        let max = film.max();
        let mut matrix = [[0.0; 3]; 3];
        for i in 0..ch {
            matrix[i][i] = params.gamma_samelayer_rgb[i] * params.inhibition_samelayer;
        }
        if ch == 3 {
            matrix[0][1] = params.gamma_interlayer_r_to_gb[0] * params.inhibition_interlayer;
            matrix[0][2] = params.gamma_interlayer_r_to_gb[1] * params.inhibition_interlayer;
            matrix[1][0] = params.gamma_interlayer_g_to_rb[0] * params.inhibition_interlayer;
            matrix[1][2] = params.gamma_interlayer_g_to_rb[1] * params.inhibition_interlayer;
            matrix[2][0] = params.gamma_interlayer_b_to_rg[0] * params.inhibition_interlayer;
            matrix[2][1] = params.gamma_interlayer_b_to_rg[1] * params.inhibition_interlayer;
        }
        let mut model = Self {
            matrix,
            max: [0.0; 3],
            k: [0.0; 3],
            reference: [0.0; 3],
            receiver_k: [0.0; 3],
            receiver_ref: [0.0; 3],
            positive: film.info.polarity == "positive",
            curves: film.curves.clone(),
            active: params.active,
        };
        for c in 0..ch {
            model.max[c] = max[c];
            model.reference[c] = 0.5 * max[c];
            model.k[c] = params.langmuir_donor_k_rgb[c] * max[c];
        }
        for m in 0..ch {
            for k in 0..ch {
                model.receiver_ref[m] += model.reference[k] * matrix[k][m];
            }
            model.receiver_k[m] = params.langmuir_receiver_k_rgb[m] * 2.0 * model.receiver_ref[m];
        }
        for row in &mut model.matrix {
            for v in row {
                *v *= params.amount;
            }
        }
        let mut axis = vec![vec![0.0; film.axis.len()]; ch];
        for i in 0..film.axis.len() {
            let density =
                std::array::from_fn::<_, 3, _>(|c| if c < ch { film.curves[c][i] } else { 0.0 });
            let corr = model.correction(&density, ch);
            for c in 0..ch {
                axis[c][i] = film.axis[i] - corr[c];
            }
        }
        for c in 0..ch {
            for (i, x) in film.axis.iter().enumerate() {
                model.curves[c][i] = linear(*x, &axis[c], &film.curves[c]);
            }
        }
        model
    }
    fn correction_raw(&self, density: &[f64], channels: usize) -> [f64; 3] {
        let mut out = [0.0; 3];
        for k in 0..channels {
            let silver = if self.positive {
                self.max[k] - density[k]
            } else {
                density[k]
            };
            let donor = if self.positive || !self.k[k].is_finite() {
                silver
            } else {
                silver * (self.k[k] + self.reference[k]) / (self.k[k] + silver)
            };
            for m in 0..channels {
                out[m] += donor * self.matrix[k][m];
            }
        }
        out
    }
    fn receive(&self, out: &mut [f64]) {
        if self.positive {
            for m in 0..out.len() {
                if self.receiver_k[m].is_finite() {
                    out[m] = out[m] * (self.receiver_k[m] + self.receiver_ref[m])
                        / (self.receiver_k[m] + out[m]);
                }
            }
        }
    }
    fn correction(&self, density: &[f64], channels: usize) -> [f64; 3] {
        let mut out = self.correction_raw(density, channels);
        self.receive(&mut out[..channels]);
        out
    }
    fn develop(&self, film: &Profile, log_e: &[f64], out: &mut [f64]) {
        film.density(log_e, out);
        if !self.active {
            return;
        }
        let correction = self.correction(out, film.channels());
        for c in 0..film.channels() {
            out[c] = linear(log_e[c] - correction[c], &film.axis, &self.curves[c]);
        }
    }
}

pub struct Simulator {
    params: Params,
    resolved_params: Params,
    converter: Option<crate::convert::FilmConverter>,
    convert_lut: Option<std::sync::Arc<crate::density_lut::DensityLut>>,
    scan_profile: Profile,
    film: Profile,
    print: Profile,
    response: std::sync::Arc<FilmResponse>,
    input_matrix: Matrix,
    output_matrix: Matrix,
    passthrough_matrix: Matrix,
    output_encode_matrix: Matrix,
    couplers: CouplerModel,
    print_weights: Vec<[f64; 3]>,
    scan_weights: Vec<[f64; 3]>,
    print_factor: f64,
    preflash: [f64; 3],
    compressor: Compressor,
    scan_white_xyz: [f64; 3],
    scan_reference: crate::scan_reference::ScanReference,
    enlarger_lut: Option<std::sync::Arc<crate::density_lut::DensityLut>>,
    scanner_lut: Option<std::sync::Arc<crate::density_lut::DensityLut>>,
    pub setup_seconds: f64,
}
impl Simulator {
    /// Digested parameters; construct a new simulator after changing settings.
    pub fn params(&self) -> &Params {
        &self.params
    }
    /// Presets resolved before preview/debug deactivation, suitable for UI keys.
    pub fn resolved_params(&self) -> &Params {
        &self.resolved_params
    }
    pub fn film(&self) -> &Profile {
        &self.film
    }
    pub fn print(&self) -> &Profile {
        &self.print
    }
    pub fn new(data: &Path, params: Params) -> Result<Self> {
        let start = Instant::now();
        let PreparedParams {
            params,
            resolved_params,
            tables,
            mut film,
            mut print,
            stock_film_base,
            stock_print_base,
        } = prepare_params(data, params)?;
        let response = FilmResponse::shared(data, &tables, &film, &params)?;
        let input_matrix =
            tables.rgb_to_xyz(&params.io.input_color_space, response.projection_white)?;
        let midgray_matrix = tables.rgb_to_xyz("sRGB", response.projection_white)?;
        let output_encode_matrix =
            tables.rgb_to_rgb(&params.io.output_color_space, &params.io.output_color_space)?;
        let passthrough_matrix =
            tables.rgb_to_rgb(&params.io.input_color_space, &params.io.output_color_space)?;
        // Midgray uses the stock curves including their toe; development removes
        // the toe afterwards, exactly as the reference initialization does.
        let midgray_raw = raw_pixel(&response, &midgray_matrix, [0.184; 3]);
        let auto_scale = if params.camera.auto_exposure {
            crate::metering::scale(
                &Image {
                    width: 1,
                    height: 1,
                    channels: film.channels(),
                    data: midgray_raw[..film.channels()].to_vec(),
                },
                &params.camera.auto_exposure_method,
            )
        } else {
            1.0
        };
        let density_mid = |ev: f64| {
            let raw = raw_pixel(&response, &midgray_matrix, [0.184 * 2.0_f64.powf(ev); 3]);
            let logs = raw.map(|v| (v * auto_scale + 1e-10).log10());
            let mut d = [0.0; 3];
            film.density(&logs, &mut d);
            d
        };
        let mid = density_mid(0.0);
        let mid_comp = density_mid(params.camera.exposure_compensation_ev);
        let lamp = tables.illuminant(&params.enlarger.illuminant)?;
        let filters = [
            params.enlarger.c_filter_neutral,
            params.enlarger.m_filter_neutral + params.enlarger.m_filter_shift,
            params.enlarger.y_filter_neutral + params.enlarger.y_filter_shift,
        ];
        let filtered = filtered_illuminant(&tables, &lamp.spectrum, filters);
        let print_weights = (0..81)
            .map(|w| {
                std::array::from_fn(|c| {
                    if c < print.channels() {
                        filtered[w] * print.sensitivity[w][c]
                    } else {
                        0.0
                    }
                })
            })
            .collect::<Vec<_>>();
        let factor = |d: &[f64]| {
            let mut raw = [0.0; 3];
            integrate(&film, d, &print_weights, print.channels(), &mut raw);
            1.0 / (raw[..print.channels()]
                .iter()
                .map(|v| v.max(1e-10).ln())
                .sum::<f64>()
                / print.channels() as f64)
                .exp()
        };
        let fmid = factor(&mid[..film.channels()]);
        let fcomp = factor(&mid_comp[..film.channels()]);
        let print_factor = match (
            params.enlarger.normalize_print_exposure,
            params.enlarger.print_exposure_compensation,
        ) {
            (true, true) => fcomp,
            (true, false) => fmid,
            (false, true) => fcomp / fmid,
            (false, false) => 1.0,
        };
        let mut preflash = [0.0; 3];
        if params.enlarger.preflash_exposure > 0.0 {
            let pf = filtered_illuminant(
                &tables,
                &lamp.spectrum,
                [
                    params.enlarger.c_filter_neutral,
                    params.enlarger.m_filter_neutral + params.enlarger.preflash_m_filter_shift,
                    params.enlarger.y_filter_neutral + params.enlarger.preflash_y_filter_shift,
                ],
            );
            for w in 0..81 {
                if stock_film_base[w].is_nan() {
                    continue;
                }
                for c in 0..print.channels() {
                    preflash[c] += 10.0_f64.powf(-stock_film_base[w])
                        * pf[w]
                        * print.sensitivity[w][c]
                        * params.enlarger.preflash_exposure;
                }
            }
        }
        let scan_profile = if params.workflow.scan_film() {
            &film
        } else {
            &print
        };
        let mut scan_profile = scan_profile.clone();
        if params.workflow.route == "input > convert-film > scan-minus-base" {
            scan_profile.base.fill(0.0);
        }
        let scan_illuminant = tables.illuminant(
            if params.workflow.convert_film() && params.workflow.scan_film() {
                &params.convert.scan_illuminant
            } else {
                &scan_profile.info.viewing_illuminant
            },
        )?;
        let norm: f64 = (0..81)
            .map(|w| scan_illuminant.spectrum[w] * tables.cmfs[w][1])
            .sum();
        let scan_weights: Vec<[f64; 3]> = (0..81)
            .map(|w| tables.cmfs[w].map(|v| v * scan_illuminant.spectrum[w] / norm))
            .collect();
        let scan_white_xyz = std::array::from_fn(|c| {
            (0..81)
                .map(|w| tables.cmfs[w][c] * scan_illuminant.spectrum[w])
                .sum::<f64>()
                / norm
        });
        let output_matrix = tables.xyz_to_rgb(&params.io.output_color_space, scan_illuminant.xy)?;
        let compressor = Compressor::new(
            &tables,
            &params.io.output_color_space,
            &params.io.output_gamut_compress,
        )?;
        let scan_reference = crate::scan_reference::ScanReference::new(
            &tables,
            &params,
            &film,
            &print,
            &stock_film_base,
            &stock_print_base,
            &print_weights,
            &scan_weights,
            print_factor,
            preflash,
        )?;
        let film_min: Vec<_> = (0..film.channels())
            .map(|c| -params.grain.density_min[c])
            .collect();
        let enlarger_lut = if params.settings.use_enlarger_lut {
            let key = serde_json::to_string(&(
                "enlarger",
                &film.dyes,
                &film.base,
                &film_min,
                film.max(),
                &print_weights,
                print.channels(),
                print_factor,
                preflash,
                params.settings.lut_resolution,
            ))?;
            Some(crate::density_lut::DensityLut::shared(key, || {
                crate::density_lut::DensityLut::new(
                    film_min.clone(),
                    film.max(),
                    print.channels(),
                    params.settings.lut_resolution,
                    |d| {
                        let mut raw = [0.0; 3];
                        integrate(&film, d, &print_weights, print.channels(), &mut raw);
                        std::array::from_fn(|c| {
                            ((raw[c] * print_factor + preflash[c]).max(0.0) + 1e-10).log10()
                        })
                    },
                )
            })?)
        } else {
            None
        };
        let scanner_lut = if params.settings.use_scanner_lut {
            let profile = &scan_profile;
            let minimum = if params.workflow.scan_film() {
                film_min
            } else {
                print.min()
            };
            let key = serde_json::to_string(&(
                "scanner",
                &profile.dyes,
                &profile.base,
                &minimum,
                profile.max(),
                &scan_weights,
                params.settings.lut_resolution,
            ))?;
            Some(crate::density_lut::DensityLut::shared(key, || {
                crate::density_lut::DensityLut::new(
                    minimum,
                    profile.max(),
                    3,
                    params.settings.lut_resolution,
                    |d| {
                        let mut raw = [0.0; 3];
                        integrate(profile, d, &scan_weights, 3, &mut raw);
                        raw.map(|v| (v.max(0.0) + 1e-10).log10())
                    },
                )
            })?)
        } else {
            None
        };
        let mins = film.min();
        for c in 0..film.channels() {
            for v in &mut film.curves[c] {
                *v -= mins[c];
            }
        }
        // Print balancing uses stock curves; only the developed print is morphed.
        crate::chemistry::apply(&mut print, &params.print_chemistry)?;
        let couplers = CouplerModel::new(&film, &params.dir_couplers);
        let converter = if params.workflow.convert_film() {
            Some(crate::convert::FilmConverter::new(&tables, &film, &params)?)
        } else {
            None
        };
        let convert_lut = if params.settings.use_convert_lut {
            if let Some(converter) = &converter {
                let key = crate::cache::data_key(data)?
                    + &serde_json::to_string(&(
                        "convert",
                        &params.film_profile,
                        &params.film_base,
                        &params.film_chemistry,
                        &params.special.film_channel_swap,
                        &params.convert.scan_illuminant,
                        &params.io.input_color_space,
                        params.settings.convert_lut_resolution,
                    ))?;
                Some(crate::density_lut::DensityLut::shared(key, || {
                    crate::density_lut::DensityLut::inverse(
                        converter,
                        params.settings.convert_lut_resolution,
                    )
                })?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Self {
            converter,
            convert_lut,
            scan_profile,
            params,
            resolved_params,
            film,
            print,
            response,
            input_matrix,
            output_matrix,
            passthrough_matrix,
            output_encode_matrix,
            couplers,
            print_weights,
            scan_weights,
            print_factor,
            preflash,
            compressor,
            scan_white_xyz,
            scan_reference,
            enlarger_lut,
            scanner_lut,
            setup_seconds: start.elapsed().as_secs_f64(),
        })
    }
    pub fn convert_input(&self, image: &mut Image, matrix: Matrix) {
        image.data.par_chunks_exact_mut(3).for_each(|p| {
            let v = mat_vec(&matrix, [p[0], p[1], p[2]]);
            p.copy_from_slice(&v);
        });
    }
    /// Replace all dependent state after validation; a failed update preserves
    /// the working simulator. Soft edits use the same explicit invalidation.
    pub fn update(&mut self, data: &Path, params: Params) -> Result<()> {
        let replacement = Self::new(data, params)?;
        *self = replacement;
        Ok(())
    }
    /// Resume from a stage tap. Injected arrays are already framed; callers
    /// provide their physical pixel spacing explicitly. No hidden crop/metering.
    pub fn process_from(
        &self,
        image: Image,
        inject: Tap,
        collect: Tap,
        pixel_um: f64,
    ) -> Result<Image> {
        Ok(self
            .process_profiled(image, inject, collect, pixel_um)?
            .image)
    }
    /// Stage executor for differential injection and honest stage timings.
    /// The fused per-pixel process_owned path remains available for throughput.
    pub fn process_profiled(
        &self,
        mut image: Image,
        inject: Tap,
        collect: Tap,
        pixel_um: f64,
    ) -> Result<ProcessReport> {
        image.validate()?;
        ensure!(
            pixel_um.is_finite() && pixel_um > 0.,
            "Pixel spacing must be positive and finite"
        );
        let start = Instant::now();
        let mut stages = std::collections::BTreeMap::new();
        let mut record = |label: &str, from: Instant| {
            stages.insert(label.into(), from.elapsed().as_secs_f64());
        };
        let mut tap = inject;
        let mut auto_scale = 1.;
        let mut spacing = pixel_um;
        let seed = self.render_seed();
        let film_scan = self.params.workflow.scan_film();
        ensure!(
            !(film_scan && [Tap::LogEPrint, Tap::CmyPrint].contains(&collect)),
            "Film scan has no print taps"
        );
        ensure!(
            !(self.params.workflow.convert_film() && collect == Tap::LogEFilm),
            "Conversion has no film exposure tap"
        );
        loop {
            let channels = match tap {
                Tap::LogEFilm | Tap::CmyFilm => self.film.channels(),
                Tap::LogEPrint | Tap::CmyPrint => self.print.channels(),
                _ => 3,
            };
            ensure!(
                image.channels == channels,
                "Wrong channel count for {}",
                tap.name()
            );
            if tap == collect {
                break;
            }
            let from = Instant::now();
            match tap {
                Tap::RgbIn => {
                    spacing = self.params.camera.film_format_mm * 1000.
                        / image.width.max(image.height) as f64
                        / self.params.io.upscale_factor;
                    if self.params.workflow.route != "input" && !self.params.workflow.convert_film()
                    {
                        auto_scale = self.auto_scale(&image);
                    }
                    image = crate::framing::preprocess(image, &self.params.io)?;
                    tap = Tap::RgbPre;
                    record("preprocess", from);
                }
                Tap::RgbPre if self.params.workflow.route == "input" => {
                    ensure!(collect == Tap::RgbOut, "Passthrough only provides RGB taps");
                    image.data.par_chunks_exact_mut(3).for_each(|p| {
                        let rgb = [p[0], p[1], p[2]].map(|v| {
                            if self.params.io.input_cctf_decoding {
                                decode(&self.params.io.input_color_space, v)
                            } else {
                                v
                            }
                        });
                        let rgb = mat_vec(&self.passthrough_matrix, rgb).map(|v| {
                            if self.params.io.output_cctf_encoding {
                                encode(&self.params.io.output_color_space, v)
                            } else {
                                v
                            }
                        });
                        p.copy_from_slice(&rgb);
                    });
                    tap = Tap::RgbOut;
                    record("passthrough", from);
                }
                Tap::RgbPre if self.converter.is_some() => {
                    let converter = self.converter.as_ref().unwrap();
                    if self.params.io.input_cctf_decoding {
                        image
                            .data
                            .iter_mut()
                            .for_each(|v| *v = decode(&self.params.io.input_color_space, *v));
                    }
                    if let Some(lut) = &self.convert_lut {
                        let batch = image.width * image.height > 1;
                        image.data.par_chunks_exact_mut(3).for_each(|p| {
                            p.copy_from_slice(
                                &lut.apply(
                                    &converter.pretransform_pixel([p[0], p[1], p[2]], batch),
                                ),
                            )
                        });
                    } else {
                        image = converter.convert_owned(image);
                    }
                    tap = Tap::CmyFilm;
                    record("converting.convert", from);
                }
                Tap::RgbPre => {
                    if inject == Tap::RgbPre {
                        auto_scale = self.auto_scale(&image);
                    }
                    image =
                        self.process_spatial(image, Tap::LogEFilm, auto_scale, spacing, seed)?;
                    tap = Tap::LogEFilm;
                    record("filming.expose", from);
                }
                Tap::LogEFilm => {
                    ensure!(
                        !self.params.workflow.convert_film()
                            && self.params.workflow.route != "input",
                        "Route has no film exposure stage"
                    );
                    image = self.develop_film(image, spacing, seed)?;
                    tap = Tap::CmyFilm;
                    record("filming.develop", from);
                }
                Tap::CmyFilm if !film_scan => {
                    ensure!(
                        self.params.workflow.route != "input",
                        "Passthrough has no film stage"
                    );
                    image = self.finish_density(image, Tap::LogEPrint, spacing, seed)?;
                    tap = Tap::LogEPrint;
                    record("printing.expose", from);
                }
                Tap::LogEPrint => {
                    ensure!(
                        !film_scan && self.params.workflow.route != "input",
                        "Route has no print stage"
                    );
                    image
                        .data
                        .par_chunks_exact_mut(image.channels)
                        .for_each(|v| {
                            let mut logs = [0.; 3];
                            logs[..v.len()].copy_from_slice(v);
                            self.print.density(&logs[..v.len()], v);
                        });
                    tap = Tap::CmyPrint;
                    record("printing.develop", from);
                }
                Tap::CmyFilm | Tap::CmyPrint => {
                    ensure!(
                        self.params.workflow.route != "input"
                            && (film_scan == (tap == Tap::CmyFilm)),
                        "Unavailable scan tap"
                    );
                    image = self.scan_density(image, seed)?;
                    tap = Tap::RgbOut;
                    record("scanning.scan", from);
                }
                Tap::RgbOut => bail!("Cannot reach {} from {}", collect.name(), inject.name()),
            }
        }
        image.validate()?;
        Ok(ProcessReport {
            image,
            stages,
            total_seconds: start.elapsed().as_secs_f64(),
        })
    }
    /// One seed per render for fast grain and glare. None chooses fresh entropy;
    /// legacy grain retains its reference layer/channel seeds independently.
    fn render_seed(&self) -> u32 {
        self.params.settings.random_seed.unwrap_or_else(|| {
            use std::hash::BuildHasher;
            std::collections::hash_map::RandomState::new().hash_one(0_u8) as u32
        })
    }
    pub fn process(&self, image: &Image, collect: Tap) -> Result<Image> {
        self.process_owned(image.clone(), collect)
    }
    pub fn process_owned(&self, image: Image, collect: Tap) -> Result<Image> {
        let output = self.process_owned_inner(image, collect, self.render_seed())?;
        output.validate()?;
        Ok(output)
    }
    fn process_owned_inner(&self, mut image: Image, collect: Tap, seed: u32) -> Result<Image> {
        image.validate()?;
        ensure!(image.channels == 3, "Input must be RGB");
        if collect == Tap::RgbIn {
            return Ok(image);
        }
        let pixel_um = self.params.camera.film_format_mm * 1000.0
            / image.width.max(image.height) as f64
            / self.params.io.upscale_factor;
        // Meter the captured full frame, before framing changes its field of view.
        let auto_scale = if collect != Tap::RgbPre
            && self.params.workflow.route != "input"
            && !self.params.workflow.convert_film()
        {
            self.auto_scale(&image)
        } else {
            1.0
        };
        image = crate::framing::preprocess(image, &self.params.io)?;
        if collect == Tap::RgbPre {
            return Ok(image);
        }
        if self.params.workflow.route == "input" {
            ensure!(collect == Tap::RgbOut, "Passthrough only provides rgb_out");
            image.data.par_chunks_exact_mut(3).for_each(|p| {
                let mut rgb = [p[0], p[1], p[2]];
                if self.params.io.input_cctf_decoding {
                    rgb = rgb.map(|v| decode(&self.params.io.input_color_space, v));
                }
                let mut rgb = mat_vec(&self.passthrough_matrix, rgb);
                if self.params.io.output_cctf_encoding {
                    rgb = rgb.map(|v| encode(&self.params.io.output_color_space, v));
                }
                p.copy_from_slice(&rgb);
            });
            return Ok(image);
        }
        let film_scan = self.params.workflow.scan_film();
        if film_scan && [Tap::LogEPrint, Tap::CmyPrint].contains(&collect) {
            bail!("Film-scan workflow has no print taps");
        }
        if let Some(converter) = &self.converter {
            ensure!(
                collect != Tap::LogEFilm,
                "Negative conversion has no film exposure tap"
            );
            if self.params.io.input_cctf_decoding {
                image
                    .data
                    .iter_mut()
                    .for_each(|v| *v = decode(&self.params.io.input_color_space, *v));
            }
            let density = if let Some(lut) = &self.convert_lut {
                let batch = image.width * image.height > 1;
                image.data.par_chunks_exact_mut(3).for_each(|p| {
                    let y = converter.pretransform_pixel([p[0], p[1], p[2]], batch);
                    p.copy_from_slice(&lut.apply(&y));
                });
                image
            } else {
                converter.convert_owned(image)
            };
            return self.finish_density(density, collect, pixel_um, seed);
        }
        if !self.params.debug.deactivate_spatial_effects
            || self.params.grain.active
            || self.params.print_glare.active
            || self.params.halation.boost_ev > 0.0
        {
            return self.process_spatial(image, collect, auto_scale, pixel_um, seed);
        }
        let channels = match collect {
            Tap::LogEFilm | Tap::CmyFilm => self.film.channels(),
            Tap::LogEPrint | Tap::CmyPrint => self.print.channels(),
            _ => 3,
        };
        if channels == 3 {
            image.data.par_chunks_exact_mut(3).for_each(|pixel| {
                let out = self.pixel([pixel[0], pixel[1], pixel[2]], collect, auto_scale);
                pixel.copy_from_slice(&out);
            });
        } else {
            let mut output = vec![0.0; image.width * image.height * channels];
            output
                .par_chunks_exact_mut(channels)
                .zip(image.data.par_chunks_exact(3))
                .for_each(|(out, p)| {
                    let v = self.pixel([p[0], p[1], p[2]], collect, auto_scale);
                    out.copy_from_slice(&v[..channels]);
                });
            image.data = output;
            image.channels = channels;
        }
        Ok(image)
    }
    fn auto_scale(&self, image: &Image) -> f64 {
        if !self.params.camera.auto_exposure {
            return 1.0;
        }
        let preview = crate::metering::preview(image);
        let ch = self.film.channels();
        let mut raw = Image {
            width: preview.width,
            height: preview.height,
            channels: ch,
            data: vec![0.0; preview.width * preview.height * ch],
        };
        raw.data
            .par_chunks_exact_mut(ch)
            .zip(preview.data.par_chunks_exact(3))
            .for_each(|(out, pixel)| {
                let mut rgb = [pixel[0], pixel[1], pixel[2]];
                if self.params.io.input_cctf_decoding {
                    rgb = rgb.map(|v| decode(&self.params.io.input_color_space, v));
                }
                out.copy_from_slice(&raw_pixel(&self.response, &self.input_matrix, rgb)[..ch]);
            });
        crate::metering::scale(&raw, &self.params.camera.auto_exposure_method)
    }
    fn process_spatial(
        &self,
        image: Image,
        collect: Tap,
        auto_scale: f64,
        pixel_um: f64,
        random_seed: u32,
    ) -> Result<Image> {
        use crate::spatial::{exponential, gaussian};
        let w = image.width;
        let h = image.height;
        let ch = self.film.channels();
        let mut raw = Image {
            width: w,
            height: h,
            channels: ch,
            data: vec![0.0; w * h * ch],
        };
        raw.data
            .par_chunks_exact_mut(ch)
            .zip(image.data.par_chunks_exact(3))
            .for_each(|(out, pixel)| {
                let mut rgb = [pixel[0], pixel[1], pixel[2]];
                if self.params.io.input_cctf_decoding {
                    rgb = rgb.map(|v| decode(&self.params.io.input_color_space, v));
                }
                let value = raw_pixel(&self.response, &self.input_matrix, rgb);
                let gain = 2.0_f64.powf(self.params.camera.exposure_compensation_ev);
                for c in 0..ch {
                    out[c] = value[c] * auto_scale * gain;
                }
            });
        drop(image);
        let hal = &self.params.halation;
        if hal.boost_ev > 0.0 {
            ensure!(
                (0.0..=1.0).contains(&hal.boost_range) && hal.protect_ev >= 0.0,
                "Invalid highlight boost range or protection"
            );
            let max = raw.data.iter().copied().fold(f64::NEG_INFINITY, |a, b| {
                if a.is_nan() || b.is_nan() {
                    f64::NAN
                } else {
                    a.max(b)
                }
            });
            if max.is_nan() {
                raw.data.fill(f64::NAN);
            } else if max > 0.0 {
                let threshold = (0.184 * 2.0f64.powf(hal.protect_ev)).clamp(0.0, max);
                if threshold != max {
                    let a = 28.0f64.powf(1.0 - hal.boost_range);
                    let dx = 1.0 - threshold / max;
                    let denom = (a * dx).exp() - a * dx - 1.0;
                    let scale = (2.0f64.powf(hal.boost_ev) - 1.0) / denom * max;
                    raw.data.par_iter_mut().for_each(|v| {
                        if *v > threshold {
                            let dx = (*v - threshold) / max;
                            *v += scale * ((a * dx).exp() - a * dx - 1.0);
                        }
                    });
                }
            }
        }
        // The pinned camera model diffuses before lens blur. Boundary rules and
        // float32 FFT stores make these operations noncommutative.
        raw = crate::diffusion::apply_owned(raw, &self.params.camera.diffusion_filter, pixel_um)?;
        if self.params.camera.lens_blur_um > 0.0 {
            raw = gaussian(&raw, &[self.params.camera.lens_blur_um / pixel_um]);
        }
        if hal.active && !self.params.debug.deactivate_spatial_effects {
            let core_sigma = hal.scatter_core_um[..ch]
                .iter()
                .map(|v| v * hal.scatter_spatial_scale / pixel_um)
                .collect::<Vec<_>>();
            let tail_lambda = hal.scatter_tail_um[..ch]
                .iter()
                .map(|v| v * hal.scatter_spatial_scale / pixel_um)
                .collect::<Vec<_>>();
            if hal.scatter_amount > 0.0
                && (core_sigma.iter().any(|v| *v > 0.0) || tail_lambda.iter().any(|v| *v > 0.0))
            {
                let mut core = gaussian(&raw, &core_sigma);
                let mut tail = exponential(&raw, &tail_lambda);
                for i in 0..raw.data.len() {
                    let c = i % ch;
                    core.data[i] *= 1.0 - hal.scatter_tail_weight[c];
                    tail.data[i] *= hal.scatter_tail_weight[c];
                    core.data[i] += tail.data[i];
                    raw.data[i] *= 1.0 - hal.scatter_amount;
                    core.data[i] *= hal.scatter_amount;
                    raw.data[i] += core.data[i];
                }
            }
            let strengths = hal.halation_strength[..ch]
                .iter()
                .map(|v| v * hal.halation_amount)
                .collect::<Vec<_>>();
            let sigma = hal.halation_first_sigma_um[..ch]
                .iter()
                .map(|v| v * hal.halation_spatial_scale / pixel_um)
                .collect::<Vec<_>>();
            if hal.halation_n_bounces > 0
                && strengths.iter().any(|v| *v > 0.0)
                && sigma.iter().any(|v| *v > 0.0)
            {
                let mut accumulator = vec![0.0; raw.data.len()];
                let sum = (0..hal.halation_n_bounces)
                    .map(|k| hal.halation_bounce_decay.powi(k as i32))
                    .sum::<f64>();
                for k in 0..hal.halation_n_bounces {
                    let weight = hal.halation_bounce_decay.powi(k as i32) / sum;
                    let blurred = gaussian(
                        &raw,
                        &sigma
                            .iter()
                            .map(|v| v * ((k + 1) as f64).sqrt())
                            .collect::<Vec<_>>(),
                    );
                    for (i, v) in blurred.data.iter().enumerate() {
                        accumulator[i] += weight * v;
                    }
                }
                for (i, v) in raw.data.iter_mut().enumerate() {
                    let c = i % ch;
                    *v += strengths[c] * accumulator[i];
                    if hal.halation_renormalize {
                        *v /= 1.0 + strengths[c];
                    }
                }
            }
        }
        for v in &mut raw.data {
            *v = ((*v * self.scan_reference.film_gain).max(0.0) + 1e-10).log10();
        }
        if collect == Tap::LogEFilm {
            return Ok(raw);
        }
        let density = self.develop_film(raw, pixel_um, random_seed)?;
        self.finish_density(density, collect, pixel_um, random_seed)
    }
    fn develop_film(&self, raw: Image, pixel_um: f64, random_seed: u32) -> Result<Image> {
        use crate::spatial::{exponential, gaussian};
        let (w, h, ch) = (raw.width, raw.height, raw.channels);
        let mut density = raw;
        if self.couplers.active {
            let mut correction = Image {
                width: w,
                height: h,
                channels: ch,
                data: vec![0.0; w * h * ch],
            };
            correction
                .data
                .par_chunks_exact_mut(ch)
                .zip(density.data.par_chunks_exact(ch))
                .for_each(|(out, logs)| {
                    let mut d = [0.; 3];
                    self.film.density(logs, &mut d[..ch]);
                    out.copy_from_slice(&self.couplers.correction_raw(&d[..ch], ch)[..ch]);
                });
            let cp = &self.params.dir_couplers;
            if cp.diffusion_size_um > 0.0 {
                let mut core = gaussian(&correction, &[cp.diffusion_size_um / pixel_um]);
                let tail = exponential(&correction, &[cp.diffusion_tail_um / pixel_um]);
                for (i, v) in core.data.iter_mut().enumerate() {
                    *v = (1.0 - cp.diffusion_tail_weight) * (*v)
                        + cp.diffusion_tail_weight * tail.data[i];
                }
                correction = core;
            }
            density
                .data
                .par_chunks_exact_mut(ch)
                .zip(correction.data.par_chunks_exact_mut(ch))
                .for_each(|(out, corr)| {
                    let mut logs = [0.; 3];
                    logs[..ch].copy_from_slice(out);
                    self.couplers.receive(corr);
                    for c in 0..ch {
                        out[c] =
                            linear(logs[c] - corr[c], &self.film.axis, &self.couplers.curves[c]);
                    }
                });
        } else {
            density.data.par_chunks_exact_mut(ch).for_each(|out| {
                let mut logs = [0.; 3];
                logs[..ch].copy_from_slice(out);
                self.film.density(&logs[..ch], out);
            });
        }
        if self.params.grain.active {
            density = crate::grain::apply(
                &density,
                &self.film,
                &self.params.grain,
                pixel_um,
                self.params.settings.use_fast_stats,
                random_seed,
            )?;
        }
        Ok(density)
    }
    fn finish_density(
        &self,
        density: Image,
        collect: Tap,
        pixel_um: f64,
        random_seed: u32,
    ) -> Result<Image> {
        use crate::spatial::gaussian;
        if collect == Tap::CmyFilm {
            return Ok(density);
        }
        let (w, h, ch) = (density.width, density.height, density.channels);
        let print_density =
            if !self.params.workflow.scan_film() && self.params.enlarger.diffusion_filter.active {
                let mut raw = Image::new(
                    w,
                    h,
                    self.print.channels(),
                    vec![0.; w * h * self.print.channels()],
                )?;
                raw.data
                    .par_chunks_exact_mut(raw.channels)
                    .zip(density.data.par_chunks_exact(ch))
                    .for_each(|(out, d)| {
                        let logs = self.after_film(d, Tap::LogEPrint, false, 0.);
                        for c in 0..out.len() {
                            out[c] = (10.0f64.powf(logs[c]) - 1e-10).max(0.);
                        }
                    });
                raw = crate::diffusion::apply_owned(
                    raw,
                    &self.params.enlarger.diffusion_filter,
                    pixel_um,
                )?;
                raw.data
                    .iter_mut()
                    .for_each(|v| *v = (v.max(0.) + 1e-10).log10());
                if collect == Tap::LogEPrint {
                    return Ok(raw);
                }
                raw.data.par_chunks_exact_mut(raw.channels).for_each(|v| {
                    let mut logs = [0.; 3];
                    logs[..v.len()].copy_from_slice(v);
                    self.print.density(&logs[..v.len()], v);
                });
                if collect == Tap::CmyPrint {
                    return Ok(raw);
                }
                Some(raw)
            } else {
                None
            };
        let out_ch = match collect {
            Tap::LogEPrint | Tap::CmyPrint => self.print.channels(),
            _ => 3,
        };
        let mut output = Image {
            width: w,
            height: h,
            channels: out_ch,
            data: vec![0.0; w * h * out_ch],
        };
        let mut glare = None;
        let g = &self.params.print_glare;
        if collect == Tap::RgbOut
            && !self.params.workflow.scan_film()
            && g.active
            && g.percent > 0.0
        {
            let mut rng = crate::random::RandomState::new(random_seed.wrapping_add(0x6c617265));
            let sigma2 = (1.0 + g.roughness * g.roughness).ln();
            let mu = g.percent.ln() - sigma2 / 2.0;
            let field = Image {
                width: w,
                height: h,
                channels: 1,
                data: (0..w * h)
                    .map(|_| (mu + sigma2.sqrt() * rng.normal()).exp() / 100.0)
                    .collect(),
            };
            glare = Some(gaussian(&field, &[g.blur]));
        }
        output
            .data
            .par_chunks_exact_mut(out_ch)
            .zip(density.data.par_chunks_exact(ch))
            .enumerate()
            .for_each(|(i, (out, d))| {
                let amount = glare.as_ref().map_or(0.0, |field| field.data[i]);
                let value = if let Some(print) = &print_density {
                    let pd = &print.data[i * print.channels..(i + 1) * print.channels];
                    let mut xyz = [0.; 3];
                    if let Some(lut) = &self.scanner_lut {
                        xyz = lut.apply(pd).map(|v| 10.0f64.powf(v));
                    } else {
                        integrate(&self.print, pd, &self.scan_weights, 3, &mut xyz);
                    }
                    self.finish_scan(xyz, false, amount)
                } else {
                    self.after_film(d, collect, false, amount)
                };
                out.copy_from_slice(&value[..out_ch])
            });
        drop(density);
        if collect == Tap::RgbOut {
            self.finish_output(output)
        } else {
            Ok(output)
        }
    }
    fn finish_output(&self, mut output: Image) -> Result<Image> {
        use crate::spatial::gaussian;
        if self.params.scanner.lens_blur > 0.0 {
            output = gaussian(&output, &[self.params.scanner.lens_blur]);
        }
        let [sigma, amount] = self.params.scanner.unsharp_mask;
        if sigma > 0.0 && amount > 0.0 {
            let blurred = gaussian(&output, &[sigma]);
            for (i, v) in output.data.iter_mut().enumerate() {
                *v += amount * (*v - blurred.data[i]);
            }
        }
        if self.params.io.output_cctf_encoding {
            output.data.par_chunks_exact_mut(3).for_each(|out| {
                let rgb = mat_vec(&self.output_encode_matrix, [out[0], out[1], out[2]])
                    .map(|v| encode(&self.params.io.output_color_space, v));
                out.copy_from_slice(&rgb);
            });
        }
        Ok(output)
    }
    fn scan_density(&self, density: Image, random_seed: u32) -> Result<Image> {
        use crate::spatial::gaussian;
        let film_scan = self.params.workflow.scan_film();
        let profile = if film_scan {
            &self.scan_profile
        } else {
            &self.print
        };
        let mut output = Image::new(
            density.width,
            density.height,
            3,
            vec![0.; density.width * density.height * 3],
        )?;
        let g = &self.params.print_glare;
        let glare = if !film_scan && g.active && g.percent > 0. {
            let mut rng = crate::random::RandomState::new(random_seed.wrapping_add(0x6c617265));
            let sigma2 = (1. + g.roughness * g.roughness).ln();
            let mu = g.percent.ln() - sigma2 / 2.;
            Some(gaussian(
                &Image::new(
                    density.width,
                    density.height,
                    1,
                    (0..density.width * density.height)
                        .map(|_| (mu + sigma2.sqrt() * rng.normal()).exp() / 100.)
                        .collect(),
                )?,
                &[g.blur],
            ))
        } else {
            None
        };
        output
            .data
            .par_chunks_exact_mut(3)
            .zip(density.data.par_chunks_exact(density.channels))
            .enumerate()
            .for_each(|(i, (out, d))| {
                let mut xyz = [0.; 3];
                if let Some(lut) = &self.scanner_lut {
                    xyz = lut.apply(d).map(|v| 10.0f64.powf(v));
                } else {
                    integrate(profile, d, &self.scan_weights, 3, &mut xyz);
                }
                out.copy_from_slice(&self.finish_scan(
                    xyz,
                    false,
                    glare.as_ref().map_or(0., |g| g.data[i]),
                ));
            });
        self.finish_output(output)
    }
    fn pixel(&self, mut rgb: [f64; 3], collect: Tap, auto_scale: f64) -> [f64; 3] {
        if self.params.io.input_cctf_decoding {
            rgb = rgb.map(|v| decode(&self.params.io.input_color_space, v));
        }
        let raw = raw_pixel(&self.response, &self.input_matrix, rgb);
        let log_e = raw.map(|v| {
            ((v * auto_scale
                * 2.0_f64.powf(self.params.camera.exposure_compensation_ev)
                * self.scan_reference.film_gain)
                .max(0.0)
                + 1e-10)
                .log10()
        });
        if collect == Tap::LogEFilm {
            return log_e;
        }
        let mut dfilm = [0.0; 3];
        self.couplers.develop(&self.film, &log_e, &mut dfilm);
        if collect == Tap::CmyFilm {
            return dfilm;
        }
        self.after_film(&dfilm, collect, true, 0.0)
    }
    fn after_film(&self, dfilm: &[f64], collect: Tap, encode_output: bool, glare: f64) -> [f64; 3] {
        let mut scanned = [0.0; 3];
        if self.params.workflow.scan_film() {
            if let Some(lut) = &self.scanner_lut {
                scanned = lut.apply(dfilm).map(|v| 10.0f64.powf(v));
            } else {
                integrate(
                    &self.scan_profile,
                    &dfilm[..self.film.channels()],
                    &self.scan_weights,
                    3,
                    &mut scanned,
                );
            }
        } else {
            let mut raw_print = [0.0; 3];
            if let Some(lut) = &self.enlarger_lut {
                raw_print = lut.apply(dfilm);
            } else {
                integrate(
                    &self.film,
                    &dfilm[..self.film.channels()],
                    &self.print_weights,
                    self.print.channels(),
                    &mut raw_print,
                );
                for c in 0..self.print.channels() {
                    raw_print[c] = ((raw_print[c] * self.print_factor + self.preflash[c]).max(0.0)
                        + 1e-10)
                        .log10();
                }
            }
            let mut log_print = [0.0; 3];
            for c in 0..self.print.channels() {
                // Keep the reference's log -> power -> exposure -> log roundtrip.
                log_print[c] = (10.0_f64.powf(raw_print[c])
                    * self.params.enlarger.print_exposure
                    * self.scan_reference.print_gain
                    + 1e-10)
                    .log10();
            }
            if collect == Tap::LogEPrint {
                return log_print;
            }
            let mut dprint = [0.0; 3];
            self.print.density(&log_print, &mut dprint);
            if collect == Tap::CmyPrint {
                return dprint;
            }
            if let Some(lut) = &self.scanner_lut {
                scanned = lut
                    .apply(&dprint[..self.print.channels()])
                    .map(|v| 10.0f64.powf(v));
            } else {
                integrate(
                    &self.print,
                    &dprint[..self.print.channels()],
                    &self.scan_weights,
                    3,
                    &mut scanned,
                );
            }
        }
        self.finish_scan(scanned, encode_output, glare)
    }
    fn finish_scan(&self, scanned: [f64; 3], encode_output: bool, glare: f64) -> [f64; 3] {
        // Scanner's log XYZ -> 10**log XYZ contributes the +1e-10 floor.
        let mut xyz = if self.scanner_lut.is_some() {
            scanned
        } else {
            scanned.map(|v| 10.0_f64.powf((v.max(0.0) + 1e-10).log10()))
        };
        self.scan_reference.correct(&mut xyz);
        for c in 0..3 {
            xyz[c] += glare * self.scan_white_xyz[c];
        }
        let rgb = self.compressor.compress(mat_vec(&self.output_matrix, xyz));
        if encode_output && self.params.io.output_cctf_encoding {
            mat_vec(&self.output_encode_matrix, rgb)
                .map(|v| encode(&self.params.io.output_color_space, v))
        } else {
            rgb
        }
    }
}
fn raw_pixel(response: &FilmResponse, matrix: &Matrix, rgb: [f64; 3]) -> [f64; 3] {
    let xyz = mat_vec(matrix, rgb);
    let brightness = xyz.iter().sum::<f64>();
    let divisor = brightness.max(1e-10);
    let mut out = [0.0; 3];
    response.lut.cubic_into(
        tri_to_quad([xyz[0] / divisor, xyz[1] / divisor]),
        &mut out[..response.lut.channels],
    );
    for v in &mut out {
        *v *= brightness;
    }
    out
}
fn filtered_illuminant(tables: &Tables, lamp: &[f64], filters: [f64; 3]) -> Vec<f64> {
    let transmission = filters.map(|v| 10.0_f64.powf(-v / 100.0));
    (0..81)
        .map(|w| {
            lamp[w]
                * (0..3)
                    .map(|c| 1.0 - (1.0 - tables.dichroics[w][c]) * (1.0 - transmission[c]))
                    .product::<f64>()
        })
        .collect()
}
pub(crate) fn apply_coupler_presets(
    data: &Path,
    film: &Profile,
    couplers: &mut Couplers,
) -> Result<()> {
    let presets: toml::Value = toml::from_str(&std::fs::read_to_string(
        data.join("presets/couplers.toml"),
    )?)?;
    let mut current = serde_json::to_value(&*couplers)?;
    let mut merge = |table: Option<&toml::Value>| -> Result<()> {
        if let Some(table) = table.and_then(toml::Value::as_table) {
            for (key, value) in table {
                if current.get(key).is_some() {
                    current[key] = serde_json::to_value(value)?;
                }
            }
        }
        Ok(())
    };
    let defaults = presets
        .get("defaults")
        .and_then(|v| v.get(&film.info.channel_model))
        .and_then(|v| v.get(&film.info.polarity));
    merge(defaults)?;
    if film.info.channel_model == "color"
        && film.info.polarity == "negative"
        && film.info.usage == "cine"
    {
        let branch = if film.info.reference_illuminant.starts_with('T') {
            "tungsten"
        } else {
            "daylight"
        };
        merge(
            defaults
                .and_then(|v| v.get("cine"))
                .and_then(|v| v.get(branch)),
        )?;
    }
    merge(presets.get(&film.info.stock))?;
    *couplers = serde_json::from_value(current)?;
    Ok(())
}

/// Stock preset digestion does not construct spectral tables or render buffers.
pub fn resolve_params(data: &Path, mut params: Params) -> Result<Params> {
    params.validate_numeric()?;
    let film = Profile::load(data, &params.film_profile)?;
    resolve_presets(data, &film, &mut params)?;
    Ok(params)
}
fn resolve_presets(data: &Path, film: &Profile, params: &mut Params) -> Result<()> {
    if params.settings.neutral_print_filters_from_database {
        let db: serde_json::Value = serde_json::from_reader(std::fs::File::open(
            data.join("filters/neutral_print_filters.json"),
        )?)?;
        let pack = db
            .get(&params.print_profile)
            .and_then(|v| v.get(&params.enlarger.illuminant))
            .and_then(|v| v.get(&params.film_profile));
        let pack = pack
            .and_then(|v| v.as_array())
            .map(|v| v.iter().map(|n| n.as_f64().unwrap()).collect::<Vec<_>>())
            .unwrap_or_else(|| vec![0.0, 50.0, 50.0]);
        params.enlarger.c_filter_neutral = pack[0];
        params.enlarger.m_filter_neutral = pack[1];
        params.enlarger.y_filter_neutral = pack[2];
    }
    if params.settings.apply_coupler_presets {
        apply_coupler_presets(data, film, &mut params.dir_couplers)?;
    }
    if params.settings.apply_grain_presets {
        crate::grain::apply_presets(data, film, &mut params.grain)?;
    }
    Ok(())
}

struct PreparedParams {
    params: Params,
    resolved_params: Params,
    tables: std::sync::Arc<Tables>,
    film: Profile,
    print: Profile,
    stock_film_base: Vec<f64>,
    stock_print_base: Vec<f64>,
}
fn prepare_params(data: &Path, mut params: Params) -> Result<PreparedParams> {
    params.validate_numeric()?;
    crate::framing::validate(&params.io)?;
    crate::diffusion::validate(&params.camera.diffusion_filter)?;
    crate::diffusion::validate(&params.enlarger.diffusion_filter)?;
    ensure!(
        [
            "average",
            "center_weighted",
            "median",
            "partial",
            "matrix",
            "multi_zone",
            "highlight_weighted"
        ]
        .contains(&params.camera.auto_exposure_method.as_str()),
        "Unknown autoexposure method"
    );
    let input_gamut = &params.io.input_gamut_compress;
    ensure!(
        input_gamut.algorithm == "xy",
        "Input gamut compression supports xy"
    );
    ensure!(
        ["locus", "inscribed_hull"].contains(&input_gamut.boundary.as_str()),
        "Unknown input gamut boundary"
    );
    ensure!(
        input_gamut.hull_detail.is_finite() && input_gamut.hull_detail > 0.0,
        "Hull detail must be positive and finite"
    );
    for knee in [
        Some(input_gamut.knee),
        Some(params.io.output_gamut_compress.knee),
        params.io.output_gamut_compress.lightness_compression,
    ]
    .into_iter()
    .flatten()
    {
        ensure!(
            knee.iter().all(|v| v.is_finite())
                && (0.0..1.0).contains(&knee[0])
                && knee[1] > 0.0
                && knee[2] > 0.0,
            "Invalid gamut compression knee"
        );
    }
    ensure!(
        params.camera.film_format_mm.is_finite() && params.camera.film_format_mm > 0.0,
        "Film format must be positive and finite"
    );
    ensure!(
        params.halation.boost_ev.is_finite() && params.halation.boost_ev >= 0.0,
        "Highlight boost must be finite and nonnegative"
    );
    let tables = Tables::shared(data)?;
    ensure!(
        tables.baseline_commit == "28bf883e1672e884307edc75852549376e13644e",
        "Colour tables do not match the pinned experimental baseline"
    );
    let mut film = Profile::load_development(
        data,
        &params.film_profile,
        params.film_chemistry.development_time,
    )?;
    let mut print = Profile::load_development(
        data,
        &params.print_profile,
        params.print_chemistry.development_time,
    )?;
    film.swap_dyes(params.special.film_channel_swap)?;
    print.swap_dyes(params.special.print_channel_swap)?;
    let stock_film_base = film.base.clone();
    let stock_print_base = print.base.clone();
    ensure!(
        params.print_base.tilt == 0.0,
        "Print base does not support spectral tilt"
    );
    crate::base::apply(&mut film, &params.film_base, true);
    crate::base::apply(&mut print, &params.print_base, false);
    crate::chemistry::apply(&mut film, &params.film_chemistry)?;
    ensure!(
        [
            "input",
            "input > film > scan",
            "input > film > print > scan",
            "input > convert-film > print > scan",
            "input > convert-film > scan",
            "input > convert-film > scan-minus-base"
        ]
        .contains(&params.workflow.route.as_str()),
        "Unsupported workflow route"
    );
    ensure!(params.io.input_rotation < 4, "Rotation must be 0..3");
    ensure!(
        params
            .io
            .output_bit_depth
            .is_none_or(|v| [8, 16, 32].contains(&v)),
        "Output bit depth must be 8,16 or 32"
    );
    ensure!(
        ["zip", "none"].contains(&params.io.output_compression.as_str()),
        "Unsupported output compression"
    );
    if let Some(space) = &params.workflow.saving_color_space {
        tables.rgb_to_rgb(&params.io.output_color_space, space)?;
    }
    ensure!(
        params.settings.preview_max_size > 0,
        "Preview size must be positive"
    );
    let mut resolved_params = params.clone();
    resolve_presets(data, &film, &mut resolved_params)?;
    if params.settings.preview_mode {
        params.enlarger.lens_blur = 0.;
        params.dir_couplers.diffusion_size_um = 0.;
        params.grain.active = false;
        params.grain.rms_granularity = vec![0.; 3];
        params.grain.blur = 0.;
        params.grain.mult_usm_amount = 0.;
        params.print_glare.blur = 0.;
        params.camera.lens_blur_um = 0.;
        params.scanner.lens_blur = 0.;
        params.scanner.unsharp_mask = [0.; 2];
    }
    if params.debug.lut_mode {
        params.debug.deactivate_spatial_effects = true;
        params.debug.deactivate_stochastic_effects = true;
        params.camera.auto_exposure = false;
        params.camera.exposure_compensation_ev = 0.0;
        params.enlarger.print_exposure_compensation = false;
        params.enlarger.print_exposure = 1.0;
        params.enlarger.y_filter_shift = 0.0;
        params.enlarger.m_filter_shift = 0.0;
        params.enlarger.preflash_exposure = 0.0;
        params.halation.boost_ev = 0.0;
        params.scanner.black_correction = false;
        params.scanner.white_correction = false;
        // Framing would crop/resample the cube grid rather than an image.
        params.io.crop = false;
        params.io.upscale_factor = 1.0;
        // Bake the exact spectral transform, without embedding a second
        // layer of interpolation from the interactive acceleration LUTs.
        params.settings.use_convert_lut = false;
        params.settings.use_enlarger_lut = false;
        params.settings.use_scanner_lut = false;
    }
    if params.debug.deactivate_spatial_effects {
        params.camera.diffusion_filter.active = false;
        params.enlarger.diffusion_filter.active = false;
        params.scanner.lens_blur = 0.0;
        params.scanner.unsharp_mask = [0.0, 0.0];
        params.camera.lens_blur_um = 0.0;
        params.enlarger.lens_blur = 0.0;
        params.dir_couplers.diffusion_size_um = 0.0;
        params.grain.blur = 0.0;
        params.grain.blur_dye_clouds_um = 0.0;
        params.grain.mult_usm_amount = 0.0;
        params.print_glare.blur = 0.0;
    }
    resolve_presets(data, &film, &mut params)?;
    ensure!(
        params.grain.density_min.len() >= film.channels()
            && params.grain.density_min.iter().all(|v| v.is_finite()),
        "Missing or invalid grain density-floor channels"
    );
    if params.debug.deactivate_stochastic_effects {
        params.grain.active = false;
        params.print_glare.active = false;
    }
    if film.info.support == "film" {
        params.halation.halation_first_sigma_um = [if film.info.usage == "cine" {
            50.0
        } else {
            65.0
        }; 3];
        params.halation.halation_strength = match film.info.antihalation.as_str() {
            "strong" => [0.015, 0.005, 0.0],
            "weak" => [0.08, 0.02, 0.0],
            "no" => [0.30, 0.10, 0.015],
            _ => params.halation.halation_strength,
        };
    }
    Ok(PreparedParams {
        params,
        resolved_params,
        tables,
        film,
        print,
        stock_film_base,
        stock_print_base,
    })
}
/// Validate settings and profiles without building the spectral response or LUTs.
pub fn validate_params(data: &Path, params: &Params) -> Result<()> {
    let prepared = prepare_params(data, params.clone())?;
    for name in [&params.io.input_color_space, &params.io.output_color_space] {
        prepared.tables.space(name)?;
    }
    for name in [&params.convert.scan_illuminant, &params.enlarger.illuminant] {
        prepared.tables.illuminant(name)?;
    }
    ensure!(
        [
            "hanatos2025",
            "arctic2026beta04",
            "jakob2019",
            "mallett2019",
            "otsu2018",
            "gauss-lasers"
        ]
        .contains(&params.settings.rgb_to_raw_method.as_str()),
        "Unknown spectral method"
    );
    ensure!(
        prepared
            .tables
            .camera_filters
            .contains_key(&params.camera.color_filter),
        "Unknown camera filter"
    );
    ensure!(
        ["off", "oklch", "oklrab", "aces_rgc", "jzazbz", "cam16ucs"]
            .contains(&params.io.output_gamut_compress.algorithm.as_str()),
        "Unknown output gamut algorithm"
    );
    ensure!(
        (1..=256).contains(&params.settings.lut_resolution)
            && (1..=256).contains(&params.settings.convert_lut_resolution),
        "LUT resolution must be 1..256"
    );
    ensure!(
        !params.workflow.convert_film() || prepared.film.channels() == 3,
        "Conversion requires three-channel film"
    );
    Ok(())
}
