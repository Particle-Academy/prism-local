# Fuzz entry points

Every target body lives in `src/targets.rs`. The `plm`, `frame`,
`frame_budgeted`, `json`, `json_write`, `json_decode`, `json_arena`,
`metadata`, `tensors`, `tensors_f16`, `vocab`, `provenance`, `sections`,
`quantized_sections` and `safetensors` binaries each read one input from stdin
and pass it to the target of the same name, so any external fuzzing tool can
drive them. PLM and section targets compute a matching digest so a mutator can
reach the structural checks. Inputs over 1 MiB are ignored by the adapters;
this is a fuzz-driver limit, not a model-file size limit, and it means size
caps of 1 MiB or more are covered by unit tests rather than by fuzzing.

`tensors_f16` exists because the `tensors` schema has only i8 and f32 tensors,
so f16 payload rules were unreachable from any target. Unit tests separately
apply deterministic bounded mutations to valid section seeds; those smoke
checks do not measure coverage. It pairs an i8 weight
matrix with a positive-normal f16 row scale and a finite-or-zero f16 vector.

## Coverage-guided engine

`engine/engine.rs` is an in-process, coverage-guided fuzzer built only on the
standard library and LLVM SanitizerCoverage (edge guards plus comparison
operands), which the pinned toolchain already provides. It adds no crate and
is not a workspace member:

```
bash fuzz/engine/build.sh               # instruments the parsers, builds target/fuzz-cov/engine
bash fuzz/engine/run.sh 120 target/fuzz-work
```

`run.sh` first requires the engine to find a planted canary (a crash behind a
magic header, a 32-bit constant and a length relation) within 60 seconds, so a
clean result means something. It then fuzzes every `corpus/<target>` for the
given seconds, at most two processes at once. It fails if the canary is missed
or if any target panics or has an input slower than one second; crashing and
slow inputs are written to `<work>/<target>/crashes`. The `Fuzz` workflow runs
this on Linux and Windows for every pull request and keeps the work directory
as an artifact.

On Windows, `PC_TABLE=1 bash fuzz/engine/build.sh` followed by
`target/fuzz-cov/engine <target> <corpus> <file-filter> lines` replays a
corpus and lists which source lines were fully, partly or never reached. A
line marked missed with one point on a `?` is usually an untaken error branch.

Do not run campaigns on a shared desktop; use the workflow.

## Corpus

`corpus/<target>/valid` contains synthetic seeds for each current target. A
workspace test checks that each seed reaches its target's successful parser
path; these are examples, not real model files or a complete fuzz corpus.
The PLM corpus also has a synthetic architecture-42 envelope to exercise the
generic structural parser; it does not represent a supported model.
The PLM, frame, tensor and f16 tensor corpora also include synthetic hostile
inputs; tests confirm they are rejected, while a mutator can explore nearby
cases. Use a separate writable working corpus when fuzzing and retain crashing
inputs as regression cases after review.

The current targets exercise framing, JSON, the draft PLM container, tensor
directory, metadata, vocabulary, provenance and composed section checks with small trusted test schemas. Additional
source safetensors inspection uses a separate converter-only target. Additional
section content, tokenization and schema-compiler targets must be added when
those parsers exist.
Malformed findings must become regression cases.
