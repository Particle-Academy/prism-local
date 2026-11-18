use prism_local_core::{
    metadata::BertSpec,
    model_checks::{check_sections, TrustedSections},
    provenance::{ProvenanceSpec, Source},
    sha256,
    tensors::{Dtype, RowScale, TensorSpec, ValueRule},
    vocab::VocabSpec,
};

fn main() -> std::io::Result<()> {
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
    let tensors = [
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
    let rules = [ValueRule::AnyI8, ValueRule::PositiveNormal];
    let scales = [RowScale {
        weight_id: 1,
        scale_id: 2,
    }];
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
    let provenance = ProvenanceSpec {
        repository: "p/m",
        revision: "abc",
        publisher: "p",
        converter: "v1",
        license: "MIT\nCopyright Example\n",
        copyright: "Copyright Example",
        sources: &sources,
    };
    let spec = TrustedSections {
        metadata: meta,
        tensors: &tensors,
        tensor_values: &rules,
        row_scales: &scales,
        vocabulary: &vocab,
        provenance: &provenance,
    };
    if let Some(bytes) = prism_local_fuzz::input()? {
        if let Some(digest) = sha256::digest(&bytes) {
            let _ = check_sections(&bytes, &digest, &spec);
        }
    }
    Ok(())
}
