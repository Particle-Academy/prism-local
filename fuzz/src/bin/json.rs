fn main() -> std::io::Result<()> {
    prism_local_fuzz::run(prism_local_fuzz::targets::json)
}
