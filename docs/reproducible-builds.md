# Reproducible build check

CI builds the committed source twice from separate `git archive HEAD` copies
on each Windows and Linux runner and compares the release executable bytes.
The check uses the pinned Rust toolchain, Cargo's offline and locked modes,
and remaps each source path to `/source`. Windows builds also pass `/Brepro`
to the linker. Run it from the repository root with
`pwsh -NoProfile -File tools/check-repro.ps1`.

The check covers committed files only, so uncommitted edits are excluded. It
establishes repeatability across two paths on the same runner; it does not
establish identical executables across operating systems, runner images,
linker versions, or machines. It also does not establish reproducibility of
model conversion or numerical inference. Those require separate evidence.
