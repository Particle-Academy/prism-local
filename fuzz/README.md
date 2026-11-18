# Fuzz entry points

The `plm`, `frame`, `frame_budgeted`, `json`, `json_write`, `metadata`, `tensors`, `vocab`, `provenance`, `sections`, `quantized_sections` and `safetensors` binaries read one input from stdin and exercise the
respective bounded parsers. PLM computes a matching digest so a mutator can
reach the structural checks. Inputs over 1 MiB are ignored by the adapters;
this is a fuzz-driver limit, not a model-file size limit. Build with
`cargo build -p prism-local-fuzz --bins --locked`, then feed binaries from an
independently provisioned fuzzing tool. The repository adds no third-party
fuzzer dependency. This is a harness, not evidence that a fuzz campaign ran.
Unit tests also apply deterministic bounded mutations to valid section seeds;
those smoke checks do not measure coverage or demonstrate a fuzzing plateau.

`corpus/<target>/valid` contains synthetic seeds for each current target. A
workspace test checks that each seed reaches its target's successful parser
path; these are examples, not real model files or a complete fuzz corpus.
The PLM, frame and tensor corpora also include synthetic hostile headers;
tests confirm they are rejected, while a mutator can explore nearby cases.
Use a separate writable working corpus when fuzzing and retain crashing inputs
as regression cases after review.

The current targets exercise framing, JSON, the draft PLM container, tensor
directory, metadata, vocabulary, provenance and composed section checks with small trusted test schemas. Additional
source safetensors inspection uses a separate converter-only target. Additional
section content, tokenization and schema-compiler targets must be added when
those parsers exist.
Malformed findings must become regression cases.
