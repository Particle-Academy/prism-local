fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .ok_or_else(|| std::io::Error::other("expected output path"))?;
    if args.next().is_some() {
        return Err(std::io::Error::other("expected exactly one output path"));
    }
    let (bytes, workers) = prism_local_core::golden::fixture();
    eprintln!("Row-dot fixture checked one worker and {workers} worker(s).");
    std::fs::write(path, bytes)
}
