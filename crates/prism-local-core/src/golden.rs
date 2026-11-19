//! A small cross-platform foundation fixture, not an inference fixture.

use std::fmt::Write as _;

pub fn bytes() -> Vec<u8> {
    fixture().0
}

/// Fixture bytes and the actual maximum worker count exercised.
pub fn fixture() -> (Vec<u8>, usize) {
    let dot = crate::math::dot_i8(&[-128, 127, -128], &[-128, 127, 127]).unwrap();
    let digest = crate::sha256::digest(b"abc").unwrap();
    let mut text = format!("dot={dot}\nsha256=");
    for byte in digest {
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text.push('\n');
    let input = [-128_i8, 127, 17, -9, 0];
    let matrix = [
        -128, 127, -128, 127, 127, 0, 0, 0, 0, 0, 127, -128, 17, 9, -1, 1, 2, 3, 4, 5, -1, -2, -3,
        -4, -5, 4, 3, 2, 1, 0,
    ];
    let serial = crate::math::dot_rows_i8(&matrix, &input, 1).expect("one worker");
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(6);
    let parallel = crate::math::dot_rows_i8(&matrix, &input, workers).expect("bounded workers");
    assert_eq!(serial, parallel, "row dot differs across worker counts");
    for value in serial {
        writeln!(&mut text, "row={value}").unwrap();
    }
    let fixture = include_str!("../tests/fixtures/math-reference.csv");
    for line in fixture.lines().skip(1) {
        let mut fields = line.split(',');
        let name = fields.next().unwrap();
        let x: f32 = fields.next().unwrap().parse().unwrap();
        let value = match name {
            "exp" => crate::math::exp_f32(x),
            "erf" => crate::math::erf_f32(x),
            "tanh" => crate::math::tanh_f32(x),
            _ => unreachable!("known fixture function"),
        }
        .unwrap();
        writeln!(&mut text, "{:08x}", value.to_bits()).unwrap();
    }
    for x in [-f32::MIN_POSITIVE, -0.0, f32::MIN_POSITIVE] {
        let exp = crate::math::exp_f32(x).unwrap().to_bits();
        let erf = crate::math::erf_f32(x).unwrap().to_bits();
        let tanh = crate::math::tanh_f32(x).unwrap().to_bits();
        writeln!(&mut text, "{exp:08x},{erf:08x},{tanh:08x}").unwrap();
    }
    (text.into_bytes(), workers)
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_independent_values() {
        assert!(super::bytes().starts_with(
            b"dot=16257\nsha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\nrow=29194\nrow=0\nrow=-32304\nrow=141\nrow=-141\nrow=-106\n"
        ));
    }

    #[test]
    fn scalar_output_bits_match_exact_arithmetic_reference() {
        let output = String::from_utf8(super::bytes()).unwrap();
        let actual: Vec<_> = output.lines().skip(8).collect();
        let expected: Vec<_> = include_str!("../tests/fixtures/math-exact-bits.txt")
            .lines()
            .collect();
        assert_eq!(actual.len(), 1_267);
        assert_eq!(actual.len(), expected.len());
        for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert_eq!(*actual, expected, "reference row {}", index + 1);
        }
    }
}
