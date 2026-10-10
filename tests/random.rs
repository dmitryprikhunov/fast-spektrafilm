use fast_spektrafilm::random::RandomState;
use serde::Deserialize;
#[derive(Deserialize)]
struct Vector {
    seed: u32,
    method: String,
    args: Vec<Vec<f64>>,
    values: Vec<f64>,
}
#[test]
fn numpy_randomstate_streams() -> anyhow::Result<()> {
    let vectors: Vec<Vector> = serde_json::from_str(include_str!("fixtures/random.json"))?;
    for vector in vectors {
        let mut rng = RandomState::new(vector.seed);
        for (i, (args, expected)) in vector.args.iter().zip(vector.values).enumerate() {
            let actual = match vector.method.as_str() {
                "uniform" => rng.uniform(),
                "normal" => rng.normal(),
                "poisson" => rng.poisson(args[0]) as f64,
                "binomial" => rng.binomial(args[0] as i64, args[1]) as f64,
                "fast_poisson" => rng.fast_poisson(args[0]) as f64,
                "fast_binomial" => rng.fast_binomial(args[0] as i64, args[1]) as f64,
                _ => unreachable!(),
            };
            assert!(
                (actual - expected).abs() <= 1e-14,
                "seed {}, {}[{i}]: {actual} vs {expected}",
                vector.seed,
                vector.method
            );
        }
    }
    Ok(())
}
