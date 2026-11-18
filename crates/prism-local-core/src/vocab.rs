//! Draft WordPiece vocabulary-section validation, not tokenization.

const HEADER: usize = 32;
const MAX_VOCAB: usize = 131_072;
const MAX_TOKEN_BYTES: usize = 1_024;
const MAX_SECTION_BYTES: usize = 16 * 1024 * 1024;

pub struct VocabSpec<'a> {
    pub count: u32,
    pub flags: u32,
    /// Trusted expected ID and spelling for five distinct special tokens.
    pub special: [(u32, &'a str); 5],
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidVocab;

/// Check IDs, spellings, UTF-8, uniqueness, bounds and exact section length.
/// This does not perform BERT normalization or WordPiece tokenization.
pub fn check(bytes: &[u8], spec: &VocabSpec<'_>) -> Result<(), InvalidVocab> {
    let count = usize::try_from(spec.count).map_err(|_| InvalidVocab)?;
    if !(5..=MAX_VOCAB).contains(&count) || bytes.len() < HEADER || bytes.len() > MAX_SECTION_BYTES
    {
        return Err(InvalidVocab);
    }
    if read32(bytes, 0)? != spec.count || read32(bytes, 4)? != spec.flags || read32(bytes, 28)? != 0
    {
        return Err(InvalidVocab);
    }
    for (index, (id, spelling)) in spec.special.iter().enumerate() {
        if *id >= spec.count
            || spelling.is_empty()
            || read32(bytes, 8 + 4 * index)? != *id
            || spec.special[..index].iter().any(|(other, _)| other == id)
        {
            return Err(InvalidVocab);
        }
    }
    let mut tokens = Vec::new();
    tokens.try_reserve_exact(count).map_err(|_| InvalidVocab)?;
    let mut pos = HEADER;
    for index in 0..count {
        let len_end = pos.checked_add(2).ok_or(InvalidVocab)?;
        let len = u16::from_le_bytes(
            bytes
                .get(pos..len_end)
                .ok_or(InvalidVocab)?
                .try_into()
                .map_err(|_| InvalidVocab)?,
        );
        let len = usize::from(len);
        if len == 0 || len > MAX_TOKEN_BYTES {
            return Err(InvalidVocab);
        }
        let end = len_end.checked_add(len).ok_or(InvalidVocab)?;
        let token = std::str::from_utf8(bytes.get(len_end..end).ok_or(InvalidVocab)?)
            .map_err(|_| InvalidVocab)?;
        if spec
            .special
            .iter()
            .any(|(id, expected)| usize::try_from(*id).ok() == Some(index) && token != *expected)
        {
            return Err(InvalidVocab);
        }
        tokens.push(token);
        pos = end;
    }
    if pos != bytes.len() {
        return Err(InvalidVocab);
    }
    tokens.sort_unstable();
    if tokens.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(InvalidVocab);
    }
    Ok(())
}

fn read32(bytes: &[u8], pos: usize) -> Result<u32, InvalidVocab> {
    let end = pos.checked_add(4).ok_or(InvalidVocab)?;
    Ok(u32::from_le_bytes(
        bytes
            .get(pos..end)
            .ok_or(InvalidVocab)?
            .try_into()
            .map_err(|_| InvalidVocab)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{check, InvalidVocab, VocabSpec};

    fn spec() -> VocabSpec<'static> {
        VocabSpec {
            count: 6,
            flags: 1,
            special: [
                (0, "[PAD]"),
                (1, "[UNK]"),
                (2, "[CLS]"),
                (3, "[SEP]"),
                (4, "[MASK]"),
            ],
        }
    }
    fn section() -> Vec<u8> {
        let mut b = vec![0; 32];
        b[0..4].copy_from_slice(&6_u32.to_le_bytes());
        b[4..8].copy_from_slice(&1_u32.to_le_bytes());
        for id in 0..5_u32 {
            b[8 + id as usize * 4..12 + id as usize * 4].copy_from_slice(&id.to_le_bytes());
        }
        for token in ["[PAD]", "[UNK]", "[CLS]", "[SEP]", "[MASK]", "hello"] {
            b.extend_from_slice(&(token.len() as u16).to_le_bytes());
            b.extend_from_slice(token.as_bytes());
        }
        b
    }
    #[test]
    fn accepts_valid_and_rejects_corrupt_sections() {
        assert_eq!(check(&section(), &spec()), Ok(()));
        let mut cases = Vec::new();
        let mut b = section();
        b[0..4].copy_from_slice(&u32::MAX.to_le_bytes());
        cases.push(b);
        let mut b = section();
        b[8..12].copy_from_slice(&1_u32.to_le_bytes());
        cases.push(b);
        let mut b = section();
        b[28] = 1;
        cases.push(b);
        let mut b = section();
        b[32..34].copy_from_slice(&u16::MAX.to_le_bytes());
        cases.push(b);
        let mut b = section();
        b[34] = 0xff;
        cases.push(b);
        let mut b = section();
        b.pop();
        cases.push(b);
        let mut b = section();
        b.push(0);
        cases.push(b);
        let mut b = section();
        let end = b.len();
        b[end - 5..].copy_from_slice(b"[PAD]");
        cases.push(b);
        for b in cases {
            assert_eq!(check(&b, &spec()), Err(InvalidVocab));
        }
        let mut s = spec();
        s.special[4].0 = 6;
        assert_eq!(check(&section(), &s), Err(InvalidVocab));
    }

    #[test]
    fn mutation_smoke_never_panics() {
        crate::test_support::mutate(&section(), |input| {
            let _ = check(input, &spec());
        });
    }
}
