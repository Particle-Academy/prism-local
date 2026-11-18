use prism_local_core::provenance::{check, LicenseKind, ProvenanceSpec, Source};

fn main() -> std::io::Result<()> {
    let sources = [Source {
        name: "weights",
        sha256: [7; 32],
    }];
    let spec = ProvenanceSpec {
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
    let apache = ProvenanceSpec {
        repository: "p/m",
        revision: "abc",
        publisher: "p",
        converter: "v1",
        license_kind: LicenseKind::Apache2,
        license: "Synthetic Apache-2.0 license text\n",
        copyright: "",
        notice: "Required upstream notice\n",
        sources: &sources,
    };
    if let Some(bytes) = prism_local_fuzz::input()? {
        let _ = check(&bytes, &spec);
        let _ = check(&bytes, &apache);
    }
    Ok(())
}
