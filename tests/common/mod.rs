use fast_spektrafilm::Image;
pub fn compare(a: &Image, b: &Image, atol: f64, label: &str) {
    a.validate().unwrap();
    b.validate().unwrap();
    assert_eq!(
        (a.height, a.width, a.channels),
        (b.height, b.width, b.channels),
        "{label}: shape"
    );
    let max_abs = a
        .data
        .iter()
        .zip(&b.data)
        .map(|(a, b)| (a - b).abs())
        .fold(0., f64::max);
    let rmse = (a
        .data
        .iter()
        .zip(&b.data)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        / a.data.len() as f64)
        .sqrt();
    eprintln!("{label}: max_abs={max_abs:e}, rmse={rmse:e}");
    assert!(max_abs <= atol, "{label}: {max_abs:e} > {atol:e}");
}
