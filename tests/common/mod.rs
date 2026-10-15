use fast_spektrafilm::{
    Image,
    numerical::{self, Contract},
};
pub fn compare(a: &Image, b: &Image, atol: f64, label: &str) {
    check(a, b, &Contract { atol, rtol: 0. }, label);
}
pub fn strict(a: &Image, b: &Image, label: &str) {
    check(a, b, numerical::contract("strict"), label);
}
fn check(a: &Image, b: &Image, contract: &Contract, label: &str) {
    let report = numerical::compare(a, b, contract).unwrap();
    eprintln!("{label}: {}", serde_json::to_string(&report).unwrap());
    assert_eq!(report.failures, 0, "{label}: {report:?}");
}
