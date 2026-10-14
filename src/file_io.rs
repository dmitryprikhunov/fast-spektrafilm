//! Shared colour-managed image I/O for the CLI and darkroom.
use crate::{
    Image, Params,
    color::{Tables, decode, encode, mat_vec},
    image::decode_raw_crop,
};
use anyhow::{Result, ensure};
use image::{ImageDecoder, ImageEncoder};
use lcms2::{Intent, PixelFormat, Profile, Transform};
use std::{fs::File, io::BufWriter, path::Path};

/// Refuse source aliases, including symlinks and hard links. Existing distinct
/// destinations are replaced only after encoding and metadata copying succeed.
pub fn validate_destination(source: &Path, destination: &Path) -> Result<()> {
    ensure!(source != destination, "Output would overwrite the source");
    if let (Ok(a), Ok(b)) = (source.canonicalize(), destination.canonicalize()) {
        ensure!(a != b, "Output would overwrite the source");
    }
    #[cfg(unix)]
    if let (Ok(a), Ok(b)) = (source.metadata(), destination.metadata()) {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            a.dev() != b.dev() || a.ino() != b.ino(),
            "Output is a hard link to the source"
        );
    }
    Ok(())
}
pub fn transactional_save(
    source: &Path,
    destination: &Path,
    save: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    validate_destination(source, destination)?;
    let temp = tempfile::Builder::new()
        .suffix(&format!(".{}", extension(destination)))
        .tempfile_in(
            destination
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?;
    save(temp.path())?;
    // Metadata encoders may replace the temporary inode. Sync the final path.
    File::open(temp.path())?.sync_all()?;
    validate_destination(source, destination)?;
    temp.persist(destination)?;
    Ok(())
}
pub fn save(
    image: &Image,
    source: &Path,
    destination: &Path,
    data: &Path,
    p: &Params,
) -> Result<()> {
    let converted;
    let (image, space, encoded) = if let Some(space) = &p.workflow.saving_color_space {
        converted = convert_color(
            image,
            &Tables::load(data)?,
            &p.io.output_color_space,
            p.io.output_cctf_encoding,
            space,
            p.workflow.saving_cctf_encoding,
        )?;
        (&converted, space.as_str(), p.workflow.saving_cctf_encoding)
    } else {
        (
            image,
            p.io.output_color_space.as_str(),
            p.io.output_cctf_encoding,
        )
    };
    transactional_save(source, destination, |temp| {
        write_with_options(
            image,
            temp,
            data,
            space,
            encoded,
            p.io.output_bit_depth,
            &p.io.output_compression,
        )?;
        if p.io.copy_metadata {
            crate::metadata::copy(source, temp, [image.width, image.height], space, encoded)?;
        }
        Ok(())
    })
}
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
pub fn convert_color(
    image: &Image,
    tables: &Tables,
    from: &str,
    from_encoded: bool,
    to: &str,
    to_encoded: bool,
) -> Result<Image> {
    image.validate()?;
    ensure!(image.channels == 3, "Colour conversion requires RGB");
    let m = tables.rgb_to_rgb(from, to)?;
    if from == to && from_encoded == to_encoded {
        return Ok(image.clone());
    }
    let mut out = image.clone();
    for p in out.data.chunks_exact_mut(3) {
        let rgb = [p[0], p[1], p[2]].map(|v| if from_encoded { decode(from, v) } else { v });
        p.copy_from_slice(&mat_vec(&m, rgb).map(|v| if to_encoded { encode(to, v) } else { v }));
    }
    out.validate()?;
    Ok(out)
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
/// Read TIFF ICC tags independently of the image crate's pixel-sized decode
/// buffer. Its TIFF wrapper can silently drop profiles larger than a tiny image.
pub fn embedded_icc(path: &Path) -> Result<Option<Vec<u8>>> {
    if ["tif", "tiff"].contains(&extension(path).as_str()) {
        let mut decoder = tiff::decoder::Decoder::new(std::io::BufReader::new(File::open(path)?))?;
        return match decoder.find_tag(tiff::tags::Tag::Unknown(34675))? {
            Some(value) => Ok(Some(value.into_u8_vec()?)),
            None => Ok(None),
        };
    }
    Ok(image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()?
        .icc_profile()?)
}
pub fn read(path: &Path, data: &Path, p: &Params, crop: Option<usize>) -> Result<Image> {
    if extension(path) == "npy" {
        let im = Image::read_npy(path)?.rotate_owned(p.io.input_rotation)?;
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
        let profile = if ["tif", "tiff"].contains(&extension(path).as_str()) {
            embedded_icc(path)?
        } else {
            decoder.icc_profile()?
        };
        let orientation = decoder.orientation()?;
        let mut decoded = image::DynamicImage::from_decoder(decoder)?;
        decoded.apply_orientation(orientation);
        let (width, height) = (decoded.width() as usize, decoded.height() as usize);
        let pixels = match decoded {
            image::DynamicImage::ImageLuma8(_)
            | image::DynamicImage::ImageLumaA8(_)
            | image::DynamicImage::ImageRgb8(_)
            | image::DynamicImage::ImageRgba8(_) => decoded
                .to_rgb8()
                .into_raw()
                .into_iter()
                .map(|v| v as f64 / 255.)
                .collect(),
            image::DynamicImage::ImageLuma16(_)
            | image::DynamicImage::ImageLumaA16(_)
            | image::DynamicImage::ImageRgb16(_)
            | image::DynamicImage::ImageRgba16(_) => decoded
                .to_rgb16()
                .into_raw()
                .into_iter()
                .map(|v| v as f64 / 65535.)
                .collect(),
            _ => decoded
                .to_rgb32f()
                .into_raw()
                .into_iter()
                .map(f64::from)
                .collect(),
        };
        im = Image::new(width, height, 3, pixels)?;
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
    } else {
        crate::raw::validate(&p.raw)?;
        im = decode_raw_crop(
            path,
            false,
            p.raw.white_balance == "as_shot",
            if p.raw.lens_correction || p.io.input_rotation != 0 {
                None
            } else {
                crop
            },
        )?;
        if p.raw.lens_correction {
            let metadata = crate::lens::metadata(path)?;
            crate::lens::correct(&mut im, &metadata)?;
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
    im = im.rotate_owned(p.io.input_rotation)?;
    if let Some(size) = crop {
        im = im.center_crop(size)?;
    }
    Ok(im)
}
pub fn write(image: &Image, path: &Path, data: &Path, space: &str, encoded: bool) -> Result<()> {
    write_with_options(image, path, data, space, encoded, None, "zip")
}
/// Explicit export precision; integer encodings truncate as in OpenImageIO.
pub fn write_with_options(
    image: &Image,
    path: &Path,
    data: &Path,
    space: &str,
    encoded: bool,
    bit_depth: Option<u8>,
    compression: &str,
) -> Result<()> {
    image.validate()?;
    ensure!(
        ["zip", "none"].contains(&compression),
        "Unsupported output compression {compression}"
    );
    if extension(path) == "npy" {
        return image.write_npy(path);
    }
    ensure!(image.channels == 3, "Image export requires RGB");
    // Unsupported representations have no copied ICC; missing expected assets
    // remain errors. Do not suppress broken installations.
    let profile = if matches!(
        (space, encoded),
        ("ACEScg", _) | ("Display P3" | "DCI-P3", false)
    ) {
        None
    } else {
        Some(icc(data, space, encoded)?)
    };
    let bits = bit_depth.unwrap_or(match extension(path).as_str() {
        "png" => 16,
        "jpg" | "jpeg" => 8,
        _ => 32,
    });
    match extension(path).as_str() {
        "tif" | "tiff" => {
            ensure!(
                [8, 16, 32].contains(&bits),
                "TIFF supports 8, 16 or 32 bits"
            );
            let compression = if compression == "zip" {
                tiff::encoder::Compression::Deflate(tiff::encoder::DeflateLevel::default())
            } else {
                tiff::encoder::Compression::Uncompressed
            };
            let mut encoder = tiff::encoder::TiffEncoder::new(BufWriter::new(File::create(path)?))?
                .with_compression(compression);
            macro_rules! frame {
                ($kind:ty, $pixels:expr) => {{
                    let mut frame = encoder.new_image::<$kind>(
                        u32::try_from(image.width)?,
                        u32::try_from(image.height)?,
                    )?;
                    if let Some(profile) = &profile {
                        frame
                            .encoder()
                            .write_tag(tiff::tags::Tag::Unknown(34675), profile.as_slice())?;
                    }
                    frame.write_data(&$pixels)?;
                }};
            }
            match bits {
                8 => frame!(
                    tiff::encoder::colortype::RGB8,
                    image
                        .data
                        .iter()
                        .map(|v| (v.clamp(0., 1.) * 255.) as u8)
                        .collect::<Vec<_>>()
                ),
                16 => frame!(
                    tiff::encoder::colortype::RGB16,
                    image
                        .data
                        .iter()
                        .map(|v| (v.clamp(0., 1.) * 65535.) as u16)
                        .collect::<Vec<_>>()
                ),
                _ => frame!(
                    tiff::encoder::colortype::RGB32Float,
                    image.data.iter().map(|v| *v as f32).collect::<Vec<_>>()
                ),
            }
        }
        "exr" => {
            ensure!([16, 32].contains(&bits), "EXR supports 16 or 32 bits");
            use exr::prelude::*;
            let encoding = Encoding {
                compression: if compression == "zip" {
                    Compression::ZIP16
                } else {
                    Compression::Uncompressed
                },
                ..Encoding::SMALL_LOSSLESS
            };
            macro_rules! exr_frame {
                ($kind:ty, $convert:expr) => {{
                    let channels = SpecificChannels::rgb(|pos: Vec2<usize>| {
                        let p = &image.data[(pos.y() * image.width + pos.x()) * 3..][..3];
                        let convert: fn(f64) -> $kind = $convert;
                        (convert(p[0]), convert(p[1]), convert(p[2]))
                    });
                    let layer = Layer::new(
                        (image.width, image.height),
                        LayerAttributes::default(),
                        encoding,
                        channels,
                    );
                    exr::image::Image::from_layer(layer).write().to_file(path)?;
                }};
            }
            if bits == 16 {
                exr_frame!(half::f16, half::f16::from_f64);
            } else {
                exr_frame!(f32, |v| v as f32);
            }
        }
        "png" => {
            ensure!([8, 16].contains(&bits), "PNG supports 8 or 16 bits");
            let mut encoder =
                image::codecs::png::PngEncoder::new(BufWriter::new(File::create(path)?));
            if let Some(profile) = profile {
                encoder.set_icc_profile(profile)?;
            }
            let bytes: Vec<_> = if bits == 16 {
                image
                    .data
                    .iter()
                    .flat_map(|v| ((v.clamp(0., 1.) * 65535.) as u16).to_ne_bytes())
                    .collect()
            } else {
                image
                    .data
                    .iter()
                    .map(|v| (v.clamp(0., 1.) * 255.) as u8)
                    .collect()
            };
            encoder.write_image(
                &bytes,
                u32::try_from(image.width)?,
                u32::try_from(image.height)?,
                if bits == 16 {
                    image::ExtendedColorType::Rgb16
                } else {
                    image::ExtendedColorType::Rgb8
                },
            )?;
        }
        "jpg" | "jpeg" => {
            ensure!(bits == 8, "JPEG supports 8 bits");
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
                .map(|v| (v.clamp(0., 1.) * 255.) as u8)
                .collect();
            encoder.write_image(
                &bytes,
                u32::try_from(image.width)?,
                u32::try_from(image.height)?,
                image::ExtendedColorType::Rgb8,
            )?;
        }
        _ => anyhow::bail!("Output extension must be PNG, JPEG, TIFF, EXR or NPY"),
    }
    Ok(())
}
