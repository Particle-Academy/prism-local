use prism_local_core::{plm, sha256};

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        // Use a matching digest so mutations reach the envelope parser.
        if let Some(digest) = sha256::digest(&bytes) {
            let _ = plm::check_envelope(&bytes, &digest);
            let _ = plm::check_envelope_for_arch(&bytes, &digest, 42);
        }
    }
    Ok(())
}
