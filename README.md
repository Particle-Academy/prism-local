# prism-local

Prism Local is a planned zero-dependency local inference engine for the Prism
ecosystem. The current repository is an **early foundation**: it does not load
models or perform inference. Byte-identical inference across CPUs, operating
systems and thread counts is a requirement that has not yet been demonstrated.

The Rust workspace uses only the standard library and the toolchain pinned in
`rust-toolchain.toml`. After installing that toolchain, build and test without
network access:

```text
cargo build --workspace --locked --offline
cargo test --workspace --locked --offline
```

The sidecar currently exposes only a bounded stdio JSON-RPC skeleton; it has no
network or download implementation. See [the protocol](docs/protocol.md) for
its limits and [the draft PLM format](docs/plm-format.md) for the model checks
that exist today. [Offline conversion](docs/offline-conversion.md),
[numerical references](docs/numeric-reference.md) and
[build reproducibility](docs/reproducible-builds.md) describe their evidence
and open work. The [fuzz harnesses](fuzz/README.md) have not yet completed a
coverage-guided campaign.
