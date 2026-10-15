//! Allocation-capacity estimates only. No full-size image is allocated or rendered.
use fast_spektrafilm::{Params, Simulator, diffusion, params::DiffusionFilter};
use serde_json::json;
fn main() -> anyhow::Result<()> {
    let mut rows = Vec::new();
    for family in ["black_pro_mist", "pro_mist", "glimmerglass", "cinebloom"] {
        let p = DiffusionFilter {
            active: true,
            filter_family: family.into(),
            ..Default::default()
        };
        for size in [64, 128, 256] {
            for format in [35., 1.5] {
                rows.push(json!({"family":family,"width":size,"height":size,"film_format_mm":format,
                "pixel_um":format*1000./size as f64,"estimate":diffusion::workspace_estimate(size,size,&p,format*1000./size as f64)?}));
            }
        }
    }
    let p = DiffusionFilter {
        active: true,
        ..Default::default()
    };
    rows.push(json!({"family":"black_pro_mist","width":6000,"height":4000,"film_format_mm":35.,
        "pixel_um":35_000./6000.,"estimate":diffusion::workspace_estimate(6000,4000,&p,35_000./6000.)?}));
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    let sim = Simulator::new(&data, Params::default())?;
    println!(
        "{}",
        json!({"kind":"analytical capacity estimates, not process RSS or full-frame measurements",
        "diffusion":rows,"resolved_portra_400_grain":sim.params().grain,"build":fast_spektrafilm::build_info()})
    );
    Ok(())
}
