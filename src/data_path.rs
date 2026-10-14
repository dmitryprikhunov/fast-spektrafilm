use std::path::PathBuf;

/// Prefer the measurement data shipped beside an installed executable.
/// Cargo builds also support the original source checkout without configuration.
pub fn default_data_dir() -> PathBuf {
    let packaged = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("data")));
    if let Some(data) = &packaged
        && data.join("reference_tables.json").is_file()
    {
        return data.clone();
    }
    let source = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/data"));
    if source.join("reference_tables.json").is_file() {
        return source;
    }
    packaged.unwrap_or_else(|| PathBuf::from("data"))
}
