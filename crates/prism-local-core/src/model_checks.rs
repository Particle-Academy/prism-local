//! Compose draft section checks without declaring a model loadable.

use crate::{metadata, plm, provenance, tensors, vocab};

pub struct TrustedSections<'a> {
    pub metadata: metadata::BertSpec,
    pub tensors: &'a [tensors::TensorSpec],
    pub tensor_values: &'a [tensors::ValueRule],
    pub row_scales: &'a [tensors::RowScale],
    pub vocabulary: &'a vocab::VocabSpec<'a>,
    pub provenance: &'a provenance::ProvenanceSpec<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Envelope(plm::Error),
    InconsistentSchema,
    Metadata,
    Tensors,
    Vocabulary,
    Provenance,
}

/// This marker explicitly is not authorization to load or use the model.
#[derive(Debug, PartialEq, Eq)]
pub struct IncompleteChecks;

/// Hash once, then check known sections from that exact immutable buffer.
/// Only declared scale relationships are checked; complete quantization and
/// architecture semantics remain unvalidated. No inference path may accept
/// this result as a valid model.
pub fn check_sections(
    bytes: &[u8],
    digest: &[u8; 32],
    spec: &TrustedSections<'_>,
) -> Result<IncompleteChecks, Error> {
    if spec.metadata.vocab != spec.vocabulary.count
        || spec.tensors.len() != spec.tensor_values.len()
        || tensors::check_row_scales(spec.tensors, spec.tensor_values, spec.row_scales).is_err()
    {
        return Err(Error::InconsistentSchema);
    }
    let envelope = plm::check_envelope(bytes, digest).map_err(Error::Envelope)?;
    metadata::check_bert(envelope.section(plm::Section::Metadata), spec.metadata)
        .map_err(|_| Error::Metadata)?;
    tensors::check_values(
        envelope.section(plm::Section::TensorDirectory),
        envelope.section(plm::Section::TensorData),
        spec.tensors,
        spec.tensor_values,
    )
    .map_err(|_| Error::Tensors)?;
    vocab::check(envelope.section(plm::Section::Vocabulary), spec.vocabulary)
        .map_err(|_| Error::Vocabulary)?;
    provenance::check(envelope.section(plm::Section::Provenance), spec.provenance)
        .map_err(|_| Error::Provenance)?;
    Ok(IncompleteChecks)
}

#[cfg(test)]
mod tests {
    use super::{check_sections, Error, IncompleteChecks, TrustedSections};
    use crate::{
        metadata::BertSpec,
        plm,
        provenance::{LicenseKind, ProvenanceSpec, Source},
        sha256,
        tensors::{Dtype, RowScale, TensorSpec, ValueRule},
        vocab::VocabSpec,
    };

    fn append_text(target: &mut Vec<u8>, value: &str) {
        target.extend_from_slice(&(value.len() as u16).to_le_bytes());
        target.extend_from_slice(value.as_bytes());
    }
    fn metadata(spec: BertSpec) -> Vec<u8> {
        let mut b = Vec::new();
        for value in [
            spec.layers,
            spec.hidden,
            spec.heads,
            spec.intermediate,
            spec.vocab,
            spec.max_positions,
            spec.token_types,
            spec.layer_norm_epsilon_bits,
        ] {
            b.extend_from_slice(&value.to_le_bytes());
        }
        b.resize(64, 0);
        b
    }
    fn tensor_directory() -> Vec<u8> {
        let mut b = vec![0_u8; 40];
        b[0..4].copy_from_slice(&1_u32.to_le_bytes());
        b[4] = 3;
        b[5] = 1;
        b[8..12].copy_from_slice(&1_u32.to_le_bytes());
        b[32..40].copy_from_slice(&4_u64.to_le_bytes());
        b
    }
    fn vocabulary() -> Vec<u8> {
        let mut b = vec![0_u8; 32];
        b[0..4].copy_from_slice(&5_u32.to_le_bytes());
        for id in 0..5_u32 {
            b[8 + 4 * id as usize..12 + 4 * id as usize].copy_from_slice(&id.to_le_bytes());
        }
        for value in ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "[MASK]"] {
            append_text(&mut b, value);
        }
        b
    }
    fn provenance() -> Vec<u8> {
        let mut b = 1_u32.to_le_bytes().to_vec();
        for value in [
            "p/m",
            "abc",
            "p",
            "v1",
            "mit",
            "MIT\nCopyright Example\n",
            "Copyright Example",
            "",
            "weights",
        ] {
            append_text(&mut b, value);
        }
        b.extend_from_slice(&[7; 32]);
        b
    }
    fn envelope(parts: &[Vec<u8>; 5]) -> Vec<u8> {
        let mut b = vec![0_u8; 224];
        b[..4].copy_from_slice(b"PLM1");
        b[4..6].copy_from_slice(&1_u16.to_le_bytes());
        b[6..8].copy_from_slice(&1_u16.to_le_bytes());
        b[16..24].copy_from_slice(&64_u64.to_le_bytes());
        b[24..28].copy_from_slice(&5_u32.to_le_bytes());
        for (i, part) in parts.iter().enumerate() {
            let aligned = (b.len() + 63) & !63;
            b.resize(aligned, 0);
            let at = 64 + i * 32;
            b[at..at + 4].copy_from_slice(&((i + 1) as u32).to_le_bytes());
            b[at + 8..at + 16].copy_from_slice(&(aligned as u64).to_le_bytes());
            b[at + 16..at + 24].copy_from_slice(&(part.len() as u64).to_le_bytes());
            b.extend_from_slice(part);
        }
        let len = b.len() as u64;
        b[8..16].copy_from_slice(&len.to_le_bytes());
        b
    }
    #[test]
    fn links_sections_to_the_same_authenticated_bytes_and_rejects_mutations() {
        let meta = BertSpec {
            layers: 1,
            hidden: 2,
            heads: 1,
            intermediate: 4,
            vocab: 5,
            max_positions: 4,
            token_types: 1,
            layer_norm_epsilon_bits: 1e-5_f32.to_bits(),
        };
        let tensor = [TensorSpec {
            id: 1,
            dtype: Dtype::F32,
            dims: [1, 0, 0, 0],
        }];
        let rules = [ValueRule::PositiveNormal];
        let vocab = VocabSpec {
            count: 5,
            flags: 0,
            special: [
                (0, "[PAD]"),
                (1, "[UNK]"),
                (2, "[CLS]"),
                (3, "[SEP]"),
                (4, "[MASK]"),
            ],
        };
        let sources = [Source {
            name: "weights",
            sha256: [7; 32],
        }];
        let provenance_spec = ProvenanceSpec {
            repository: "p/m",
            revision: "abc",
            publisher: "p",
            converter: "v1",
            license_kind: LicenseKind::Mit,
            license: "MIT\nCopyright Example\n",
            copyright: "Copyright Example",
            notice: "",
            sources: &sources,
        };
        let spec = TrustedSections {
            metadata: meta,
            tensors: &tensor,
            tensor_values: &rules,
            row_scales: &[],
            vocabulary: &vocab,
            provenance: &provenance_spec,
        };
        let parts = [
            metadata(meta),
            tensor_directory(),
            vocabulary(),
            provenance(),
            1.0_f32.to_bits().to_le_bytes().to_vec(),
        ];
        let b = envelope(&parts);
        let digest = sha256::digest(&b).unwrap();
        assert_eq!(check_sections(&b, &digest, &spec), Ok(IncompleteChecks));
        let row_tensors = [
            TensorSpec {
                id: 1,
                dtype: Dtype::I8,
                dims: [2, 4, 0, 0],
            },
            TensorSpec {
                id: 2,
                dtype: Dtype::F32,
                dims: [2, 0, 0, 0],
            },
        ];
        let row_rules = [ValueRule::AnyI8, ValueRule::PositiveNormal];
        let row_scales = [RowScale {
            weight_id: 1,
            scale_id: 2,
        }];
        let row_spec = TrustedSections {
            tensors: &row_tensors,
            tensor_values: &row_rules,
            row_scales: &row_scales,
            ..spec
        };
        let mut row_parts = parts.clone();
        let mut directory = vec![0_u8; 80];
        directory[0..4].copy_from_slice(&1_u32.to_le_bytes());
        directory[4] = 1;
        directory[5] = 2;
        directory[8..12].copy_from_slice(&2_u32.to_le_bytes());
        directory[12..16].copy_from_slice(&4_u32.to_le_bytes());
        directory[32..40].copy_from_slice(&8_u64.to_le_bytes());
        directory[40..44].copy_from_slice(&2_u32.to_le_bytes());
        directory[44] = 3;
        directory[45] = 1;
        directory[48..52].copy_from_slice(&2_u32.to_le_bytes());
        directory[64..72].copy_from_slice(&64_u64.to_le_bytes());
        directory[72..80].copy_from_slice(&8_u64.to_le_bytes());
        row_parts[1] = directory;
        row_parts[4] = vec![0; 72];
        row_parts[4][64..68].copy_from_slice(&1_f32.to_bits().to_le_bytes());
        row_parts[4][68..72].copy_from_slice(&2_f32.to_bits().to_le_bytes());
        let row_bytes = envelope(&row_parts);
        assert_eq!(
            check_sections(&row_bytes, &sha256::digest(&row_bytes).unwrap(), &row_spec),
            Ok(IncompleteChecks)
        );
        row_parts[4][64..68].copy_from_slice(&(-1_f32).to_bits().to_le_bytes());
        let bad_rows = envelope(&row_parts);
        assert_eq!(
            check_sections(&bad_rows, &sha256::digest(&bad_rows).unwrap(), &row_spec),
            Err(Error::Tensors)
        );
        let mut altered = b.clone();
        let last = altered.len() - 1;
        altered[last] ^= 1;
        assert_eq!(
            check_sections(&altered, &digest, &spec),
            Err(Error::Envelope(plm::Error::DigestMismatch))
        );
        let mut bad_meta = parts.clone();
        bad_meta[0][0] = 0;
        let mut bad_tensor = parts.clone();
        bad_tensor[4] = f32::NAN.to_bits().to_le_bytes().to_vec();
        let mut bad_vocab = parts.clone();
        bad_vocab[2][34] = 0xff;
        let mut bad_provenance = parts.clone();
        bad_provenance[3][6] ^= 1;
        for (parts, expected) in [
            (bad_meta, Error::Metadata),
            (bad_tensor, Error::Tensors),
            (bad_vocab, Error::Vocabulary),
            (bad_provenance, Error::Provenance),
        ] {
            let mutated = envelope(&parts);
            let hash = sha256::digest(&mutated).unwrap();
            assert_eq!(check_sections(&mutated, &hash, &spec), Err(expected));
        }
        crate::test_support::mutate(&b, |input| {
            let hash = sha256::digest(input).unwrap();
            let _ = check_sections(input, &hash, &spec);
        });
        let mut wrong = meta;
        wrong.vocab = 6;
        let inconsistent = TrustedSections {
            metadata: wrong,
            ..spec
        };
        assert_eq!(
            check_sections(&b, &digest, &inconsistent),
            Err(Error::InconsistentSchema)
        );
    }
}
