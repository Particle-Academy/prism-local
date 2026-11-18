# PLM1 container draft

Status: **incomplete and not an interoperable model format**. The current
parser checks only the authenticated container envelope. It does not accept a
model for inference. Tensor schemas, architecture metadata, tokenizer,
provenance contents and future decoder architectures are not yet specified or
validated. Do not produce or distribute model files based on this draft.

All integers are unsigned little-endian. A consumer obtains an expected SHA-256
from a trusted pin and hashes the exact immutable byte buffer it will inspect.
A match establishes only byte identity, not model validity. This draft requires
the entire file in one buffer; memory-mapped files or streaming must not be
introduced without a separate same-bytes and resource-cap analysis.

The fixed 64-byte header is: magic `PLM1` at 0, u16 format version `1` at 4,
u16 architecture `1` (BGE encoder) at 6, u64 exact file length at 8, u64
section-table offset at 16, u32 section count at 24, and zero bytes at 28–63.
Other architecture identifiers are reserved and refused until specified. A
future architecture can define recurrent layer weights and state requirements
without treating them as BGE tensors. Runtime state is not persisted in a
weight file by this envelope definition; this does not constrain its future
representation.

The section table begins at a 64-byte-aligned offset and contains exactly five
32-byte records, strictly ordered by type. Record fields are u32 type, u32
zero, u64 offset, u64 length, and u64 zero. Types 1 through 5 are respectively
architecture metadata, tensor directory, tokenizer, provenance, and tensor
data. Each section is nonempty, 64-byte aligned, fully inside the file and
disjoint from every other section, the header and the section table. Reject
unknown types, duplicate or missing types, overflows, gaps inside the table,
nonzero reserved fields, truncation and lengths that do not match the file.
Unused bytes between sections have no semantics and are not parsed.

The envelope parser never allocates in proportion to attacker-provided counts
and uses checked arithmetic and checked integer conversions. Structural success
is not validation of any section. Until each section and its relationships are
validated, no caller may load or use weights, token IDs, metadata or template
content for inference. The format may change while those rules are developed.

## Draft tensor directory

The directory checker accepts a trusted, code-defined architecture schema;
schema selection from model metadata is not implemented. It requires exactly
one 40-byte record for each schema tensor, sorted by strictly increasing u32
tensor ID. A record consists of u32 ID, u8 dtype (1 = i8, 2 = f16, 3 = f32),
u8 rank (1–4), two zero bytes, four u32 dimensions (nonzero within rank and
zero afterward), u64 data-section-relative offset, and u64 byte length. All
dimensions and dtypes must exactly equal the trusted schema. Offsets are
64-byte aligned. Checked shape products multiplied by dtype size must equal the
length; the range must fit the data section and not overlap any other tensor.
The checker accepts at most 1,024 tensors and rejects duplicates or omissions.

This draft does not define the BGE tensor schema, quantization scale layout,
or payload validation. A directory passing these checks is not a loadable model.
