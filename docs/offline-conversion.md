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
revision's actual license file at conversion where present, accepting only
MIT or Apache-2.0. For a model whose repository has no license file, conversion
is refused unless the manifest records all three: (1) the license field from
the model card at the pinned model revision; (2) the linked license text at a
commit that existed no later than the model's release, with its Git blob SHA
and SHA-256; and (3) the exact copyright line from that license in the project
NOTICE and the converted model's metadata. Model publisher is a separate field.
The converter must independently verify these records and the bytes at
conversion, and refuse any missing or mismatched item. Retain the full license
text and all required notices with the converted artifact. Reject
unrecognised architectures, parameters, tensor names, shapes and dtypes.
The decider model selection is deferred until embedding measurements are
available. Each architecture requires its own explicit conversion and
validation rules; similar model names do not imply interchangeable weights.

The proposed BGE source is `BAAI/bge-small-en-v1.5` at revision
`5c38ec7c405ec4b44b94cc5a9bb96e735b38267a`. At that revision Hugging Face
reports `model.safetensors` SHA-256
`3c9f31665447c8911517620762200d2245a2518d6e7208acc78cd9db317e21ad`.
This is a source-host metadata observation, not a locally verified download or
an output `.plm` hash. Other inputs, actual file bytes and license evidence
still need independent verification before conversion.

The pinned model card declares MIT and links the FlagEmbedding MIT license,
but the pinned model repository's root listing has no separate LICENSE file.
The linked GitHub `master` URL is mutable. Its license-file history identifies
the introduction at FlagOpen/FlagEmbedding commit
`c086741f5e117b7b8ce1745ea00b6c262f281a01`, dated 2023-08-03, precedes the
model repository's first upload on 2023-09-12. The license's Git blob SHA-1 is
`360931513aa6c02f933a403202afa99ac2c5bc88` and its 1,065 downloaded bytes
hashed to SHA-256
`587a673933425dbc36ec61268d3b954051b2d3ef3c9b322ede357976055ffdd5`.
The pinned model card declares MIT and links that upstream license path, but
conversion must verify all evidence and input bytes again. Do not silently
treat a license tag as a complete redistribution notice.

The proposed values live in `models/bge-small-en-v1.5.toml`. That file is an
incomplete evidence manifest: the converter, source-weight verification and
metadata/NOTICE enforcement do not exist yet and it does not authorize a model
conversion or distribution.

Evidence: [pinned model card](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/README.md),
[pinned source weights](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/model.safetensors),
and [pinned upstream license](https://github.com/FlagOpen/FlagEmbedding/blob/c086741f5e117b7b8ce1745ea00b6c262f281a01/LICENSE).

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
