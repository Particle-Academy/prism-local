//! Bounded stdin adapter for external fuzzing tools.
use std::io::{self, Read};

const MAX_INPUT: u64 = 1_048_576;

pub fn input() -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Ok(None);
    }
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use prism_local_core::{
        frame, json,
        metadata::{self, BertSpec},
        model_checks::{self, TrustedSections},
        plm,
        provenance::{self, ProvenanceSpec, Source},
        sha256,
        tensors::{self, Dtype, TensorSpec, ValueRule},
        vocab::{self, VocabSpec},
    };

    #[test]
    fn synthetic_corpus_reaches_valid_parser_paths() {
        let json = include_bytes!("../corpus/json/valid");
        assert!(json::parse(json).is_ok());
        let frame = include_bytes!("../corpus/frame/valid");
        assert_eq!(
            frame::read_frame(&mut frame.as_slice(), frame::HARD_MAX_FRAME)
                .unwrap()
                .unwrap(),
            json
        );
        let meta = BertSpec {
            layers: 12,
            hidden: 384,
            heads: 12,
            intermediate: 1536,
            vocab: 30522,
            max_positions: 512,
            token_types: 2,
            layer_norm_epsilon_bits: 1e-12_f32.to_bits(),
        };
        assert!(metadata::check_bert(include_bytes!("../corpus/metadata/valid"), meta).is_ok());
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
        assert!(vocab::check(include_bytes!("../corpus/vocab/valid"), &vocab).is_ok());
        let sources = [Source {
            name: "weights",
            sha256: [7; 32],
        }];
        let provenance = ProvenanceSpec {
            repository: "p/m",
            revision: "abc",
            publisher: "p",
            converter: "v1",
            license: "MIT\nCopyright Example\n",
            copyright: "Copyright Example",
            sources: &sources,
        };
        assert!(
            provenance::check(include_bytes!("../corpus/provenance/valid"), &provenance).is_ok()
        );
        let tensor_specs = [
            TensorSpec {
                id: 1,
                dtype: Dtype::I8,
                dims: [2, 4, 0, 0],
            },
            TensorSpec {
                id: 2,
                dtype: Dtype::F32,
                dims: [4, 0, 0, 0],
            },
        ];
        let tensor_input = include_bytes!("../corpus/tensors/valid");
        let (directory, data) = tensor_input.split_at(80);
        assert!(tensors::check_values(
            directory,
            data,
            &tensor_specs,
            &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero]
        )
        .is_ok());
        let bytes = include_bytes!("../corpus/plm/valid");
        assert!(plm::check_envelope(bytes, &sha256::digest(bytes).unwrap()).is_ok());
        let small_meta = BertSpec {
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
        let spec = TrustedSections {
            metadata: small_meta,
            tensors: &tensor,
            tensor_values: &rules,
            vocabulary: &vocab,
            provenance: &provenance,
        };
        let bytes = include_bytes!("../corpus/sections/valid");
        assert!(
            model_checks::check_sections(bytes, &sha256::digest(bytes).unwrap(), &spec).is_ok()
        );
    }
}
