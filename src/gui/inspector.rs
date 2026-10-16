//! Typed parameter editor. Serialization is an internal bridge, never UI text.
use super::{Field, State, backend, model, presentation};
use crate::Params;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

fn name(key: &str) -> String {
    match key {
        "active" => "Enabled".into(),
        "input_cctf_decoding" => "Decode input transfer function".into(),
        "output_cctf_encoding" => "Encode output transfer function".into(),
        "y_filter_shift" => "Yellow filter shift".into(),
        "m_filter_shift" => "Magenta filter shift".into(),
        "y_filter_neutral" => "Neutral yellow".into(),
        "m_filter_neutral" => "Neutral magenta".into(),
        "c_filter_neutral" => "Neutral cyan".into(),
        "exposure_compensation_ev" => "Exposure compensation (EV)".into(),
        "input_color_space" | "output_color_space" => "Colour space".into(),
        "auto_exposure_method" => "Metering".into(),
        "monitor_profile" => "Monitor ICC profile".into(),
        "rgb_to_raw_method" => "Spectral reconstruction".into(),
        _ => {
            let s = key
                .replace('_', " ")
                .replace(" um", " (µm)")
                .replace(" mm", " (mm)")
                .replace(" ev", " (EV)");
            let mut c = s.chars();
            c.next()
                .map(|v| v.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}
fn choices(key: &str, s: &State) -> Vec<(String, String)> {
    let values: &[&str] = match key {
        "/special/film_channel_swap" | "/special/print_channel_swap" => {
            return [
                ("[0,1,2]", "Cyan · Magenta · Yellow"),
                ("[0,2,1]", "Cyan · Yellow · Magenta"),
                ("[1,0,2]", "Magenta · Cyan · Yellow"),
                ("[1,2,0]", "Magenta · Yellow · Cyan"),
                ("[2,0,1]", "Yellow · Cyan · Magenta"),
                ("[2,1,0]", "Yellow · Magenta · Cyan"),
            ]
            .into_iter()
            .map(|(v, l)| (v.into(), l.into()))
            .collect();
        }
        "/workflow/route" => {
            return backend::ROUTES
                .iter()
                .zip(backend::ROUTE_LABELS)
                .map(|(v, l)| (v.to_string(), l.into()))
                .collect();
        }
        "/film_profile" => {
            return s
                .films
                .iter()
                .map(|v| (v.id.clone(), v.label.clone()))
                .collect();
        }
        "/print_profile" => {
            return s
                .papers
                .iter()
                .map(|v| (v.id.clone(), v.label.clone()))
                .collect();
        }
        "/workflow/saving_color_space" => {
            return [
                ("", "Processing colour"),
                ("sRGB", "sRGB"),
                ("Display P3", "Display P3"),
                ("Adobe RGB (1998)", "Adobe RGB (1998)"),
                ("ProPhoto RGB", "ProPhoto RGB"),
                ("ITU-R BT.2020", "ITU-R BT.2020"),
                ("ACES2065-1", "ACES2065-1"),
                ("ACEScg", "ACEScg"),
            ]
            .into_iter()
            .map(|(v, l)| (v.into(), l.into()))
            .collect();
        }
        "/io/output_bit_depth" => {
            return [("", "Automatic"), ("8", "8"), ("16", "16"), ("32", "32")]
                .into_iter()
                .map(|(v, l)| (v.into(), l.into()))
                .collect();
        }
        "/io/input_color_space" | "/io/output_color_space" => &[
            "sRGB",
            "Display P3",
            "Adobe RGB (1998)",
            "ProPhoto RGB",
            "ITU-R BT.2020",
            "ACES2065-1",
            "ACEScg",
        ],
        "/io/output_compression" => &["zip", "none"],
        "/raw/white_balance" => &["as_shot", "daylight", "tungsten", "custom"],
        "/camera/auto_exposure_method" => &[
            "center_weighted",
            "average",
            "median",
            "partial",
            "matrix",
            "multi_zone",
            "highlight_weighted",
        ],
        "/io/input_gamut_compress/boundary" => &["inscribed_hull", "locus"],
        "/io/output_gamut_compress/algorithm" => {
            &["off", "oklch", "oklrab", "aces_rgc", "jzazbz", "cam16ucs"]
        }
        "/settings/rgb_to_raw_method" => &[
            "hanatos2025",
            "arctic2026beta04",
            "jakob2019",
            "mallett2019",
            "otsu2018",
            "gauss-lasers",
        ],
        "/camera/color_filter" => return s.filters.iter().map(|v| (v.clone(), name(v))).collect(),
        _ if key.ends_with("/filter_family") => {
            &["black_pro_mist", "glimmerglass", "pro_mist", "cinebloom"]
        }
        _ => &[],
    };
    values
        .iter()
        .map(|v| {
            (
                v.to_string(),
                if v.contains('_') {
                    name(v)
                } else {
                    v.to_string()
                },
            )
        })
        .collect()
}
fn flatten(value: &Value, key: &str, label: &str, s: &State, out: &mut Vec<Field>) {
    if key == "/io/output_gamut_compress/lightness_compression" {
        out.push(Field {
            key: format!("{key}/enabled").into(),
            label: "Lightness compression".into(),
            kind: 1,
            checked: !value.is_null(),
            ..Default::default()
        });
        if value.is_null() {
            return;
        }
    }
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_key(|(k, _)| {
                let path = format!("{key}/{k}");
                presentation::specs()
                    .iter()
                    .position(|s| s.path == path)
                    .unwrap_or(usize::MAX)
            });
            for (k, v) in entries {
                flatten(v, &format!("{key}/{k}"), &name(k), s, out);
            }
        }
        Value::Array(_) if key.starts_with("/special/") => {
            let options = choices(key, s);
            let selected = value.to_string();
            let label = if key.contains("film") {
                "Film dye order"
            } else {
                "Print dye order"
            };
            out.push(Field {
                key: key.into(),
                label: label.into(),
                kind: 2,
                value: options
                    .iter()
                    .find(|(v, _)| *v == selected)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_else(|| "Invalid dye order".into())
                    .into(),
                options: model(options.into_iter().map(|(_, l)| l)),
                ..Default::default()
            });
        }
        Value::Array(array) => {
            for (i, v) in array.iter().enumerate() {
                if let Some(row) = key.strip_prefix("/convert/calibration/") {
                    let channels = ["Red", "Green", "Blue"];
                    if let Ok(row) = row.parse::<usize>() {
                        flatten(
                            v,
                            &format!("{key}/{i}"),
                            &format!("{} → {}", channels[row], channels[i]),
                            s,
                            out,
                        );
                        continue;
                    }
                }
                let suffix = if key.ends_with("knee") || key.ends_with("lightness_compression") {
                    ["Threshold", "Limit", "Power"][i].to_string()
                } else if key.ends_with("particle_scale_sublayers") {
                    ["Fast", "Medium", "Slow"][i].to_string()
                } else if key.ends_with("crop_center") {
                    ["X", "Y"][i].to_string()
                } else if key.ends_with("crop_size") {
                    ["Width", "Height"][i].to_string()
                } else if key.ends_with("unsharp_mask") {
                    ["Radius", "Amount"][i].to_string()
                } else if key.ends_with("micro_structure") {
                    ["Amount", "Size"][i].to_string()
                } else if array.len() == 3
                    && !key.contains("knee")
                    && !key.contains("lightness")
                    && !key.contains("calibration")
                {
                    ["Red", "Green", "Blue"][i].to_string()
                } else {
                    (i + 1).to_string()
                };
                flatten(
                    v,
                    &format!("{key}/{i}"),
                    &format!("{label} · {suffix}"),
                    s,
                    out,
                );
            }
        }
        _ => {
            let options = choices(key, s);
            let raw = value.as_str().map(str::to_owned).unwrap_or_else(|| {
                if value.is_null() {
                    String::new()
                } else {
                    value.to_string()
                }
            });
            let display = options
                .iter()
                .find(|(v, _)| *v == raw)
                .map_or(raw.clone(), |(_, l)| l.clone());
            out.push(Field {
                key: key.into(),
                label: label.into(),
                value: display.into(),
                kind: if value.is_boolean() {
                    1
                } else if !options.is_empty() {
                    2
                } else {
                    0
                },
                checked: value.as_bool().unwrap_or(false),
                options: model(options.into_iter().map(|(_, l)| l)),
                hint: if value.is_null() {
                    "Automatic".into()
                } else {
                    "".into()
                },
                ..Default::default()
            });
        }
    }
}
pub fn fields(s: &State, tab: i32) -> Vec<Field> {
    let p = serde_json::to_value(&s.params).unwrap();
    let groups: &[(&str, &[&str])] = match tab {
        0 => &[
            ("RAW", &["/raw"]),
            (
                "Crop and upscale",
                &[
                    "/io/input_rotation",
                    "/io/crop",
                    "/io/crop_center",
                    "/io/crop_size",
                    "/io/upscale_factor",
                ],
            ),
            (
                "Input",
                &["/io/input_color_space", "/io/input_cctf_decoding"],
            ),
            (
                "Camera",
                &[
                    "/camera/exposure_compensation_ev",
                    "/camera/auto_exposure",
                    "/camera/film_format_mm",
                    "/camera/auto_exposure_method",
                    "/camera/color_filter",
                    "/camera/lens_blur_um",
                ],
            ),
            ("Profiles", &["/film_profile", "/print_profile"]),
            (
                "Enlarger",
                &[
                    "/enlarger/print_exposure",
                    "/enlarger/print_exposure_compensation",
                    "/enlarger/y_filter_shift",
                    "/enlarger/m_filter_shift",
                ],
            ),
            ("Scanner", &["/scanner"]),
            (
                "Output",
                &[
                    "/io/output_color_space",
                    "/io/output_cctf_encoding",
                    "/workflow/saving_color_space",
                    "/workflow/saving_cctf_encoding",
                    "/io/copy_metadata",
                    "/io/output_bit_depth",
                    "/io/output_compression",
                ],
            ),
        ],
        1 => &[
            ("Chemistry", &["/film_chemistry"]),
            ("Base", &["/film_base"]),
            ("Halation", &["/halation"]),
            ("Couplers", &["/dir_couplers"]),
            ("Grain", &["/grain/active", "/grain/rms_granularity"]),
            (
                "Grain / Pixel statistics",
                &[
                    "/grain/density_min",
                    "/grain/uniformity",
                    "/grain/particle_scale_sublayers",
                ],
            ),
            (
                "Grain / Texture",
                &[
                    "/grain/blur",
                    "/grain/mult_usm_amount",
                    "/grain/mult_usm_sigma",
                ],
            ),
            (
                "Grain / Micro substructure",
                &["/grain/blur_dye_clouds_um", "/grain/micro_structure"],
            ),
            ("Diffusion", &["/camera/diffusion_filter"]),
        ],
        2 => &[
            ("Chemistry", &["/print_chemistry"]),
            ("Base", &["/print_base"]),
            (
                "Preflash",
                &[
                    "/enlarger/preflash_exposure",
                    "/enlarger/preflash_y_filter_shift",
                    "/enlarger/preflash_m_filter_shift",
                ],
            ),
            ("Glare", &["/print_glare"]),
            ("Diffusion", &["/enlarger/diffusion_filter"]),
        ],
        3 => &[
            (
                "Spectral upsampling",
                &[
                    "/settings/rgb_to_raw_method",
                    "/settings/apply_hanatos2025_adaptation_window",
                    "/settings/apply_hanatos2025_adaptation_surface",
                    "/settings/spectral_gaussian_blur",
                ],
            ),
            ("Input gamut compress", &["/io/input_gamut_compress"]),
            ("Output gamut compress", &["/io/output_gamut_compress"]),
            ("Experimental", &["/enlarger/illuminant", "/special"]),
            ("Negative conversion", &["/convert"]),
        ],
        _ => &[
            (
                "Processing",
                &[
                    "/settings/preview_max_size",
                    "/settings/preview_mode",
                    "/settings/use_convert_lut",
                    "/settings/convert_lut_resolution",
                    "/settings/use_enlarger_lut",
                    "/settings/use_scanner_lut",
                    "/settings/lut_resolution",
                    "/settings/use_fast_stats",
                    "/settings/random_seed",
                ],
            ),
            (
                "Stock presets",
                &[
                    "/settings/neutral_print_filters_from_database",
                    "/settings/apply_coupler_presets",
                    "/settings/apply_grain_presets",
                ],
            ),
            (
                "Print balance",
                &[
                    "/enlarger/normalize_print_exposure",
                    "/enlarger/y_filter_neutral",
                    "/enlarger/m_filter_neutral",
                    "/enlarger/c_filter_neutral",
                ],
            ),
            (
                "Effects",
                &[
                    "/debug/deactivate_spatial_effects",
                    "/debug/deactivate_stochastic_effects",
                ],
            ),
        ],
    };
    let mut out = Vec::new();
    for (title, paths) in groups {
        let nested = title.split_once(" / ");
        if let Some((parent, _)) = nested
            && !s.sections.contains(&format!("{tab}:{parent}"))
        {
            continue;
        }
        let id = format!("{tab}:{title}");
        let open = s.sections.contains(&id);
        out.push(Field {
            key: id.into(),
            label: nested.map_or(*title, |(_, title)| title).into(),
            depth: if nested.is_some() { 1 } else { 0 },
            kind: 3,
            checked: open,
            ..Default::default()
        });
        if open {
            let start = out.len();
            for path in *paths {
                // The input algorithm is fixed; the experimental enlarger lens blur
                // and LUT-baking mode are intentionally not interactive photo controls.
                if let Some(value) = p.pointer(path) {
                    flatten(
                        value,
                        path,
                        &name(path.rsplit('/').next().unwrap()),
                        s,
                        &mut out,
                    );
                }
            }
            for field in &mut out[start..] {
                field.depth = if nested.is_some() { 2 } else { 1 };
            }
        }
    }
    out.retain(|f| {
        !["/io/input_gamut_compress/algorithm", "/print_base/tilt"].contains(&f.key.as_str())
    });
    for field in &mut out {
        if field.kind != 3 {
            field.tooltip = presentation::wrapped_tooltip(&field.key).into();
            let value = p.pointer(&field.key);
            field.wheel = field.kind == 1
                || field.kind == 2
                || value.is_some_and(|v| v.is_number() || v.is_null())
                    && field.key != "/workflow/saving_color_space";
        }
    }
    out
}

pub fn workflow(s: &State) -> Field {
    let mut fields = Vec::new();
    flatten(
        &Value::String(s.params.workflow.route.clone()),
        "/workflow/route",
        "Workflow",
        s,
        &mut fields,
    );
    let mut field = fields.pop().unwrap();
    field.tooltip = presentation::wrapped_tooltip(&field.key).into();
    field.wheel = true;
    field
}

pub fn edit(s: &mut State, key: &str, text: &str) -> Result<bool> {
    let mut p = serde_json::to_value(&s.params)?;
    if key == "/io/output_gamut_compress/lightness_compression/enabled" {
        let previous = s.params.io.output_gamut_compress.lightness_compression;
        s.params.io.output_gamut_compress.lightness_compression = if text.parse::<bool>()? {
            previous.or(Some([0.95, 1., 1.6]))
        } else {
            None
        };
        return Ok(previous != s.params.io.output_gamut_compress.lightness_compression);
    }
    let options = choices(key, s);
    let raw = options
        .iter()
        .find(|(_, l)| l == text)
        .map_or(text, |(v, _)| v.as_str());
    let value = p.pointer_mut(key).context("Unknown control")?;
    let previous = value.clone();
    *value = if key.starts_with("/special/") {
        serde_json::from_str(raw)?
    } else if value.is_boolean() {
        Value::Bool(raw.parse()?)
    } else if key == "/workflow/saving_color_space" {
        if raw.is_empty() {
            Value::Null
        } else {
            Value::String(raw.into())
        }
    } else if value.is_string() {
        Value::String(raw.into())
    } else if raw.trim().is_empty() {
        Value::Null
    } else {
        let n: f64 = raw.parse().context("Enter a number")?;
        ensure!(n.is_finite(), "Enter a finite number");
        // Preserve integer fields, while allowing normal decimal entry for floats.
        if value.is_u64() || key.ends_with("random_seed") || key.ends_with("output_bit_depth") {
            ensure!(
                n >= 0. && n.fract() == 0. && n < u64::MAX as f64,
                "Enter a non-negative whole number"
            );
            Value::from(n as u64)
        } else {
            Value::from(n)
        }
    };
    if *value == previous {
        return Ok(false);
    }
    let mut next: Params = serde_json::from_value(p)?;
    if key.starts_with("/grain/") {
        next.settings.apply_grain_presets = false;
    }
    if key.starts_with("/dir_couplers/") {
        next.settings.apply_coupler_presets = false;
    }
    if [
        "/enlarger/y_filter_neutral",
        "/enlarger/m_filter_neutral",
        "/enlarger/c_filter_neutral",
    ]
    .contains(&key)
    {
        next.settings.neutral_print_filters_from_database = false;
    }
    s.params = next;
    Ok(true)
}

/// Wheel edits use the reference's increments and editor bounds. Text entry keeps
/// float64 precision and remains subject to the processing model's validation.
pub fn step(s: &mut State, key: &str, steps: i32) -> Result<bool> {
    if steps == 0 {
        return Ok(false);
    }
    let p = serde_json::to_value(&s.params)?;
    if key == "/io/output_gamut_compress/lightness_compression/enabled" {
        return edit(s, key, if steps > 0 { "true" } else { "false" });
    }
    let value = p.pointer(key).context("Unknown control")?;
    let options = choices(key, s);
    if !options.is_empty() {
        let raw = value.as_str().map(str::to_owned).unwrap_or_else(|| {
            if value.is_null() {
                String::new()
            } else {
                value.to_string()
            }
        });
        let index = options
            .iter()
            .position(|(v, _)| *v == raw)
            .context("Unknown option")?;
        let next = (index as i64 - i64::from(steps)).clamp(0, options.len() as i64 - 1) as usize;
        return edit(s, key, &options[next].1);
    }
    if value.is_boolean() {
        return edit(s, key, if steps > 0 { "true" } else { "false" });
    }
    ensure!(
        value.is_number() || value.is_null(),
        "This field has no numeric wheel control"
    );
    let spec = presentation::spec(key);
    let step = spec.and_then(|s| s.step).unwrap_or(1.);
    let mut number = value.as_f64().unwrap_or(0.) + f64::from(steps) * step;
    let unsigned =
        value.is_u64() || key.ends_with("random_seed") || key.ends_with("output_bit_depth");
    let minimum = spec
        .and_then(|s| s.min)
        .unwrap_or(if unsigned { 0. } else { -1_000_000. });
    let maximum = spec.and_then(|s| s.max).unwrap_or(1_000_000.);
    number = if key == "/io/input_rotation" {
        number.clamp(0., 3.)
    } else {
        number.clamp(minimum, maximum)
    };
    if unsigned {
        number = number.round();
    }
    edit(s, key, &number.to_string())
}
