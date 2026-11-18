# Fuzz entry points

The `plm`, `frame` and `json` binaries read one input from stdin and exercise the
respective bounded parsers. PLM computes a matching digest so a mutator can
reach the structural checks. Inputs over 1 MiB are ignored by the adapters;
this is a fuzz-driver limit, not a model-file size limit. Build with
`cargo build -p prism-local-fuzz --bins --locked`, then feed binaries from an
independently provisioned fuzzing tool. The repository adds no third-party
fuzzer dependency. This is a harness, not evidence that a fuzz campaign ran.

The current targets exercise framing, JSON, and the draft PLM container only.
Section contents, tokenizer and schema-compiler targets must be added when
those parsers exist. Malformed findings must become regression cases.
