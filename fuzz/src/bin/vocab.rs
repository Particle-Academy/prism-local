use prism_local_core::vocab::{check, VocabSpec};

fn main() -> std::io::Result<()> {
    let spec = VocabSpec {
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
    if let Some(bytes) = prism_local_fuzz::input()? {
        let _ = check(&bytes, &spec);
    }
    Ok(())
}
