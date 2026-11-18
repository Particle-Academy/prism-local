//! A small cross-platform foundation fixture, not an inference fixture.

use std::fmt::Write as _;

pub fn bytes() -> Vec<u8> {
    let dot = crate::math::dot_i8(&[-128, 127, -128], &[-128, 127, 127]).unwrap();
    let digest = crate::sha256::digest(b"abc").unwrap();
    let mut text = format!("dot={dot}\nsha256=");
    for byte in digest {
        write!(&mut text, "{byte:02x}").unwrap();
    }
    text.push('\n');
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
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_independent_values() {
        assert!(super::bytes().starts_with(
            b"dot=16257\nsha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        ));
    }
}
