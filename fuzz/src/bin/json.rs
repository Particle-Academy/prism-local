use prism_local_core::json;

fn main() -> std::io::Result<()> {
    if let Some(bytes) = prism_local_fuzz::input()? {
        let _ = json::parse(&bytes);
    }
    Ok(())
}
