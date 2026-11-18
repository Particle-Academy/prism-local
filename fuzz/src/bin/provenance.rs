use prism_local_core::provenance::{check, ProvenanceSpec, Source};

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
        license: "MIT\nCopyright Example\n",
        copyright: "Copyright Example",
        sources: &sources,
    };
    if let Some(bytes) = prism_local_fuzz::input()? {
        let _ = check(&bytes, &spec);
    }
    Ok(())
}
