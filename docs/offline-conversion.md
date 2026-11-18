# Offline conversion and reference outputs (design)

Status: design and partial source-inspection foundations. A complete converter,
reference generator and model loader do not exist yet. This document describes
the required evidence, not a claim of reproducibility.

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
the model card at the pinned model revision; (2) the linked license text at its
latest file change no later than model release, pinned to an immutable commit
with its Git blob SHA and SHA-256; and (3) the exact copyright line in the project
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
An explicit download on 2026-11-18 had 133,466,304 bytes and matched that
SHA-256. This is an observation of a local source snapshot, not an output
`.plm` hash or authorization to convert. The converter must independently
verify the actual bytes and license evidence each time it converts.

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
incomplete evidence manifest: the converter, conversion-time input verification
and metadata/NOTICE enforcement do not exist yet and it does not authorize a model
conversion or distribution.

The verified source snapshot's safetensors header describes 200 tensors: 199
F32 tensors and one I64 `embeddings.position_ids` buffer with shape `[1, 512]`.
The buffer contains the integers 0 through 511 in order. The header's ranges
were checked for contiguous, non-overlapping coverage of the data and lengths
consistent with the listed shapes. There are also two F32 pooler tensors.
The pinned pooling configuration requests CLS-token pooling; the converter
must explicitly define and validate how the position-ID and pooler tensors
are handled, based on reference behavior. These observations are not a complete
tensor schema, quantization recipe, converter or inference validation.

The separate converter library can inspect an immutable, already-buffered
safetensors input after comparing its SHA-256 with a trusted digest. It checks
F32/I64 tensor names, shapes, ranges and exact metadata against a trusted schema;
its returned slices refer to those same authenticated bytes. A local check
accepted all 200 BGE entries against an independently written expected source
inventory. The library does not verify tensor values, license evidence or model
semantics, read files, produce PLM files, or authorize conversion. Callers must
enforce a size limit before buffering; future large-file handling needs a
separate same-bytes review. The sidecar does not depend on this library.

Evidence: [pinned model card](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/README.md),
[pinned source weights](https://huggingface.co/BAAI/bge-small-en-v1.5/blob/5c38ec7c405ec4b44b94cc5a9bb96e735b38267a/model.safetensors),
and [pinned upstream license](https://github.com/FlagOpen/FlagEmbedding/blob/c086741f5e117b7b8ce1745ea00b6c262f281a01/LICENSE).
The [model history](https://huggingface.co/BAAI/bge-small-en-v1.5/commits/main)
and [license-file history](https://github.com/FlagOpen/FlagEmbedding/commits/master/LICENSE)
show the dates used above.

Conversion must be deterministic: the same verified inputs, converter and
recipe produce identical bytes, independently checked in two clean builds.
Publish exact output digests only with the approved delivery process. Clients
will pin those digests; the engine must hash the exact immutable bytes it then
parses, avoiding replacement between verification and use. This is a design
requirement, not yet an implemented protection.

## Conversion procedure to implement

1. A developer explicitly supplies a local source snapshot, a reviewed pinned
   manifest, a converter build and a destination. Conversion performs no
   retrieval. Reject an incomplete manifest, unrecognized recipe or missing
   exact input. Do not discover model files by executing source code.
2. Read each required input into an immutable bounded buffer, check its exact
   length and SHA-256 against the trusted manifest, and parse those same bytes.
   The manifest names every input and the converter and toolchain revisions;
   a digest copied from the untrusted input itself is not a trust anchor.
   Large inputs need a separately reviewed bounded-storage strategy that keeps
   verification and conversion tied to the same bytes.
3. Recheck model-card license evidence and the full license bytes, including
   the pinned historical evidence when no repository license exists. Verify
   the copyright line in NOTICE and in the output metadata. Refuse on a missing
   or mismatched item. Validate the architecture, tokenizer configuration,
   exact required tensor names, shapes, dtypes and value constraints against
   a trusted recipe; reject unexpected or duplicate entries.
4. Apply an explicitly versioned quantization and serialization recipe with
   fixed rounding, ordering, alignment and padding. No timestamps, random IDs,
   filesystem order or machine paths may affect the artifact. Check allocation
   arithmetic before allocating, and report failures without publishing partial
   output. Output to a new temporary file and only place the result at the
   requested destination after validation succeeds.
5. Repeat from independently prepared clean source and build directories, then
   compare the complete artifact bytes and SHA-256. Re-parse each artifact with
   the complete validator, record the evidence, and seek delivery approval
   before distributing it. A match is reproducibility evidence for the tested
   environments, not proof that the model is safe or numerically accurate.

The precise input format, complete tensor mapping, quantization recipe,
buffering strategy and safe destination behavior must be specified and
implemented before this procedure can run. The existing draft PLM validator
does not authorize conversion or provide the complete re-parse in step 5.

Compute reference outputs separately in a disposable, pinned environment.
Record environment image or equivalent immutable digest, package versions and
hashes, reference script digest, CPU mode, input corpus digest, seeds and all
numeric settings. Retain the reviewed script outside the repository with its
digest so a fresh disposable environment can repeat the run, and commit only
reference outputs and their provenance; the disposable third-party environment
is not a runtime or build dependency. Verify that a
second clean reference run reproduces the committed output or explicitly
record the observed nondeterminism before using it as a gate. Do not execute
unreviewed model code. Model and reference data licenses must permit the
intended use and distribution.

Reference checks must cover tokenizer IDs, unquantized intermediate values,
final embeddings and decision scores. Token IDs must agree exactly; numeric
quality against the upstream model and byte identity across engine platforms
are distinct gates. No reference result establishes cross-CPU engine identity
without exercising the engine on those CPUs and execution paths.
