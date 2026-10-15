//! Diagnostic only: compare batch and scalar inverse evaluation on the same grid.
//! Run with a reference-generated NPY grid, then compare with Python float64 output.
use fast_spektrafilm::{Image, Params, Simulator, color::Tables, convert::FilmConverter};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 3, "review_inverse GRID.npy OUTPUT_DIRECTORY");
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let mut p = Params::default();
    p.camera.auto_exposure = false;
    p.debug.deactivate_spatial_effects = true;
    p.debug.deactivate_stochastic_effects = true;
    p.workflow.route = "input > convert-film > scan".into();
    let sim = Simulator::new(&data, p.clone())?;
    let converter = FilmConverter::new(&Tables::load(&data)?, sim.film(), &p)?;
    let input = Image::read_npy(Path::new(&args[1]))?;
    let folder = Path::new(&args[2]);
    std::fs::create_dir_all(folder)?;
    converter
        .convert(&input)
        .write_npy(&folder.join("batch.npy"))?;
    let mut scalar = input.clone();
    for p in scalar.data.chunks_exact_mut(3) {
        p.copy_from_slice(&converter.invert([p[0], p[1], p[2]]));
    }
    scalar.write_npy(&folder.join("scalar.npy"))?;
    let (lo, hi) = converter.gamut_bounds();
    println!("{}", serde_json::json!({"lo": lo, "hi": hi}));
    Ok(())
}
