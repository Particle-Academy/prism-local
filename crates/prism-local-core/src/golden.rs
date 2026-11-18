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
    text.into_bytes()
}

#[cfg(test)]
mod tests {
    #[test]
    fn matches_independent_values() {
        assert_eq!(
            super::bytes(),
            b"dot=16257\nsha256=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\n"
        );
    }
}
