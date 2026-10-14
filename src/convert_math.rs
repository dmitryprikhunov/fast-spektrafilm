//! Correctly-rounded binary32 log10 used by the pinned NumPy inverse seed.
//! CORE-MATH revision bc385c2, Copyright (c) 2022-2023 Alexei Sibidanov.
//! Rust translation; MIT notice in licenses/core-math-log10f.txt.
//! Older system log10f implementations shift fitted calibration parameters.
const TR: [f64; 65] = [
    f64::from_bits(0x3ff0000000000000),
    f64::from_bits(0x3fef81f820000000),
    f64::from_bits(0x3fef07c1f0000000),
    f64::from_bits(0x3fee9131ac000000),
    f64::from_bits(0x3fee1e1e1e000000),
    f64::from_bits(0x3fedae6077000000),
    f64::from_bits(0x3fed41d41d000000),
    f64::from_bits(0x3fecd85689000000),
    f64::from_bits(0x3fec71c71c000000),
    f64::from_bits(0x3fec0e0704000000),
    f64::from_bits(0x3febacf915000000),
    f64::from_bits(0x3feb4e81b5000000),
    f64::from_bits(0x3feaf286bd000000),
    f64::from_bits(0x3fea98ef60000000),
    f64::from_bits(0x3fea41a41a000000),
    f64::from_bits(0x3fe9ec8e95000000),
    f64::from_bits(0x3fe999999a000000),
    f64::from_bits(0x3fe948b0fd000000),
    f64::from_bits(0x3fe8f9c190000000),
    f64::from_bits(0x3fe8acb90f000000),
    f64::from_bits(0x3fe8618618000000),
    f64::from_bits(0x3fe8181818000000),
    f64::from_bits(0x3fe7d05f41000000),
    f64::from_bits(0x3fe78a4c81000000),
    f64::from_bits(0x3fe745d174000000),
    f64::from_bits(0x3fe702e05c000000),
    f64::from_bits(0x3fe6c16c17000000),
    f64::from_bits(0x3fe6816817000000),
    f64::from_bits(0x3fe642c859000000),
    f64::from_bits(0x3fe6058160000000),
    f64::from_bits(0x3fe5c9882c000000),
    f64::from_bits(0x3fe58ed231000000),
    f64::from_bits(0x3fe5555555000000),
    f64::from_bits(0x3fe51d07eb000000),
    f64::from_bits(0x3fe4e5e0a7000000),
    f64::from_bits(0x3fe4afd6a0000000),
    f64::from_bits(0x3fe47ae148000000),
    f64::from_bits(0x3fe446f865000000),
    f64::from_bits(0x3fe4141414000000),
    f64::from_bits(0x3fe3e22cbd000000),
    f64::from_bits(0x3fe3b13b14000000),
    f64::from_bits(0x3fe3813814000000),
    f64::from_bits(0x3fe3521cfb000000),
    f64::from_bits(0x3fe323e34a000000),
    f64::from_bits(0x3fe2f684be000000),
    f64::from_bits(0x3fe2c9fb4e000000),
    f64::from_bits(0x3fe29e412a000000),
    f64::from_bits(0x3fe27350b9000000),
    f64::from_bits(0x3fe2492492000000),
    f64::from_bits(0x3fe21fb781000000),
    f64::from_bits(0x3fe1f7047e000000),
    f64::from_bits(0x3fe1cf06ae000000),
    f64::from_bits(0x3fe1a7b961000000),
    f64::from_bits(0x3fe1811812000000),
    f64::from_bits(0x3fe15b1e5f000000),
    f64::from_bits(0x3fe135c811000000),
    f64::from_bits(0x3fe1111111000000),
    f64::from_bits(0x3fe0ecf56c000000),
    f64::from_bits(0x3fe0c97150000000),
    f64::from_bits(0x3fe0a6810a000000),
    f64::from_bits(0x3fe0842108000000),
    f64::from_bits(0x3fe0624dd3000000),
    f64::from_bits(0x3fe0410410000000),
    f64::from_bits(0x3fe0204081000000),
    f64::from_bits(0x3fe0000000000000),
];
const TL: [f64; 65] = [
    f64::from_bits(0xbd0d45fd6237ebe3),
    f64::from_bits(0x3f7b947689311b6e),
    f64::from_bits(0x3f8b5e909c96d7d5),
    f64::from_bits(0x3f945f4f59ed2165),
    f64::from_bits(0x3f9af5f92cbd8f1e),
    f64::from_bits(0x3fa0ba01a606de8c),
    f64::from_bits(0x3fa3ed119b9a2b7b),
    f64::from_bits(0x3fa714834298eec2),
    f64::from_bits(0x3faa30a9d98357fb),
    f64::from_bits(0x3fad41d512670813),
    f64::from_bits(0x3fb02428c0f65519),
    f64::from_bits(0x3fb1a23444eecc3e),
    f64::from_bits(0x3fb31b30543f4cb4),
    f64::from_bits(0x3fb48f3ed39bfd04),
    f64::from_bits(0x3fb5fe8049a0e423),
    f64::from_bits(0x3fb769140a6aa008),
    f64::from_bits(0x3fb8cf1836c98cb3),
    f64::from_bits(0x3fba30a9d55541a1),
    f64::from_bits(0x3fbb8de4d1ee823e),
    f64::from_bits(0x3fbce6e4202ca2e6),
    f64::from_bits(0x3fbe3bc1accace07),
    f64::from_bits(0x3fbf8c9683b5abd4),
    f64::from_bits(0x3fc06cbd68ca9a6e),
    f64::from_bits(0x3fc11142f19df730),
    f64::from_bits(0x3fc1b3e71fa7a97f),
    f64::from_bits(0x3fc254b4d37a46e3),
    f64::from_bits(0x3fc2f3b6912cbf07),
    f64::from_bits(0x3fc390f683115886),
    f64::from_bits(0x3fc42c7e7fffc5a8),
    f64::from_bits(0x3fc4c65808c78d3c),
    f64::from_bits(0x3fc55e8c50751c55),
    f64::from_bits(0x3fc5f52445dec3d8),
    f64::from_bits(0x3fc68a288c3f1200),
    f64::from_bits(0x3fc71da17bdf0d19),
    f64::from_bits(0x3fc7af973608afd9),
    f64::from_bits(0x3fc84011952a2579),
    f64::from_bits(0x3fc8cf1837a7ea60),
    f64::from_bits(0x3fc95cb2891e43d6),
    f64::from_bits(0x3fc9e8e7b0f869e0),
    f64::from_bits(0x3fca73beaa5db18d),
    f64::from_bits(0x3fcafd3e394558d3),
    f64::from_bits(0x3fcb856cf060d9f1),
    f64::from_bits(0x3fcc0c5134de1ffc),
    f64::from_bits(0x3fcc91f1371bc99f),
    f64::from_bits(0x3fcd1652ffcd3f53),
    f64::from_bits(0x3fcd997c6f635e75),
    f64::from_bits(0x3fce1b733ab90f3b),
    f64::from_bits(0x3fce9c3ceadac856),
    f64::from_bits(0x3fcf1bdeec43a305),
    f64::from_bits(0x3fcf9a5e7a5fa3fe),
    f64::from_bits(0x3fd00be05ac02f2b),
    f64::from_bits(0x3fd04a054d81a2d4),
    f64::from_bits(0x3fd087a0835957fb),
    f64::from_bits(0x3fd0c4b457099517),
    f64::from_bits(0x3fd101431aa1fe51),
    f64::from_bits(0x3fd13d4f08b98dd8),
    f64::from_bits(0x3fd178da53edb892),
    f64::from_bits(0x3fd1b3e71e9f9d58),
    f64::from_bits(0x3fd1ee777defdeed),
    f64::from_bits(0x3fd2288d7b48e23b),
    f64::from_bits(0x3fd2622b0f52e49f),
    f64::from_bits(0x3fd29b522a4c6314),
    f64::from_bits(0x3fd2d404b0e30f80),
    f64::from_bits(0x3fd30c4478f3fbe5),
    f64::from_bits(0x3fd34413509f7915),
];
const B: [f64; 3] = [
    f64::from_bits(0x3fdbcb7b15c5a2f8),
    f64::from_bits(0xbfcbcbb1dbb88eba),
    f64::from_bits(0x3fc2871c39d521c6),
];
const C: [f64; 7] = [
    f64::from_bits(0x3fdbcb7b1526e50e),
    f64::from_bits(0xbfcbcb7b1526e53d),
    f64::from_bits(0x3fc287a7636f3fa2),
    f64::from_bits(0xbfbbcb7b146a14b3),
    f64::from_bits(0x3fb63c627d5219cb),
    f64::from_bits(0xbfb2880736c8762d),
    f64::from_bits(0x3fafc1ecf913961a),
];
pub(crate) fn log10_f32(x: f32) -> f32 {
    let mut ux = x.to_bits();
    if !((1 << 23)..0x7f800000).contains(&ux) {
        if x == 0. {
            return f32::NEG_INFINITY;
        }
        if x.is_nan() || x == f32::INFINITY {
            return x;
        }
        if x < 0. {
            return f32::NAN;
        }
        let n = ux.leading_zeros() - 8;
        ux = (ux << n).wrapping_sub(n << 23);
    }
    let m = ux & ((1 << 23) - 1);
    let j = ((m + (1 << 16)) >> 17) as usize;
    let ix = TR[j];
    let l = TL[j];
    let e = ((ux as i32) >> 23) - 127;
    let je = ((e + 1) as u32).wrapping_mul(0x4d104d4) >> 28;
    const POWERS: [u32; 16] = [
        0x3f800000, 0x41200000, 0x42c80000, 0x447a0000, 0x461c4000, 0x47c35000, 0x49742400,
        0x4b189680, 0x4cbebc20, 0x4e6e6b28, 0x501502f9, 0, 0, 0, 0, 0,
    ];
    if ux == POWERS[je as usize] {
        return je as f32;
    }
    let tz = f64::from_bits(((m as u64) | (1023u64 << 23)) << 29);
    let z = tz * ix - 1.;
    let z2 = z * z;
    let mut r = ((e as f64 * LOG2_HI + l) + z * B[0]) + z2 * (B[1] + z * B[2]);
    let mut ub = r as f32;
    let lb = (r + BOUND) as f32;
    if ub != lb {
        let mut f =
            z * ((C[0] + z * C[1]) + z2 * ((C[2] + z * C[3]) + z2 * (C[4] + z * C[5] + z2 * C[6])));
        f -= LOG2_LO * e as f64;
        f += l - TL[0];
        let el = e as f64 * LOG2_ROUNDED;
        r = el + f;
        ub = r as f32;
        if r.to_bits() & ((1 << 28) - 1) == 0 {
            let dr = (el - r) + f;
            r += dr * 32.;
            ub = r as f32;
        }
    }
    ub
}
const LOG2_HI: f64 = f64::from_bits(0x3fd34413509f79ff);
const LOG2_LO: f64 = f64::from_bits(0x3c90cee0ed4ca7e9);
const LOG2_ROUNDED: f64 = f64::from_bits(0x3fd34413509f7a00);
const BOUND: f64 = f64::from_bits(0x3ddb008000000000);

#[cfg(test)]
mod tests {
    #[test]
    fn seed_logarithm_matches_pinned_numpy() {
        let cases: Vec<[u32; 2]> =
            serde_json::from_str(include_str!("../tests/fixtures/debt/log10_f32.json")).unwrap();
        for [input, expected] in cases {
            assert_eq!(
                super::log10_f32(f32::from_bits(input)).to_bits(),
                expected,
                "input={input:08x}"
            );
        }
        assert_eq!(super::log10_f32(f32::INFINITY), f32::INFINITY);
        assert_eq!(super::log10_f32(0.), f32::NEG_INFINITY);
        assert!(super::log10_f32(-1.).is_nan());
    }
}
