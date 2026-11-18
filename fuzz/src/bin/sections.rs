use prism_local_core::{
    metadata::BertSpec,
    model_checks::{check_sections_with_scratch, TrustedSections},
    provenance::{LicenseKind, ProvenanceSpec, Source},
    sha256,
    tensors::{Dtype, TensorSpec, ValueRule},
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
    let tensor = [TensorSpec {
        id: 1,
        dtype: Dtype::F32,
        dims: [1, 0, 0, 0],
    }];
    let rules = [ValueRule::PositiveNormal];
    let vocabulary = VocabSpec {
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
        vocabulary: &vocabulary,
        provenance: &provenance,
    };
    if let Some(bytes) = prism_local_fuzz::input()? {
        if let Some(digest) = sha256::digest(&bytes) {
            let _ = check_sections_with_scratch(&bytes, &digest, &spec, &mut [0; 2], &mut [0; 5]);
        }
    }
    Ok(())
}
