//! Draft tensor-directory checks against a trusted architecture schema.
//!
//! This checks directory structure only. It does not validate tensor payloads,
//! quantization scales, metadata, tokenizer, provenance or an entire model.

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
pub struct TensorSpec {
    pub id: u32,
    pub dtype: Dtype,
    /// Nonzero leading dimensions followed by zero padding; rank is 1–4.
    pub dims: [u32; 4],
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidDirectory;

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

#[cfg(test)]
mod tests {
    use super::{check, Dtype, InvalidDirectory, TensorSpec};

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
    fn mutation_smoke_never_panics() {
        crate::test_support::mutate(&valid(), |input| {
            let _ = check(input, 80, &schema());
        });
    }
}
