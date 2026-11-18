use prism_local_core::frame::{read_frame, HARD_MAX_FRAME};

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        let _ = read_frame(&mut bytes.as_slice(), HARD_MAX_FRAME);
    }
    Ok(())
}
