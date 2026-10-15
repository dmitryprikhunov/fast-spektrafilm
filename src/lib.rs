pub mod base;
pub mod chemistry;
pub mod color;
pub mod convert;
mod convert_math;
mod data_path;
pub mod diffusion;
pub mod file_io;
pub mod grain;
#[cfg(feature = "gui")]
pub mod gui;
pub mod image;
pub mod interpolation;
pub mod lens;
pub mod metering;
pub mod params;
pub mod pipeline;
pub mod profile;
pub mod random;
pub mod raw;
mod scan_reference;
pub mod spatial;
pub mod spectral;

pub use data_path::default_data_dir;
pub use image::Image;
pub use params::Params;
pub use pipeline::{Simulator, Tap};
mod appearance;
mod density_lut;
mod framing;

pub mod metadata;

mod optimization;
pub mod preview;
pub mod print_balance;

pub mod numerical;

mod cache;

/// Compiler/profile metadata embedded in the measured binary, not inferred from
/// the environment of a later benchmark process. Cargo.toml records defaults.
pub fn build_info() -> serde_json::Value {
    serde_json::json!({
        "compiler": env!("SPEKTRAFILM_BUILD_COMPILER"),
        "profile": env!("SPEKTRAFILM_BUILD_PROFILE"), "opt_level": env!("SPEKTRAFILM_BUILD_OPT_LEVEL"),
        "debug": env!("SPEKTRAFILM_BUILD_DEBUG"), "target": env!("SPEKTRAFILM_BUILD_TARGET"),
        "host": env!("SPEKTRAFILM_BUILD_HOST"),
        "rustflags": env!("SPEKTRAFILM_BUILD_RUSTFLAGS"),
        "encoded_rustflags": env!("SPEKTRAFILM_BUILD_CARGO_ENCODED_RUSTFLAGS"),
        "release_lto_override": env!("SPEKTRAFILM_BUILD_CARGO_PROFILE_RELEASE_LTO"),
        "release_codegen_units_override": env!("SPEKTRAFILM_BUILD_CARGO_PROFILE_RELEASE_CODEGEN_UNITS"),
        "cargo_toml": include_str!("../Cargo.toml")
    })
}
