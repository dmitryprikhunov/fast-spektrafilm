//! Typed parameter editor. Serialization is an internal bridge, never UI text.
use super::{Field, State, backend, model};
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
            for (k, v) in map {
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
            });
        }
    }
}
pub fn fields(s: &State, tab: i32) -> Vec<Field> {
    let p = serde_json::to_value(&s.params).unwrap();
    let groups: &[(&str, &[&str])] = match tab {
        0 => &[
            (
                "Profiles",
                &["/film_profile", "/print_profile", "/workflow"],
            ),
            ("RAW", &["/raw"]),
            (
                "Preview and crop",
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
                    "/camera/auto_exposure_method",
                    "/camera/film_format_mm",
                    "/camera/lens_blur_um",
                    "/camera/color_filter",
                ],
            ),
            (
                "Enlarger",
                &[
                    "/enlarger/print_exposure_compensation",
                    "/enlarger/print_exposure",
                    "/enlarger/y_filter_shift",
                    "/enlarger/m_filter_shift",
                    "/enlarger/illuminant",
                ],
            ),
            ("Scanner", &["/scanner"]),
            (
                "Output",
                &[
                    "/io/output_color_space",
                    "/io/output_cctf_encoding",
                    "/io/copy_metadata",
                    "/io/output_bit_depth",
                    "/io/output_compression",
                ],
            ),
        ],
        1 => &[
            ("Development", &["/film_chemistry"]),
            ("Film base", &["/film_base"]),
            ("Grain", &["/grain"]),
            ("Halation", &["/halation"]),
            ("DIR couplers", &["/dir_couplers"]),
            ("Camera diffusion", &["/camera/diffusion_filter"]),
        ],
        2 => &[
            ("Development", &["/print_chemistry"]),
            ("Paper base", &["/print_base"]),
            ("Glare", &["/print_glare"]),
            ("Enlarger diffusion", &["/enlarger/diffusion_filter"]),
            (
                "Preflash",
                &[
                    "/enlarger/preflash_exposure",
                    "/enlarger/preflash_y_filter_shift",
                    "/enlarger/preflash_m_filter_shift",
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
        ],
        3 => &[
            ("Negative conversion", &["/convert"]),
            ("Dye order", &["/special"]),
            ("Input gamut", &["/io/input_gamut_compress"]),
            ("Output gamut", &["/io/output_gamut_compress"]),
        ],
        _ => &[
            ("Processing", &["/settings"]),
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
        let id = format!("{tab}:{title}");
        let open = s.sections.contains(&id);
        out.push(Field {
            key: id.into(),
            label: (*title).into(),
            kind: 3,
            checked: open,
            ..Default::default()
        });
        if open {
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
        }
    }
    out.retain(|f| {
        !["/io/input_gamut_compress/algorithm", "/print_base/tilt"].contains(&f.key.as_str())
    });
    out
}
pub fn edit(s: &mut State, key: &str, text: &str) -> Result<()> {
    let mut p = serde_json::to_value(&s.params)?;
    if key == "/io/output_gamut_compress/lightness_compression/enabled" {
        s.params.io.output_gamut_compress.lightness_compression = if text == "true" {
            Some([0.95, 1., 1.6])
        } else {
            None
        };
        return Ok(());
    }
    let options = choices(key, s);
    let raw = options
        .iter()
        .find(|(_, l)| l == text)
        .map_or(text, |(v, _)| v.as_str());
    let value = p.pointer_mut(key).context("Unknown control")?;
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
    Ok(())
}
