//! Draft tensor checks against a trusted architecture schema.
//!
//! Directory and value checks are separate. Neither validates quantization
//! relationships, metadata, tokenizer, provenance or an entire model.

const ENTRY: usize = 40;
const MAX_TENSORS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dtype {
    I8,
    F16,
    F32,
}

impl Dtype {
    fn id(self) -> u8 {
        match self {
            Self::I8 => 1,
            Self::F16 => 2,
            Self::F32 => 3,
        }
    }
    fn size(self) -> u64 {
        match self {
            Self::I8 => 1,
            Self::F16 => 2,
            Self::F32 => 4,
        }
    }
}

/// Supplied by trusted engine code, never constructed from model metadata.
#[derive(Clone, Copy)]
pub struct TensorSpec {
    pub id: u32,
    pub dtype: Dtype,
    /// Nonzero leading dimensions followed by zero padding; rank is 1–4.
    pub dims: [u32; 4],
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidDirectory;

/// A trusted rule for the corresponding tensor's payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueRule {
    AnyI8,
    FiniteNormalOrZero,
    PositiveNormal,
}

/// A trusted schema relation between one i8 matrix and its per-row scales.
#[derive(Clone, Copy)]
pub struct RowScale {
    pub weight_id: u32,
    pub scale_id: u32,
}

/// Validate declared per-row scale relationships in a trusted schema.
/// Every declared pair must name distinct, unreused tensors: an i8 matrix and
/// a positive-normal f16/f32 scale vector with one element per matrix row.
/// This checks only declared pairs, not quantization accuracy or completeness.
pub fn check_row_scales(
    specs: &[TensorSpec],
    rules: &[ValueRule],
    pairs: &[RowScale],
) -> Result<(), InvalidDirectory> {
    if specs.len() > MAX_TENSORS
        || specs.len() != rules.len()
        || pairs.len() > specs.len()
        || specs.windows(2).any(|pair| pair[0].id >= pair[1].id)
    {
        return Err(InvalidDirectory);
    }
    for (index, pair) in pairs.iter().enumerate() {
        if pair.weight_id == pair.scale_id
            || pairs[..index].iter().any(|other| {
                other.weight_id == pair.weight_id
                    || other.scale_id == pair.scale_id
                    || other.weight_id == pair.scale_id
                    || other.scale_id == pair.weight_id
            })
        {
            return Err(InvalidDirectory);
        }
        let weight = specs
            .iter()
            .position(|spec| spec.id == pair.weight_id)
            .ok_or(InvalidDirectory)?;
        let scale = specs
            .iter()
            .position(|spec| spec.id == pair.scale_id)
            .ok_or(InvalidDirectory)?;
        let weight_spec = &specs[weight];
        let scale_spec = &specs[scale];
        let [rows, columns, third, fourth] = weight_spec.dims;
        if weight_spec.dtype != Dtype::I8
            || rules[weight] != ValueRule::AnyI8
            || rows == 0
            || columns == 0
            || third != 0
            || fourth != 0
            || !matches!(scale_spec.dtype, Dtype::F16 | Dtype::F32)
            || rules[scale] != ValueRule::PositiveNormal
            || scale_spec.dims != [rows, 0, 0, 0]
        {
            return Err(InvalidDirectory);
        }
    }
    Ok(())
}

/// Check exact IDs, types, shapes, lengths, alignment and non-overlap.
/// `data_len` is the independently validated tensor-data section length.
pub fn check(
    directory: &[u8],
    data_len: u64,
    specs: &[TensorSpec],
) -> Result<(), InvalidDirectory> {
    if specs.is_empty()
        || specs.len() > MAX_TENSORS
        || specs.len().checked_mul(ENTRY) != Some(directory.len())
    {
        return Err(InvalidDirectory);
    }
    let mut ranges = Vec::with_capacity(specs.len());
    let mut previous_id = None;
    for (record, spec) in directory.as_chunks::<ENTRY>().0.iter().zip(specs) {
        if previous_id.is_some_and(|previous| previous >= spec.id) {
            return Err(InvalidDirectory);
        }
        previous_id = Some(spec.id);
        let id = u32::from_le_bytes(record[0..4].try_into().map_err(|_| InvalidDirectory)?);
        if id != spec.id || record[4] != spec.dtype.id() || record[6..8] != [0, 0] {
            return Err(InvalidDirectory);
        }
        let rank = usize::from(record[5]);
        if !(1..=4).contains(&rank) {
            return Err(InvalidDirectory);
        }
        let mut elements = 1_u64;
        for (index, expected_dim) in spec.dims.iter().enumerate() {
            let start = 8 + 4 * index;
            let dim = u32::from_le_bytes(
                record[start..start + 4]
                    .try_into()
                    .map_err(|_| InvalidDirectory)?,
            );
            if dim != *expected_dim || (index < rank && dim == 0) || (index >= rank && dim != 0) {
                return Err(InvalidDirectory);
            }
            if index < rank {
                elements = elements
                    .checked_mul(u64::from(dim))
                    .ok_or(InvalidDirectory)?;
            }
        }
        let offset = u64::from_le_bytes(record[24..32].try_into().map_err(|_| InvalidDirectory)?);
        let length = u64::from_le_bytes(record[32..40].try_into().map_err(|_| InvalidDirectory)?);
        let expected = elements
            .checked_mul(spec.dtype.size())
            .ok_or(InvalidDirectory)?;
        let end = offset.checked_add(length).ok_or(InvalidDirectory)?;
        if offset % 64 != 0 || length != expected || end > data_len {
            return Err(InvalidDirectory);
        }
        ranges.push((offset, end));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(InvalidDirectory);
    }
    Ok(())
}

/// Validate payload values after directory validation, using trusted rules.
/// Float subnormals and nonfinite values are refused; positive scale-like
/// values must additionally be nonzero and positive. This is not sufficient
/// to establish quantization correctness or complete model validity.
pub fn check_values(
    directory: &[u8],
    data: &[u8],
    specs: &[TensorSpec],
    rules: &[ValueRule],
) -> Result<(), InvalidDirectory> {
    if rules.len() != specs.len() {
        return Err(InvalidDirectory);
    }
    check(
        directory,
        u64::try_from(data.len()).map_err(|_| InvalidDirectory)?,
        specs,
    )?;
    let mut ranges = Vec::new();
    ranges
        .try_reserve_exact(specs.len())
        .map_err(|_| InvalidDirectory)?;
    for ((record, spec), rule) in directory
        .as_chunks::<ENTRY>()
        .0
        .iter()
        .zip(specs)
        .zip(rules)
    {
        let offset = u64::from_le_bytes(record[24..32].try_into().map_err(|_| InvalidDirectory)?);
        let length = u64::from_le_bytes(record[32..40].try_into().map_err(|_| InvalidDirectory)?);
        let start = usize::try_from(offset).map_err(|_| InvalidDirectory)?;
        let end = start
            .checked_add(usize::try_from(length).map_err(|_| InvalidDirectory)?)
            .ok_or(InvalidDirectory)?;
        let payload = data.get(start..end).ok_or(InvalidDirectory)?;
        ranges.push((start, end));
        let valid = match (spec.dtype, rule) {
            (Dtype::I8, ValueRule::AnyI8) => true,
            (Dtype::F16, ValueRule::FiniteNormalOrZero) => payload
                .as_chunks::<2>()
                .0
                .iter()
                .all(|b| valid_float(u32::from(u16::from_le_bytes(*b)), 10, 5, false)),
            (Dtype::F16, ValueRule::PositiveNormal) => payload
                .as_chunks::<2>()
                .0
                .iter()
                .all(|b| valid_float(u32::from(u16::from_le_bytes(*b)), 10, 5, true)),
            (Dtype::F32, ValueRule::FiniteNormalOrZero) => payload
                .as_chunks::<4>()
                .0
                .iter()
                .all(|b| valid_float(u32::from_le_bytes(*b), 23, 8, false)),
            (Dtype::F32, ValueRule::PositiveNormal) => payload
                .as_chunks::<4>()
                .0
                .iter()
                .all(|b| valid_float(u32::from_le_bytes(*b), 23, 8, true)),
            _ => false,
        };
        if !valid {
            return Err(InvalidDirectory);
        }
    }
    ranges.sort_unstable();
    let mut next = 0;
    for (start, end) in ranges {
        if data
            .get(next..start)
            .ok_or(InvalidDirectory)?
            .iter()
            .any(|&byte| byte != 0)
        {
            return Err(InvalidDirectory);
        }
        next = end;
    }
    if data
        .get(next..)
        .ok_or(InvalidDirectory)?
        .iter()
        .any(|&byte| byte != 0)
    {
        return Err(InvalidDirectory);
    }
    Ok(())
}

fn valid_float(bits: u32, mantissa_bits: u32, exponent_bits: u32, positive: bool) -> bool {
    let exponent_mask = (1_u32 << exponent_bits) - 1;
    let exponent = (bits >> mantissa_bits) & exponent_mask;
    let mantissa = bits & ((1_u32 << mantissa_bits) - 1);
    let sign = bits >> (mantissa_bits + exponent_bits);
    exponent != exponent_mask
        && !(exponent == 0 && mantissa != 0)
        && (!positive || (sign == 0 && exponent != 0))
}

#[cfg(test)]
mod tests {
    use super::{
        check, check_row_scales, check_values, Dtype, InvalidDirectory, RowScale, TensorSpec,
        ValueRule,
    };

    fn entry(id: u32, dtype: u8, rank: u8, dims: [u32; 4], offset: u64, len: u64) -> Vec<u8> {
        let mut b = vec![0; 40];
        b[0..4].copy_from_slice(&id.to_le_bytes());
        b[4] = dtype;
        b[5] = rank;
        for (i, d) in dims.iter().enumerate() {
            b[8 + i * 4..12 + i * 4].copy_from_slice(&d.to_le_bytes());
        }
        b[24..32].copy_from_slice(&offset.to_le_bytes());
        b[32..40].copy_from_slice(&len.to_le_bytes());
        b
    }
    fn schema() -> [TensorSpec; 2] {
        [
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
        ]
    }
    fn valid() -> Vec<u8> {
        [
            entry(1, 1, 2, [2, 4, 0, 0], 0, 8),
            entry(2, 3, 1, [4, 0, 0, 0], 64, 16),
        ]
        .concat()
    }
    #[test]
    fn validates_schema_and_rejects_shape_overflow_overlap_and_truncation() {
        assert_eq!(check(&valid(), 80, &schema()), Ok(()));
        let mut cases = Vec::new();
        let mut b = valid();
        b[0] = 2;
        cases.push(b);
        let mut b = valid();
        b[4] = 3;
        cases.push(b);
        let mut b = valid();
        b[5] = 0;
        cases.push(b);
        let mut b = valid();
        b[6] = 1;
        cases.push(b);
        let mut b = valid();
        b[16] = 1;
        cases.push(b);
        let mut b = valid();
        b[24..32].copy_from_slice(&1_u64.to_le_bytes());
        cases.push(b);
        let mut b = valid();
        b[32..40].copy_from_slice(&7_u64.to_le_bytes());
        cases.push(b);
        let mut b = valid();
        b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(b);
        let mut b = valid();
        b[64..72].copy_from_slice(&0_u64.to_le_bytes());
        cases.push(b);
        let mut b = valid();
        b[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        cases.push(b);
        let mut b = valid();
        b[32..40].copy_from_slice(&u64::MAX.to_le_bytes());
        cases.push(b);
        let mut b = valid();
        b.pop();
        cases.push(b);
        for b in cases {
            assert_eq!(check(&b, 80, &schema()), Err(InvalidDirectory));
        }
        assert_eq!(check(&valid(), 79, &schema()), Err(InvalidDirectory));
        assert_eq!(check(&[], 80, &[]), Err(InvalidDirectory));
    }
    #[test]
    fn rejects_overflow_even_when_untrusted_shape_matches_the_schema() {
        let dims = [u32::MAX; 4];
        let schema = [TensorSpec {
            id: 1,
            dtype: Dtype::F32,
            dims,
        }];
        assert_eq!(
            check(&entry(1, 3, 4, dims, 0, 1), u64::MAX, &schema),
            Err(InvalidDirectory)
        );
    }

    #[test]
    fn checks_declared_per_row_scale_schema_relationships() {
        let mut specs = [
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
        let pair = RowScale {
            weight_id: 1,
            scale_id: 2,
        };
        assert_eq!(check_row_scales(&specs, &rules, &[pair]), Ok(()));
        specs[1].dims[0] = 3;
        assert_eq!(
            check_row_scales(&specs, &rules, &[pair]),
            Err(InvalidDirectory)
        );
        specs[1].dims[0] = 2;
        assert_eq!(
            check_row_scales(
                &specs,
                &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero],
                &[pair]
            ),
            Err(InvalidDirectory)
        );
        assert_eq!(
            check_row_scales(&specs, &rules, &[pair, pair]),
            Err(InvalidDirectory)
        );
        assert_eq!(
            check_row_scales(
                &specs,
                &rules,
                &[RowScale {
                    weight_id: 1,
                    scale_id: 3
                }]
            ),
            Err(InvalidDirectory)
        );
        specs.swap(0, 1);
        assert_eq!(
            check_row_scales(&specs, &rules, &[pair]),
            Err(InvalidDirectory)
        );
    }

    #[test]
    fn mutation_smoke_never_panics() {
        crate::test_support::mutate(&valid(), |input| {
            let _ = check(input, 80, &schema());
        });
    }

    #[test]
    fn checks_f32_payload_rules_and_rejects_nonfinite_subnormal_and_bad_scale() {
        let directory = valid();
        let mut data = vec![0_u8; 80];
        for (index, value) in [1.0_f32, 2.0, 3.0, 4.0].iter().enumerate() {
            data[64 + index * 4..68 + index * 4].copy_from_slice(&value.to_bits().to_le_bytes());
        }
        let scales = [ValueRule::AnyI8, ValueRule::PositiveNormal];
        assert_eq!(check_values(&directory, &data, &schema(), &scales), Ok(()));
        let mut nonzero_padding = data.clone();
        nonzero_padding[8] = 1;
        assert_eq!(
            check_values(&directory, &nonzero_padding, &schema(), &scales),
            Err(InvalidDirectory)
        );
        let mut nonzero_trailer = data.clone();
        nonzero_trailer.push(1);
        assert_eq!(
            check_values(&directory, &nonzero_trailer, &schema(), &scales),
            Err(InvalidDirectory)
        );
        let mut shifted_directory = directory.clone();
        shifted_directory[24..32].copy_from_slice(&64_u64.to_le_bytes());
        shifted_directory[64..72].copy_from_slice(&128_u64.to_le_bytes());
        let mut shifted_data = vec![0; 64];
        shifted_data.extend_from_slice(&data);
        assert_eq!(
            check_values(&shifted_directory, &shifted_data, &schema(), &scales),
            Ok(())
        );
        shifted_data[0] = 1;
        assert_eq!(
            check_values(&shifted_directory, &shifted_data, &schema(), &scales),
            Err(InvalidDirectory)
        );
        for bits in [
            0_u32,
            (-0.0_f32).to_bits(),
            (-1.0_f32).to_bits(),
            f32::INFINITY.to_bits(),
            f32::NAN.to_bits(),
            1,
        ] {
            let mut bad = data.clone();
            bad[64..68].copy_from_slice(&bits.to_le_bytes());
            assert_eq!(
                check_values(&directory, &bad, &schema(), &scales),
                Err(InvalidDirectory)
            );
        }
        let mut negative = data.clone();
        negative[64..68].copy_from_slice(&(-1.0_f32).to_bits().to_le_bytes());
        assert_eq!(
            check_values(
                &directory,
                &negative,
                &schema(),
                &[ValueRule::AnyI8, ValueRule::FiniteNormalOrZero]
            ),
            Ok(())
        );
        assert_eq!(
            check_values(&directory, &data, &schema(), &[ValueRule::AnyI8]),
            Err(InvalidDirectory)
        );
        assert_eq!(
            check_values(
                &directory,
                &data,
                &schema(),
                &[ValueRule::PositiveNormal, ValueRule::PositiveNormal]
            ),
            Err(InvalidDirectory)
        );
    }

    #[test]
    fn checks_f16_payloads_without_platform_float_conversion() {
        let specs = [TensorSpec {
            id: 1,
            dtype: Dtype::F16,
            dims: [2, 0, 0, 0],
        }];
        let directory = entry(1, 2, 1, [2, 0, 0, 0], 0, 4);
        assert_eq!(
            check_values(
                &directory,
                &[0, 0x3c, 0, 0xbc],
                &specs,
                &[ValueRule::FiniteNormalOrZero]
            ),
            Ok(())
        );
        assert_eq!(
            check_values(
                &directory,
                &[0, 0x3c, 0, 0xbc],
                &specs,
                &[ValueRule::PositiveNormal]
            ),
            Err(InvalidDirectory)
        );
        for bits in [0x7c00_u16, 0x7e00, 0x0001] {
            let mut data = [0, 0x3c, 0, 0];
            data[2..4].copy_from_slice(&bits.to_le_bytes());
            assert_eq!(
                check_values(&directory, &data, &specs, &[ValueRule::FiniteNormalOrZero]),
                Err(InvalidDirectory)
            );
        }
    }
}
