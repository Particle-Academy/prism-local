//! Draft BERT architecture metadata checks. No model loading is enabled.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BertSpec {
    pub layers: u32,
    pub hidden: u32,
    pub heads: u32,
    pub intermediate: u32,
    pub vocab: u32,
    pub max_positions: u32,
    pub token_types: u32,
    pub layer_norm_epsilon_bits: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidMetadata;

/// Require an exact match to a trusted engine-defined schema and hard caps.
pub fn check_bert(bytes: &[u8], expected: BertSpec) -> Result<(), InvalidMetadata> {
    if bytes.len() != 64 || bytes[32..].iter().any(|&b| b != 0) {
        return Err(InvalidMetadata);
    }
    let chunks = bytes.as_chunks::<4>().0;
    let value = |index: usize| u32::from_le_bytes(chunks[index]);
    let actual = BertSpec {
        layers: value(0),
        hidden: value(1),
        heads: value(2),
        intermediate: value(3),
        vocab: value(4),
        max_positions: value(5),
        token_types: value(6),
        layer_norm_epsilon_bits: value(7),
    };
    let epsilon = f32::from_bits(actual.layer_norm_epsilon_bits);
    if !(1..=128).contains(&actual.layers)
        || !(1..=16_384).contains(&actual.hidden)
        || !(1..=256).contains(&actual.heads)
        || !actual.hidden.is_multiple_of(actual.heads)
        || !(1..=65_536).contains(&actual.intermediate)
        || !(5..=131_072).contains(&actual.vocab)
        || !(1..=131_072).contains(&actual.max_positions)
        || !(1..=1024).contains(&actual.token_types)
        || !epsilon.is_normal()
        || !(0.0..=1.0).contains(&epsilon)
        || actual != expected
    {
        return Err(InvalidMetadata);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{check_bert, BertSpec, InvalidMetadata};
    fn expected() -> BertSpec {
        BertSpec {
            layers: 12,
            hidden: 384,
            heads: 12,
            intermediate: 1536,
            vocab: 30522,
            max_positions: 512,
            token_types: 2,
            layer_norm_epsilon_bits: 1e-12_f32.to_bits(),
        }
    }
    fn bytes(spec: BertSpec) -> Vec<u8> {
        let mut out = Vec::new();
        for v in [
            spec.layers,
            spec.hidden,
            spec.heads,
            spec.intermediate,
            spec.vocab,
            spec.max_positions,
            spec.token_types,
            spec.layer_norm_epsilon_bits,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.resize(64, 0);
        out
    }
    #[test]
    fn requires_exact_schema_and_rejects_invalid_values() {
        let spec = expected();
        let valid = bytes(spec);
        assert_eq!(check_bert(&valid, spec), Ok(()));
        let mut b = valid.clone();
        b[63] = 1;
        assert_eq!(check_bert(&b, spec), Err(InvalidMetadata));
        assert_eq!(check_bert(&valid[..63], spec), Err(InvalidMetadata));
        let mut different = spec;
        different.layers = 13;
        assert_eq!(check_bert(&bytes(different), spec), Err(InvalidMetadata));
        for field in 0..8 {
            let mut b = valid.clone();
            b[field * 4..field * 4 + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert_eq!(check_bert(&b, spec), Err(InvalidMetadata));
        }
        let mut invalid = spec;
        invalid.heads = 13;
        assert_eq!(check_bert(&bytes(invalid), invalid), Err(InvalidMetadata));
        invalid = spec;
        invalid.layer_norm_epsilon_bits = f32::NAN.to_bits();
        assert_eq!(check_bert(&bytes(invalid), invalid), Err(InvalidMetadata));
    }

    #[test]
    fn mutation_smoke_never_panics() {
        let spec = expected();
        crate::test_support::mutate(&bytes(spec), |input| {
            let _ = check_bert(input, spec);
        });
    }
}
