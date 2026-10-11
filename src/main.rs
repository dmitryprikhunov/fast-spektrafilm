use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use fast_spektrafilm::{
    Image, Params, Simulator, Tap,
    color::{Tables, mat_vec},
    image::{decode_raw_crop, raw_version},
    profile::Profile,
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long,global=true,default_value=concat!(env!("CARGO_MANIFEST_DIR"),"/data"))]
    data_dir: PathBuf,
    #[arg(long, global = true)]
    threads: Option<usize>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// List the original film and print profiles.
    Profiles,
    /// Emit all supported configuration fields and defaults.
    Defaults,
    /// Decode a camera RAW to linear RGB NPY or float TIFF.
    Decode {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        half_size: bool,
        #[arg(long)]
        crop_size: Option<usize>,
        #[arg(long, default_value = "as_shot")]
        white_balance: String,
        #[arg(long, default_value = "ProPhoto RGB")]
        color_space: String,
        #[arg(long, default_value_t = 5500.)]
        temperature: f64,
        #[arg(long, default_value_t = 1.)]
        tint: f64,
    },
    /// Process an RGB NPY or RAW through the film pipeline.
    Process {
        input: PathBuf,
        output: PathBuf,
        /// Process a center crop; RAW conversion allocates floats for this crop only.
        #[arg(long)]
        crop_size: Option<usize>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        film: Option<String>,
        #[arg(long)]
        print: Option<String>,
        #[arg(long)]
        deterministic: bool,
        #[arg(long)]
        no_spatial: bool,
        #[arg(long)]
        no_auto_exposure: bool,
        #[arg(long,value_enum,default_value_t=Tap::RgbOut)]
        collect: Tap,
        #[arg(long, default_value_t = 1)]
        repeat: usize,
    },
    /// Fit a scanned negative's clear base or device calibration and save settings.
    Calibrate {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        device: bool,
    },
    /// Bake the deterministic transform to a standard .cube LUT.
    Bake {
        output: PathBuf,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, default_value_t = 33)]
        resolution: usize,
    },
}
fn write(image: &Image, path: &Path) -> Result<()> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "npy" => image.write_npy(path),
        "tif" | "tiff" => image.write_tiff(path),
        _ => anyhow::bail!("Output extension must be .npy or .tiff"),
    }
}
fn read_params(path: Option<PathBuf>) -> Result<Params> {
    if let Some(p) = path {
        Ok(serde_json::from_reader(std::fs::File::open(p)?)?)
    } else {
        Ok(Params::default())
    }
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(n) = cli.threads {
        ensure!(n > 0, "Thread count must be positive");
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()?;
    }
    match cli.command {
        Command::Profiles => {
            let mut files = std::fs::read_dir(cli.data_dir.join("profiles"))?
                .map(|v| v.map(|v| v.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            files.sort();
            for f in files {
                if f.extension().and_then(|v| v.to_str()) != Some("json") {
                    continue;
                }
                let stock = f.file_stem().unwrap().to_string_lossy();
                let p = Profile::load(&cli.data_dir, &stock)?;
                println!(
                    "{}\t{}\t{}\t{}",
                    stock, p.info.stage, p.info.channel_model, p.info.name
                );
            }
        }
        Command::Defaults => println!("{}", serde_json::to_string_pretty(&Params::default())?),
        Command::Decode {
            input,
            output,
            half_size,
            crop_size,
            white_balance,
            color_space,
            temperature,
            tint,
        } => {
            let raw = fast_spektrafilm::params::RawSettings {
                white_balance,
                temperature,
                tint,
                ..Default::default()
            };
            fast_spektrafilm::raw::validate(&raw)?;
            let start = Instant::now();
            let mut image =
                decode_raw_crop(&input, half_size, raw.white_balance == "as_shot", crop_size)?;
            fast_spektrafilm::raw::adjust(&mut image, &Tables::load(&cli.data_dir)?, &raw)?;
            if color_space != "ACES2065-1" {
                let t = Tables::load(&cli.data_dir)?;
                let m = t.rgb_to_rgb("ACES2065-1", &color_space)?;
                for p in image.data.chunks_exact_mut(3) {
                    let v = mat_vec(&m, [p[0], p[1], p[2]]);
                    p.copy_from_slice(&v);
                }
            }
            let secs = start.elapsed().as_secs_f64();
            write(&image, &output)?;
            eprintln!(
                "{}",
                serde_json::json!({"libraw":raw_version(),"decode_seconds":secs,"width":image.width,"height":image.height,"color_space":color_space})
            );
        }
        Command::Process {
            input,
            output,
            crop_size,
            config,
            film,
            print,
            deterministic,
            no_spatial,
            no_auto_exposure,
            collect,
            repeat,
        } => {
            ensure!(repeat > 0, "repeat must be positive");
            let mut params = read_params(config)?;
            if let Some(film) = film {
                params.film_profile = film;
            }
            if let Some(print) = print {
                params.print_profile = print;
            }
            if deterministic {
                params.debug.deactivate_stochastic_effects = true;
            }
            if no_spatial {
                params.debug.deactivate_spatial_effects = true;
            }
            if no_auto_exposure {
                params.camera.auto_exposure = false;
            }
            let start = Instant::now();
            let image = fast_spektrafilm::file_io::read(&input, &cli.data_dir, &params, crop_size)?;
            let decode_seconds = start.elapsed().as_secs_f64();
            let sim = Simulator::new(&cli.data_dir, params)?;
            let mut times = Vec::new();
            for _ in 1..repeat {
                let input = image.clone();
                let start = Instant::now();
                std::hint::black_box(sim.process_owned(input, collect)?);
                times.push(start.elapsed().as_secs_f64());
            }
            let start = Instant::now();
            let result = sim.process_owned(image, collect)?;
            times.push(start.elapsed().as_secs_f64());
            fast_spektrafilm::file_io::write(
                &result,
                &output,
                &cli.data_dir,
                &sim.params().io.output_color_space,
                sim.params().io.output_cctf_encoding,
            )?;
            eprintln!(
                "{}",
                serde_json::json!({"setup_seconds":sim.setup_seconds,"decode_seconds":decode_seconds,"process_seconds":times,"width":result.width,"height":result.height,"channels":result.channels,"tap":collect.name(),"threads":rayon::current_num_threads()})
            );
        }
        Command::Calibrate {
            input,
            output,
            config,
            device,
        } => {
            ensure!(
                input != output,
                "Settings output must differ from the source image"
            );
            let mut p = read_params(config)?;
            let mut image = fast_spektrafilm::file_io::read(&input, &cli.data_dir, &p, None)?;
            if p.io.input_cctf_decoding {
                image.data.iter_mut().for_each(|v| {
                    *v = fast_spektrafilm::color::decode(&p.io.input_color_space, *v)
                });
            }
            let tables = Tables::load(&cli.data_dir)?;
            if device {
                p.workflow.route = "input > convert-film > scan".into();
                let sim = Simulator::new(&cli.data_dir, p.clone())?;
                p.convert.calibration =
                    fast_spektrafilm::convert::FilmConverter::new(&tables, sim.film(), &p)?
                        .fit_calibration(&image)?;
            } else {
                let sample =
                    fast_spektrafilm::convert::sample_base(&image, p.convert.base_percentile)?;
                let stock = Profile::load(&cli.data_dir, &p.film_profile)?;
                (p.film_base, p.convert.exposure_compensation_ev) =
                    fast_spektrafilm::convert::fit_base(&tables, &stock, &p, sample)?;
            }
            std::fs::write(output, serde_json::to_string_pretty(&p)?)?;
        }
        Command::Bake {
            output,
            config,
            resolution,
        } => {
            ensure!(
                (2..=256).contains(&resolution),
                "LUT resolution must be 2..256"
            );
            let mut params = read_params(config)?;
            params.debug.lut_mode = true;
            let sim = Simulator::new(&cli.data_dir, params)?;
            let mut rgb = Vec::with_capacity(resolution.pow(3) * 3);
            // .cube stores red fastest, then green, then blue.
            for b in 0..resolution {
                for g in 0..resolution {
                    for r in 0..resolution {
                        rgb.extend([
                            r as f64 / (resolution - 1) as f64,
                            g as f64 / (resolution - 1) as f64,
                            b as f64 / (resolution - 1) as f64,
                        ]);
                    }
                }
            }
            let result = sim.process_owned(
                Image::new(resolution * resolution, resolution, 3, rgb)?,
                Tap::RgbOut,
            )?;
            use std::io::Write;
            let mut file = std::io::BufWriter::new(std::fs::File::create(output)?);
            writeln!(
                file,
                "# fast-spektrafilm; baseline 28bf883e1672e884307edc75852549376e13644e\nLUT_3D_SIZE {resolution}\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1"
            )?;
            for p in result.data.chunks_exact(3) {
                writeln!(file, "{:.12} {:.12} {:.12}", p[0], p[1], p[2])?;
            }
        }
    }
    Ok(())
}
