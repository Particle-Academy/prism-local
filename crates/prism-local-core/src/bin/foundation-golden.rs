fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .ok_or_else(|| std::io::Error::other("expected output path"))?;
    if args.next().is_some() {
        return Err(std::io::Error::other("expected exactly one output path"));
    }
    std::fs::write(path, prism_local_core::golden::bytes())
}
