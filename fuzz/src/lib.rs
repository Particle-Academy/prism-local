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
        memory_plan::{checked_plan, Allocation, RequestBuffers},
        metadata::{self, BertSpec},
        model_checks::{self, TrustedSections},
        plm,
        provenance::{self, LicenseKind, ProvenanceSpec, Source},
        sha256,
        tensors::{self, Dtype, RowScale, TensorSpec, ValueRule},
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
        let bytes = include_bytes!("../corpus/frame_budgeted/valid");
        let (prefix, mut wire) = bytes.split_at(2);
        let budget = u64::from(u16::from_le_bytes([prefix[0], prefix[1]]));
        let plan = checked_plan(
            1,
            0,
            &[Allocation {
                count: budget,
                element_bytes: 1,
            }],
        )
        .unwrap();
        let mut buffers = RequestBuffers::new(plan);
        let frame =
            frame::read_frame_budgeted(&mut wire, frame::HARD_MAX_FRAME, || Ok(()), &mut buffers)
                .unwrap()
                .unwrap();
        assert_eq!(frame.as_slice(), json);
        let denied = include_bytes!("../corpus/frame_budgeted/denied");
        let (prefix, mut wire) = denied.split_at(2);
        let budget = u64::from(u16::from_le_bytes([prefix[0], prefix[1]]));
        let plan = checked_plan(
            1,
            0,
            &[Allocation {
                count: budget,
                element_bytes: 1,
            }],
        )
        .unwrap();
        let mut buffers = RequestBuffers::new(plan);
        assert!(frame::read_frame_budgeted(
            &mut wire,
            frame::HARD_MAX_FRAME,
            || Ok(()),
            &mut buffers
        )
        .is_err());
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
            license_kind: LicenseKind::Mit,
            license: "MIT\nCopyright Example\n",
            copyright: "Copyright Example",
            notice: "",
            sources: &sources,
        };
        assert!(
            provenance::check(include_bytes!("../corpus/provenance/valid"), &provenance).is_ok()
        );
        let apache = ProvenanceSpec {
            license_kind: LicenseKind::Apache2,
            license: "Synthetic Apache-2.0 license text\n",
            copyright: "",
            notice: "Required upstream notice\n",
            ..provenance
        };
        assert!(
            provenance::check(include_bytes!("../corpus/provenance/valid-apache"), &apache).is_ok()
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
            row_scales: &[],
            vocabulary: &vocab,
            provenance: &provenance,
        };
        let bytes = include_bytes!("../corpus/sections/valid");
        assert!(
            model_checks::check_sections(bytes, &sha256::digest(bytes).unwrap(), &spec).is_ok()
        );
        let row_specs = [
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
        let scales = [RowScale {
            weight_id: 1,
            scale_id: 2,
        }];
        let row_spec = TrustedSections {
            tensors: &row_specs,
            tensor_values: &row_rules,
            row_scales: &scales,
            ..spec
        };
        let bytes = include_bytes!("../corpus/quantized_sections/valid");
        assert!(
            model_checks::check_sections(bytes, &sha256::digest(bytes).unwrap(), &row_spec).is_ok()
        );
        let bytes = include_bytes!("../corpus/quantized_sections/nonzero-padding");
        assert!(
            model_checks::check_sections(bytes, &sha256::digest(bytes).unwrap(), &row_spec)
                .is_err()
        );
    }

    #[test]
    fn hostile_corpus_is_rejected() {
        for bytes in [
            include_bytes!("../corpus/plm/overflowing-count").as_slice(),
            include_bytes!("../corpus/plm/overflowing-offset").as_slice(),
            include_bytes!("../corpus/plm/overlapping-sections").as_slice(),
            include_bytes!("../corpus/plm/nonzero-padding").as_slice(),
        ] {
            assert!(plm::check_envelope(bytes, &sha256::digest(bytes).unwrap()).is_err());
        }
        let frame = include_bytes!("../corpus/frame/oversized");
        assert!(frame::read_frame(&mut frame.as_slice(), frame::HARD_MAX_FRAME).is_err());
        let specs = [
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
        let bytes = include_bytes!("../corpus/tensors/overflowing-dimension");
        let (directory, data) = bytes.split_at(80);
        assert!(tensors::check_values(
            directory,
            data,
            &specs,
            &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero]
        )
        .is_err());
    }

    #[test]
    fn converter_source_corpus_has_success_and_failure_seeds() {
        use prism_local_convert::safetensors::{inspect, Dtype, TensorSpec};
        let specs = [
            TensorSpec {
                name: "position",
                dtype: Dtype::I64,
                shape: &[2],
            },
            TensorSpec {
                name: "weight",
                dtype: Dtype::F32,
                shape: &[2],
            },
        ];
        let valid = include_bytes!("../corpus/safetensors/valid");
        assert!(inspect(valid, &sha256::digest(valid).unwrap(), &specs, Some("pt")).is_ok());
        let invalid = include_bytes!("../corpus/safetensors/overflowing-header");
        assert!(inspect(
            invalid,
            &sha256::digest(invalid).unwrap(),
            &specs,
            Some("pt")
        )
        .is_err());
    }
}
