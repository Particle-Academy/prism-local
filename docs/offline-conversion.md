# Offline conversion and reference outputs (design)

Status: design only. The converter, reference generator, and model loader do
not exist yet. This document describes the required evidence, not a claim of
reproducibility.

The engine and converter are Rust programs built without third-party crates.
The engine has no network code. Upstream source retrieval and reference
generation are separate explicit developer steps, never performed on load.
No dependency or build script may silently fetch models or tooling.

For each model, record the repository, exact immutable revision and SHA-256 of
every input (including configuration, tokenizer, weights, license and any
notice), source file sizes, converter source revision and toolchain, and
conversion recipe. Verify these before conversion. Re-check the pinned
revision's actual license file at conversion, accepting only MIT or Apache-2.0;
retain required license and notices with the converted artifact. Reject
unrecognised architectures, parameters, tensor names, shapes and dtypes.
The decider model selection is deferred until embedding measurements are
available. Each architecture requires its own explicit conversion and
validation rules; similar model names do not imply interchangeable weights.

Conversion must be deterministic: the same verified inputs, converter and
recipe produce identical bytes, independently checked in two clean builds.
Publish exact output digests only with the approved delivery process. Clients
will pin those digests; the engine must hash the exact immutable bytes it then
parses, avoiding replacement between verification and use. This is a design
requirement, not yet an implemented protection.

Compute reference outputs separately in a disposable, pinned environment.
Record environment image or equivalent immutable digest, package versions and
hashes, script source, CPU mode, input corpus digest, seeds and all numeric
settings. Commit only reference outputs and their provenance; the disposable
third-party environment is not a runtime or build dependency. Verify that a
second clean reference run reproduces the committed output or explicitly
record the observed nondeterminism before using it as a gate. Do not execute
unreviewed model code. Model and reference data licenses must permit the
intended use and distribution.

Reference checks must cover tokenizer IDs, unquantized intermediate values,
final embeddings and decision scores. Token IDs must agree exactly; numeric
quality against the upstream model and byte identity across engine platforms
are distinct gates. No reference result establishes cross-CPU engine identity
without exercising the engine on those CPUs and execution paths.
