//! Draft provenance-section checks against a separately trusted manifest.

const MAX_SECTION: usize = 1024 * 1024;
const MAX_SOURCES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseKind {
    Mit,
    Apache2,
}

impl LicenseKind {
    fn label(self) -> &'static str {
        match self {
            Self::Mit => "mit",
            Self::Apache2 => "apache-2.0",
        }
    }
}

pub struct Source<'a> {
    pub name: &'a str,
    pub sha256: [u8; 32],
}
pub struct ProvenanceSpec<'a> {
    pub repository: &'a str,
    pub revision: &'a str,
    pub publisher: &'a str,
    pub converter: &'a str,
    pub license_kind: LicenseKind,
    pub license: &'a str,
    pub copyright: &'a str,
    /// Exact trusted notice text, empty only when no notice is required.
    pub notice: &'a str,
    /// Sorted by source name, with no duplicates.
    pub sources: &'a [Source<'a>],
}

#[derive(Debug, PartialEq, Eq)]
pub struct InvalidProvenance;

/// Compare all fields to trusted values. Does not verify source file bytes or
/// grant permission to load the model; those are separate required checks.
pub fn check(bytes: &[u8], spec: &ProvenanceSpec<'_>) -> Result<(), InvalidProvenance> {
    if bytes.len() > MAX_SECTION
        || spec.sources.is_empty()
        || spec.sources.len() > MAX_SOURCES
        || (spec.license_kind == LicenseKind::Mit
            && (spec.copyright.is_empty()
                || !spec.license.lines().any(|line| line == spec.copyright)))
        || spec
            .sources
            .windows(2)
            .any(|pair| pair[0].name >= pair[1].name)
    {
        return Err(InvalidProvenance);
    }
    let mut cursor = Cursor { bytes, pos: 0 };
    let count = u32::from_le_bytes(cursor.take(4)?.try_into().map_err(|_| InvalidProvenance)?);
    if usize::try_from(count).ok() != Some(spec.sources.len()) {
        return Err(InvalidProvenance);
    }
    for expected in [
        spec.repository,
        spec.revision,
        spec.publisher,
        spec.converter,
        spec.license_kind.label(),
        spec.license,
    ] {
        if expected.is_empty() || cursor.text()? != expected {
            return Err(InvalidProvenance);
        }
    }
    if cursor.text_allow_empty()? != spec.copyright || cursor.text_allow_empty()? != spec.notice {
        return Err(InvalidProvenance);
    }
    for source in spec.sources {
        if source.name.is_empty()
            || cursor.text()? != source.name
            || cursor.take(32)? != source.sha256
        {
            return Err(InvalidProvenance);
        }
    }
    if cursor.pos != bytes.len() {
        return Err(InvalidProvenance);
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], InvalidProvenance> {
        let end = self.pos.checked_add(len).ok_or(InvalidProvenance)?;
        let value = self.bytes.get(self.pos..end).ok_or(InvalidProvenance)?;
        self.pos = end;
        Ok(value)
    }
    fn text(&mut self) -> Result<&'a str, InvalidProvenance> {
        let value = self.text_allow_empty()?;
        if value.is_empty() {
            return Err(InvalidProvenance);
        }
        Ok(value)
    }
    fn text_allow_empty(&mut self) -> Result<&'a str, InvalidProvenance> {
        let len = u16::from_le_bytes(self.take(2)?.try_into().map_err(|_| InvalidProvenance)?);
        std::str::from_utf8(self.take(usize::from(len))?).map_err(|_| InvalidProvenance)
    }
}

#[cfg(test)]
mod tests {
    use super::{check, InvalidProvenance, LicenseKind, ProvenanceSpec, Source};
    fn spec<'a>(sources: &'a [Source<'a>]) -> ProvenanceSpec<'a> {
        ProvenanceSpec {
            repository: "publisher/model",
            revision: "abc",
            publisher: "publisher",
            converter: "v1",
            license_kind: LicenseKind::Mit,
            license: "MIT\nCopyright (c) 2022 Example\n",
            copyright: "Copyright (c) 2022 Example",
            notice: "",
            sources,
        }
    }
    fn section() -> Vec<u8> {
        let mut b = 1_u32.to_le_bytes().to_vec();
        for value in [
            "publisher/model",
            "abc",
            "publisher",
            "v1",
            "mit",
            "MIT\nCopyright (c) 2022 Example\n",
            "Copyright (c) 2022 Example",
            "",
            "model.bin",
        ] {
            b.extend_from_slice(&(value.len() as u16).to_le_bytes());
            b.extend_from_slice(value.as_bytes());
        }
        b.extend_from_slice(&[7; 32]);
        b
    }
    #[test]
    fn enforces_identity_license_copyright_source_digests_and_exact_length() {
        let sources = [Source {
            name: "model.bin",
            sha256: [7; 32],
        }];
        let s = spec(&sources);
        let b = section();
        assert_eq!(check(&b, &s), Ok(()));
        for end in 0..b.len() {
            assert_eq!(check(&b[..end], &s), Err(InvalidProvenance));
        }
        let mut altered = b.clone();
        altered[6] ^= 1;
        assert_eq!(check(&altered, &s), Err(InvalidProvenance));
        let mut altered = b.clone();
        let last = altered.len() - 1;
        altered[last] ^= 1;
        assert_eq!(check(&altered, &s), Err(InvalidProvenance));
        let mut altered = b.clone();
        altered.push(0);
        assert_eq!(check(&altered, &s), Err(InvalidProvenance));
        let mut altered = b.clone();
        let license_id = altered
            .windows(3)
            .position(|window| window == b"mit")
            .unwrap();
        altered[license_id..license_id + 3].copy_from_slice(b"gpl");
        assert_eq!(check(&altered, &s), Err(InvalidProvenance));
        let mut wrong = spec(&sources);
        wrong.copyright = "other";
        assert_eq!(check(&b, &wrong), Err(InvalidProvenance));
    }
    #[test]
    fn rejects_duplicate_or_unsorted_trusted_source_names() {
        let sources = [
            Source {
                name: "model.bin",
                sha256: [7; 32],
            },
            Source {
                name: "model.bin",
                sha256: [7; 32],
            },
        ];
        assert_eq!(check(&section(), &spec(&sources)), Err(InvalidProvenance));
    }

    #[test]
    fn supports_apache_without_inventing_a_copyright_line_and_checks_notice() {
        let sources = [Source {
            name: "model.bin",
            sha256: [7; 32],
        }];
        let spec = ProvenanceSpec {
            repository: "publisher/model",
            revision: "abc",
            publisher: "publisher",
            converter: "v1",
            license_kind: LicenseKind::Apache2,
            license: "Synthetic Apache-2.0 license text\n",
            copyright: "",
            notice: "Required upstream notice\n",
            sources: &sources,
        };
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        for text in [
            spec.repository,
            spec.revision,
            spec.publisher,
            spec.converter,
            "apache-2.0",
            spec.license,
            spec.copyright,
            spec.notice,
            "model.bin",
        ] {
            bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
        bytes.extend_from_slice(&[7; 32]);
        assert_eq!(check(&bytes, &spec), Ok(()));
        let wrong_notice = ProvenanceSpec { notice: "", ..spec };
        assert_eq!(check(&bytes, &wrong_notice), Err(InvalidProvenance));
        let wrong_kind = ProvenanceSpec {
            license_kind: LicenseKind::Mit,
            ..spec
        };
        assert_eq!(check(&bytes, &wrong_kind), Err(InvalidProvenance));
    }

    #[test]
    fn mutation_smoke_never_panics() {
        let sources = [Source {
            name: "model.bin",
            sha256: [7; 32],
        }];
        crate::test_support::mutate(&section(), |input| {
            let _ = check(input, &spec(&sources));
        });
    }
}
