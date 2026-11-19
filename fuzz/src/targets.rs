//! Fuzz target bodies shared by the stdin binaries and the coverage-guided
//! engine in `fuzz/engine`. Keeping them in this library means generic and
//! inline parser code is instantiated here, where the fuzz build instruments it.
use prism_local_convert::safetensors as st;
use prism_local_core::{
    frame::{read_frame, read_frame_budgeted, HARD_MAX_FRAME},
    json::{self, decode_string_into, string_output_len, write_string, Value},
    json_arena::{self, Arena, Error as ArenaError, Kind},
    memory_plan::{checked_plan, Allocation, RequestBuffers},
    metadata::{check_bert, BertSpec},
    model_checks::{check_sections_budgeted, scratch_requirements, TrustedSections},
    plm,
    provenance::{self, LicenseKind, ProvenanceSpec, Source},
    sha256,
    tensors::{self, Dtype, RowScale, TensorSpec, ValueRule},
    vocab::{self, VocabSpec},
};

pub fn frame(b: &[u8]) {
    let _ = read_frame(&mut &b[..], HARD_MAX_FRAME);
}

pub fn frame_budgeted(b: &[u8]) {
    if let Some((prefix, wire)) = b.split_at_checked(2) {
        let budget = u64::from(u16::from_le_bytes([prefix[0], prefix[1]]));
        if let Ok(plan) = checked_plan(
            1,
            0,
            &[Allocation {
                count: budget,
                element_bytes: 1,
            }],
        ) {
            let mut buffers = RequestBuffers::new(plan);
            let mut input = wire;
            let _ = read_frame_budgeted(&mut input, HARD_MAX_FRAME, || Ok(()), &mut buffers);
        }
    }
}

pub fn json(b: &[u8]) {
    let _ = json::parse(b);
}

fn same(arena: &Arena<'_>, index: usize, expected: &Value) -> bool {
    match expected {
        Value::Null => arena.kind(index) == Some(Kind::Null),
        Value::Bool(v) => arena.kind(index) == Some(Kind::Bool(*v)),
        Value::Number(v) => arena.number(index) == Some(v.as_str()),
        Value::String(v) => arena.string(index) == Some(v.as_str()),
        Value::Array(values) => {
            if arena.kind(index) != Some(Kind::Array) {
                return false;
            }
            let mut child = arena.first_child(index);
            for v in values {
                let Some(c) = child else { return false };
                if !same(arena, c, v) {
                    return false;
                }
                child = arena.next_sibling(c);
            }
            child.is_none()
        }
        Value::Object(values) => {
            if arena.kind(index) != Some(Kind::Object) {
                return false;
            }
            let mut child = arena.first_child(index);
            for (k, v) in values {
                let Some(c) = child else { return false };
                if arena.key(c) != Some(k.as_str()) || !same(arena, c, v) {
                    return false;
                }
                child = arena.next_sibling(c);
            }
            child.is_none()
        }
    }
}

pub fn json_arena(b: &[u8]) {
    let plan = checked_plan(
        4,
        0,
        &[Allocation {
            count: 4 * 1024 * 1024,
            element_bytes: 1,
        }],
    )
    .unwrap();
    let mut buffers = RequestBuffers::new(plan);
    match (json::parse(b), json_arena::parse(b, &mut buffers)) {
        (Ok(expected), Ok(arena)) => assert!(same(&arena, arena.root(), &expected)),
        (Err(_), Err(ArenaError::Invalid)) | (_, Err(ArenaError::Resource)) => {}
        _ => panic!("JSON parsers disagree"),
    }
}

pub fn json_decode(b: &[u8]) {
    if let Some((prefix, token)) = b.split_at_checked(2) {
        let budget = u64::from(u16::from_le_bytes([prefix[0], prefix[1]]));
        let plan = checked_plan(
            1,
            0,
            &[Allocation {
                count: 65_536,
                element_bytes: 1,
            }],
        )
        .unwrap();
        let mut buffers = RequestBuffers::new(plan);
        if let Ok(mut output) = buffers.allocate::<u8>(budget) {
            if let Ok(written) = decode_string_into(token, output.as_mut_slice()) {
                assert!(matches!(json::parse(token),
                    Ok(Value::String(d)) if d.as_bytes() == &output.as_slice()[..written]));
            }
        }
    }
}

pub fn json_write(b: &[u8]) {
    if b.len() > 65_536 {
        return;
    }
    if let Ok(value) = std::str::from_utf8(b) {
        if let Some(length) = string_output_len(value) {
            let plan = checked_plan(
                1,
                0,
                &[Allocation {
                    count: 1_048_576,
                    element_bytes: 1,
                }],
            )
            .unwrap();
            let mut buffers = RequestBuffers::new(plan);
            if let Ok(mut output) = buffers.allocate::<u8>(length as u64) {
                let written = write_string(value, output.as_mut_slice()).expect("sized output");
                assert!(matches!(json::parse(&output.as_slice()[..written]),
                    Ok(Value::String(d)) if d == value));
            }
        }
    }
}

const BGE: BertSpec = BertSpec {
    layers: 12,
    hidden: 384,
    heads: 12,
    intermediate: 1536,
    vocab: 30522,
    max_positions: 512,
    token_types: 2,
    layer_norm_epsilon_bits: 1e-12_f32.to_bits(),
};

pub fn metadata(b: &[u8]) {
    let _ = check_bert(b, BGE);
}

pub fn plm(b: &[u8]) {
    if let Some(d) = sha256::digest(b) {
        let _ = plm::check_envelope(b, &d);
        let _ = plm::check_envelope_for_arch(b, &d, 42);
    }
}

const SOURCES: [Source; 1] = [Source {
    name: "weights",
    sha256: [7; 32],
}];

fn mit() -> ProvenanceSpec<'static> {
    ProvenanceSpec {
        repository: "p/m",
        revision: "abc",
        publisher: "p",
        converter: "v1",
        license_kind: LicenseKind::Mit,
        license: "MIT\nCopyright Example\n",
        copyright: "Copyright Example",
        notice: "",
        sources: &SOURCES,
    }
}

pub fn provenance(b: &[u8]) {
    let apache = ProvenanceSpec {
        repository: "p/m",
        revision: "abc",
        publisher: "p",
        converter: "v1",
        license_kind: LicenseKind::Apache2,
        license: "Synthetic Apache-2.0 license text\n",
        copyright: "",
        notice: "Required upstream notice\n",
        sources: &SOURCES,
    };
    let _ = provenance::check(b, &mit());
    let _ = provenance::check(b, &apache);
}

fn small_meta() -> BertSpec {
    BertSpec {
        layers: 1,
        hidden: 2,
        heads: 1,
        intermediate: 4,
        vocab: 5,
        max_positions: 4,
        token_types: 1,
        layer_norm_epsilon_bits: 1e-5_f32.to_bits(),
    }
}

fn small_vocab() -> VocabSpec<'static> {
    VocabSpec {
        count: 5,
        flags: 0,
        special: [
            (0, "[PAD]"),
            (1, "[UNK]"),
            (2, "[CLS]"),
            (3, "[SEP]"),
            (4, "[MASK]"),
        ],
    }
}

fn run_sections(b: &[u8], spec: &TrustedSections<'_>) {
    if let Some(d) = sha256::digest(b) {
        let required = scratch_requirements(spec).expect("trusted fuzz schema");
        let plan = checked_plan(2, b.len() as u64, &required).expect("bounded fuzz input");
        let _ = check_sections_budgeted(b, &d, spec, &mut RequestBuffers::new(plan));
    }
}

pub fn sections(b: &[u8]) {
    let tensor = [TensorSpec {
        id: 1,
        dtype: Dtype::F32,
        dims: [1, 0, 0, 0],
    }];
    let rules = [ValueRule::PositiveNormal];
    let vocabulary = small_vocab();
    let prov = mit();
    let spec = TrustedSections {
        metadata: small_meta(),
        tensors: &tensor,
        tensor_values: &rules,
        row_scales: &[],
        vocabulary: &vocabulary,
        provenance: &prov,
    };
    run_sections(b, &spec);
}

pub fn quantized_sections(b: &[u8]) {
    let t = [
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
    let vocabulary = small_vocab();
    let prov = mit();
    let spec = TrustedSections {
        metadata: small_meta(),
        tensors: &t,
        tensor_values: &rules,
        row_scales: &scales,
        vocabulary: &vocabulary,
        provenance: &prov,
    };
    run_sections(b, &spec);
}

pub fn tensors(b: &[u8]) {
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
    let mut scratch = [0_u64; 4];
    let _ = tensors::check_with_scratch(b, 1024, &specs, &mut scratch);
    if let Some((dir, data)) = b.split_at_checked(80) {
        let _ = tensors::check_values_with_scratch(
            dir,
            data,
            &specs,
            &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero],
            &mut scratch,
        );
    }
}

pub fn vocab(b: &[u8]) {
    let mut scratch = [0_u64; 5];
    let _ = vocab::check_with_scratch(b, &small_vocab(), &mut scratch);
}

pub fn safetensors(b: &[u8]) {
    let specs = [
        st::TensorSpec {
            name: "position",
            dtype: st::Dtype::I64,
            shape: &[2],
        },
        st::TensorSpec {
            name: "weight",
            dtype: st::Dtype::F32,
            shape: &[2],
        },
    ];
    if let Some(h) = sha256::digest(b) {
        let _ = st::inspect(b, &h, &specs, Some("pt"));
    }
}

/// Planted multi-stage bug: the fuzz workflow first proves the engine can
/// find a crash behind a magic header, a 32-bit constant and a length
/// relation. Never call this from anything but the fuzz engine.
pub fn canary(b: &[u8]) {
    if b.len() < 12 || &b[..4] != b"PLM1" {
        return;
    }
    let tag = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    if tag != 0x5EED_CAFE {
        return;
    }
    let count = u32::from_le_bytes([b[8], b[9], b[10], b[11]]) as usize;
    if count > 4 && count == b.len() - 12 {
        panic!("canary reached");
    }
}

pub fn tensors_f16(b: &[u8]) {
    let specs = [
        TensorSpec {
            id: 1,
            dtype: Dtype::I8,
            dims: [2, 4, 0, 0],
        },
        TensorSpec {
            id: 2,
            dtype: Dtype::F16,
            dims: [2, 0, 0, 0],
        },
        TensorSpec {
            id: 3,
            dtype: Dtype::F16,
            dims: [4, 0, 0, 0],
        },
    ];
    let rules = [
        ValueRule::AnyI8,
        ValueRule::PositiveNormal,
        ValueRule::FiniteNormalOrZero,
    ];
    let scales = [RowScale {
        weight_id: 1,
        scale_id: 2,
    }];
    tensors::check_row_scales(&specs, &rules, &scales).expect("trusted fuzz schema");
    if let Some((dir, data)) = b.split_at_checked(120) {
        let _ = tensors::check_values_with_scratch(dir, data, &specs, &rules, &mut [0_u64; 6]);
    }
}

pub fn lookup(name: &str) -> Option<fn(&[u8])> {
    Some(match name {
        "canary" => canary,
        "frame" => frame,
        "frame_budgeted" => frame_budgeted,
        "json" => json,
        "json_arena" => json_arena,
        "json_decode" => json_decode,
        "json_write" => json_write,
        "metadata" => metadata,
        "plm" => plm,
        "provenance" => provenance,
        "sections" => sections,
        "quantized_sections" => quantized_sections,
        "tensors" => tensors,
        "tensors_f16" => tensors_f16,
        "vocab" => vocab,
        "safetensors" => safetensors,
        _ => return None,
    })
}
