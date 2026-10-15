use std::path::Path;

fn sources(dir: &Path, build: &mut cc::Build) {
    let mut entries = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            sources(&path, build);
        } else if path.extension().and_then(|s| s.to_str()) == Some("cpp") {
            build.file(path);
        }
    }
}
fn main() {
    for key in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_PROFILE_RELEASE_LTO",
        "CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        println!(
            "cargo:rustc-env=SPEKTRAFILM_BUILD_{key}={}",
            std::env::var(key).unwrap_or_default()
        );
    }
    for key in ["PROFILE", "OPT_LEVEL", "DEBUG", "TARGET", "HOST"] {
        println!(
            "cargo:rustc-env=SPEKTRAFILM_BUILD_{key}={}",
            std::env::var(key).unwrap_or_default()
        );
    }
    let compiler = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("--version")
        .output()
        .unwrap();
    println!(
        "cargo:rustc-env=SPEKTRAFILM_BUILD_COMPILER={}",
        String::from_utf8_lossy(&compiler.stdout).trim()
    );
    println!("cargo:rerun-if-changed=ui/darkroom.slint");
    #[cfg(feature = "gui")]
    slint_build::compile_with_config(
        "ui/darkroom.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )
    .expect("Compile Slint darkroom UI");
    let mut adapter = cc::Build::new();
    adapter.file("native/raw.c");
    if std::env::var_os("CARGO_FEATURE_BUNDLED_LIBRAW").is_some() {
        // Original native RAW decoder, pinned to the reference rawpy build.
        // All new film-processing algorithms are in Rust.
        adapter.include("vendor/libraw");
        adapter.compile("spektrafilm_raw_ffi");
        let mut raw = cc::Build::new();
        raw.cpp(true)
            .include("vendor/libraw")
            .warnings(false)
            .flag_if_supported("-fopenmp");
        sources(Path::new("vendor/libraw/src"), &mut raw);
        raw.compile("spektrafilm_libraw");
        if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
            println!("cargo:rustc-link-lib=gomp");
        }
        println!("cargo:rerun-if-changed=vendor/libraw");
    } else {
        let lib = pkg_config::Config::new()
            .probe("libraw_r")
            .expect("Install libraw-dev and pkg-config");
        for path in lib.include_paths {
            adapter.include(path);
        }
        adapter.compile("spektrafilm_raw_ffi");
    }
    println!("cargo:rerun-if-changed=native/raw.c");
}
