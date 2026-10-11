//! Shared colour-managed image I/O for the CLI and darkroom.
use crate::{
    Image, Params,
    color::{Tables, decode, encode, mat_vec},
    image::decode_raw_crop,
};
use anyhow::{Context, Result, ensure};
use image::{ImageDecoder, ImageEncoder};
use lcms2::{Intent, PixelFormat, Profile, Transform};
use std::{fs::File, io::BufWriter, path::Path};
pub fn is_rgb_file(path: &Path) -> bool {
    matches!(
        extension(path).as_str(),
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "webp" | "exr"
    )
}
pub fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub fn icc(data: &Path, space: &str, encoded: bool) -> Result<Vec<u8>> {
    let name = match (space, encoded) {
        ("sRGB", true) => "ellelstone/sRGB-elle-V2-srgbtrc.icc",
        ("sRGB", false) => "ellelstone/sRGB-elle-V2-g10.icc",
        ("Adobe RGB (1998)", true) => "ellelstone/ClayRGB-elle-V2-g22.icc",
        ("Adobe RGB (1998)", false) => "ellelstone/ClayRGB-elle-V2-g10.icc",
        ("ProPhoto RGB", true) => "ellelstone/LargeRGB-elle-V2-g18.icc",
        ("ProPhoto RGB", false) => "ellelstone/LargeRGB-elle-V2-g10.icc",
        ("ITU-R BT.2020", true) => "ellelstone/Rec2020-elle-V2-rec709.icc",
        ("ITU-R BT.2020", false) => "ellelstone/Rec2020-elle-V2-g10.icc",
        ("ACES2065-1", _) => "ellelstone/ACES-elle-V2-g10.icc",
        ("Display P3", true) => "saucecontrol/DisplayP3-v2-micro.icc",
        ("DCI-P3", true) => "saucecontrol/DCI-P3-v4.icc",
        _ => anyhow::bail!("No embedded ICC profile for {space} with encoding={encoded}"),
    };
    Ok(std::fs::read(data.join("icc").join(name))?)
}
pub fn transform_icc(image: &mut Image, input: &[u8], output: &[u8]) -> Result<()> {
    ensure!(image.channels == 3, "ICC transform requires RGB");
    let a = Profile::new_icc(input)?;
    let b = Profile::new_icc(output)?;
    let transform: Transform<[f64; 3], [f64; 3]> = Transform::new(
        &a,
        PixelFormat::RGB_DBL,
        &b,
        PixelFormat::RGB_DBL,
        Intent::RelativeColorimetric,
    )?;
    // Bound the secondary buffer rather than copying the entire full-resolution image.
    for chunk in image.data.chunks_mut(3 * 4096) {
        let mut pixels: Vec<_> = chunk.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect();
        transform.transform_in_place(&mut pixels);
        for (out, p) in chunk.chunks_exact_mut(3).zip(pixels) {
            out.copy_from_slice(&p);
        }
    }
    image.validate()
}
pub fn read(path: &Path, data: &Path, p: &Params, crop: Option<usize>) -> Result<Image> {
    if extension(path) == "npy" {
        let im = Image::read_npy(path)?;
        return if let Some(size) = crop {
            im.center_crop(size)
        } else {
            Ok(im)
        };
    }
    let tables = Tables::load(data)?;
    let mut im;
    if is_rgb_file(path) {
        let mut decoder = image::ImageReader::open(path)?
            .with_guessed_format()?
            .into_decoder()?;
        let profile = decoder.icc_profile()?;
        let orientation = decoder.orientation()?;
        let mut decoded = image::DynamicImage::from_decoder(decoder)?;
        decoded.apply_orientation(orientation);
        let rgb = decoded.to_rgb32f();
        im = Image::new(
            rgb.width() as usize,
            rgb.height() as usize,
            3,
            rgb.into_raw().into_iter().map(f64::from).collect(),
        )?;
        let tagged = profile.is_some();
        if let Some(profile) = profile {
            let target = icc(data, "sRGB", true)?;
            if profile != target {
                transform_icc(&mut im, &profile, &target)?;
            }
        }
        if (tagged || matches!(extension(path).as_str(), "jpg" | "jpeg" | "png" | "webp"))
            && !(p.io.input_color_space == "sRGB" && p.io.input_cctf_decoding)
        {
            let m = tables.rgb_to_rgb("sRGB", &p.io.input_color_space)?;
            for v in im.data.chunks_exact_mut(3) {
                let rgb = mat_vec(
                    &m,
                    [
                        decode("sRGB", v[0]),
                        decode("sRGB", v[1]),
                        decode("sRGB", v[2]),
                    ],
                );
                for c in 0..3 {
                    v[c] = if p.io.input_cctf_decoding {
                        encode(&p.io.input_color_space, rgb[c])
                    } else {
                        rgb[c]
                    };
                }
            }
        }
        if let Some(size) = crop {
            im = im.center_crop(size)?;
        }
    } else {
        crate::raw::validate(&p.raw)?;
        im = decode_raw_crop(
            path,
            false,
            p.raw.white_balance == "as_shot",
            if p.raw.lens_correction { None } else { crop },
        )?;
        if p.raw.lens_correction {
            let metadata = crate::lens::metadata(path)?;
            crate::lens::correct(&mut im, &metadata)?;
            if let Some(size) = crop {
                im = im.center_crop(size)?;
            }
        }
        crate::raw::adjust(&mut im, &tables, &p.raw)?;
        let m = tables.rgb_to_rgb("ACES2065-1", &p.io.input_color_space)?;
        for v in im.data.chunks_exact_mut(3) {
            let rgb = mat_vec(&m, [v[0], v[1], v[2]]);
            for c in 0..3 {
                v[c] = if p.io.input_cctf_decoding {
                    encode(&p.io.input_color_space, rgb[c])
                } else {
                    rgb[c]
                };
            }
        }
    }
    Ok(im)
}
pub fn write(image: &Image, path: &Path, data: &Path, space: &str, encoded: bool) -> Result<()> {
    image.validate()?;
    if extension(path) == "npy" {
        return image.write_npy(path);
    }
    ensure!(image.channels == 3, "Image export requires RGB");
    let profile = icc(data, space, encoded).ok();
    match extension(path).as_str() {
        "tif" | "tiff" => {
            let mut encoder = tiff::encoder::TiffEncoder::new(BufWriter::new(File::create(path)?))?;
            let mut frame = encoder.new_image::<tiff::encoder::colortype::RGB32Float>(
                u32::try_from(image.width)?,
                u32::try_from(image.height)?,
            )?;
            if let Some(profile) = profile {
                frame
                    .encoder()
                    .write_tag(tiff::tags::Tag::Unknown(34675), profile.as_slice())?;
            }
            frame.write_data(&image.data.iter().map(|v| *v as f32).collect::<Vec<_>>())?;
        }
        "exr" => {
            let pixels: Vec<f32> = image.data.iter().map(|v| *v as f32).collect();
            let frame = image::Rgb32FImage::from_raw(
                u32::try_from(image.width)?,
                u32::try_from(image.height)?,
                pixels,
            )
            .context("Invalid EXR dimensions")?;
            frame.save_with_format(path, image::ImageFormat::OpenExr)?;
        }
        "png" => {
            let mut encoder =
                image::codecs::png::PngEncoder::new(BufWriter::new(File::create(path)?));
            if let Some(profile) = profile {
                encoder.set_icc_profile(profile)?;
            }
            let bytes: Vec<_> = image
                .data
                .iter()
                .flat_map(|v| ((v.clamp(0., 1.) * 65535.).round() as u16).to_ne_bytes())
                .collect();
            encoder.write_image(
                &bytes,
                u32::try_from(image.width)?,
                u32::try_from(image.height)?,
                image::ExtendedColorType::Rgb16,
            )?;
        }
        "jpg" | "jpeg" => {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                BufWriter::new(File::create(path)?),
                95,
            );
            if let Some(profile) = profile {
                encoder.set_icc_profile(profile)?;
            }
            let bytes: Vec<_> = image
                .data
                .iter()
                .map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
                .collect();
            encoder.write_image(
                &bytes,
                u32::try_from(image.width)?,
                u32::try_from(image.height)?,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        _ => anyhow::bail!("Output extension must be PNG, JPEG, TIFF or NPY"),
    }
    Ok(())
}
