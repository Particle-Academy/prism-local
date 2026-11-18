//! Draft PLM1 envelope parsing. This does not validate or load a model.

use crate::sha256;

const HEADER_LEN: usize = 64;
const RECORD_LEN: usize = 32;
const SECTIONS: usize = 5;

/// An authenticated but otherwise untrusted, structurally checked envelope.
/// Its sections have not been validated and must not be used for inference.
#[derive(Debug, PartialEq, Eq)]
pub struct StructuralEnvelope;

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    DigestMismatch,
    InvalidEnvelope,
}

/// Verify the trusted digest and check container structure using the same bytes.
///
/// Success is not model validation: all section content remains untrusted.
pub fn check_envelope(bytes: &[u8], expected: &[u8; 32]) -> Result<StructuralEnvelope, Error> {
    if sha256::digest(bytes).as_ref() != Some(expected) {
        return Err(Error::DigestMismatch);
    }
    validate(bytes).ok_or(Error::InvalidEnvelope)?;
    Ok(StructuralEnvelope)
}

fn validate(bytes: &[u8]) -> Option<()> {
    let header = bytes.get(..HEADER_LEN)?;
    if header.get(..4)? != b"PLM1"
        || read_u16(header, 4)? != 1
        || read_u16(header, 6)? != 1
        || usize::try_from(read_u64(header, 8)?).ok()? != bytes.len()
        || read_u32(header, 24)? != SECTIONS as u32
        || header.get(28..)?.iter().any(|&b| b != 0)
    {
        return None;
    }
    let table_start = usize::try_from(read_u64(header, 16)?).ok()?;
    let table_end = table_start.checked_add(SECTIONS.checked_mul(RECORD_LEN)?)?;
    if table_start < HEADER_LEN || table_start % 64 != 0 || table_end > bytes.len() {
        return None;
    }

    let mut ranges = [(0_usize, 0_usize); SECTIONS];
    for (index, range) in ranges.iter_mut().enumerate() {
        let pos = table_start.checked_add(index.checked_mul(RECORD_LEN)?)?;
        let record = bytes.get(pos..pos.checked_add(RECORD_LEN)?)?;
        if read_u32(record, 0)? != u32::try_from(index + 1).ok()?
            || read_u32(record, 4)? != 0
            || read_u64(record, 24)? != 0
        {
            return None;
        }
        let start = usize::try_from(read_u64(record, 8)?).ok()?;
        let len = usize::try_from(read_u64(record, 16)?).ok()?;
        let end = start.checked_add(len)?;
        if len == 0
            || start % 64 != 0
            || end > bytes.len()
            || overlaps((start, end), (0, HEADER_LEN))
            || overlaps((start, end), (table_start, table_end))
        {
            return None;
        }
        *range = (start, end);
    }
    for (i, &left) in ranges.iter().enumerate() {
        if ranges[i + 1..].iter().any(|&right| overlaps(left, right)) {
            return None;
        }
    }
    Some(())
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

fn read_u16(bytes: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(pos..pos.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(pos..pos.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn read_u64(bytes: &[u8], pos: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(pos..pos.checked_add(8)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{check_envelope, sha256, Error, StructuralEnvelope};

    fn put16(b: &mut [u8], pos: usize, n: u16) {
        b[pos..pos + 2].copy_from_slice(&n.to_le_bytes());
    }
    fn put32(b: &mut [u8], pos: usize, n: u32) {
        b[pos..pos + 4].copy_from_slice(&n.to_le_bytes());
    }
    fn put64(b: &mut [u8], pos: usize, n: u64) {
        b[pos..pos + 8].copy_from_slice(&n.to_le_bytes());
    }
    fn valid() -> Vec<u8> {
        let mut b = vec![0_u8; 576];
        b[..4].copy_from_slice(b"PLM1");
        put16(&mut b, 4, 1);
        put16(&mut b, 6, 1);
        put64(&mut b, 8, 576);
        put64(&mut b, 16, 64);
        put32(&mut b, 24, 5);
        for i in 0..5 {
            let pos = 64 + i * 32;
            put32(&mut b, pos, (i + 1) as u32);
            put64(&mut b, pos + 8, (256 + i * 64) as u64);
            put64(&mut b, pos + 16, 1);
        }
        b
    }
    fn check(b: &[u8]) -> Result<StructuralEnvelope, Error> {
        check_envelope(b, &sha256::digest(b).unwrap())
    }

    #[test]
    fn accepts_only_well_formed_envelope_and_authenticates_first() {
        let mut b = valid();
        let digest = sha256::digest(&b).unwrap();
        assert_eq!(check_envelope(&b, &digest), Ok(StructuralEnvelope));
        b[256] ^= 1;
        assert_eq!(check_envelope(&b, &digest), Err(Error::DigestMismatch));
        assert_eq!(check(&b), Ok(StructuralEnvelope)); // content is not yet checked
    }

    #[test]
    fn rejects_truncation_and_all_reserved_header_bytes() {
        let b = valid();
        for end in 0..b.len() {
            assert_eq!(check(&b[..end]), Err(Error::InvalidEnvelope));
        }
        for index in 28..64 {
            let mut b = valid();
            b[index] = 1;
            assert_eq!(check(&b), Err(Error::InvalidEnvelope));
        }
    }

    #[test]
    fn rejects_overflows_overlap_bad_alignment_and_missing_types() {
        for (offset, value) in [
            (16, u64::MAX),
            (72, u64::MAX),
            (80, u64::MAX),
            (72, 64),
            (72, 257),
            (104, 256),
            (80, 0),
        ] {
            let mut b = valid();
            put64(&mut b, offset, value);
            assert_eq!(
                check(&b),
                Err(Error::InvalidEnvelope),
                "offset={offset} value={value}"
            );
        }
        let mut b = valid();
        put32(&mut b, 96, 1);
        assert_eq!(check(&b), Err(Error::InvalidEnvelope));
        let mut b = valid();
        put32(&mut b, 24, u32::MAX);
        assert_eq!(check(&b), Err(Error::InvalidEnvelope));
        let mut b = valid();
        put16(&mut b, 6, 2);
        assert_eq!(check(&b), Err(Error::InvalidEnvelope));
    }

    #[test]
    fn deterministic_mutation_smoke_never_panics() {
        let seed = valid();
        let mut state = 0x6a09_e667_u32;
        for _ in 0..4096 {
            let mut b = seed.clone();
            for _ in 0..4 {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let index = state as usize % b.len();
                b[index] ^= (state >> 24) as u8;
            }
            let _ = check(&b);
        }
    }
}
