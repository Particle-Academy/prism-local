# PLM1 container draft

Status: **incomplete and not an interoperable model format**. The current
parser checks an authenticated container envelope, with separate draft checks
for BERT metadata, tensor directories, WordPiece vocabulary and provenance.
These are not yet integrated into a complete BGE validator, and no model can
be accepted for inference. Quantization relationships and future decoder
architectures remain unspecified. Do not produce or distribute model files
based on this draft.

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

## Draft BERT architecture metadata

The BERT architecture section is exactly 64 bytes: eight u32 values in
order for layer count, hidden size, attention-head count, intermediate size,
vocabulary size, maximum positions, token-type count and the IEEE-754 f32
bits for layer-norm epsilon; all remaining bytes are zero. Values must exactly
match a separately trusted schema, fit hard limits, have hidden size divisible
by nonzero head count, and have a positive normal finite epsilon no larger
than one. Other architectures require their own metadata specification.
Metadata acceptance alone does not validate the model or its weight layout.

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

The separate payload checker requires a trusted per-tensor rule: any i8,
finite normal-or-zero f16/f32, or strictly positive normal f16/f32. It reads
little-endian float bits directly and refuses infinities, NaNs and subnormals;
the positive rule also refuses signed zero and negative values. A dtype and
rule mismatch is refused. These checks do not establish scale relationships or
quantization correctness.

This draft does not define the BGE tensor schema or quantization scale layout.
A directory or payload passing these checks is not a loadable model.

## Draft WordPiece vocabulary section

A 32-byte header contains u32 vocabulary count, u32 normalization flags,
five u32 special-token IDs in the trusted schema's order, and one zero u32.
Each vocabulary entry then contains a little-endian u16 byte length and that
many UTF-8 bytes. The section ends immediately after the final token. A trusted
architecture schema supplies the exact expected count, flags, special IDs and
special spellings. IDs must be in range and distinct; each special spelling
must match its entry. All tokens must be nonempty, unique valid UTF-8 of at
most 1,024 bytes. The section is limited to 16 MiB and 131,072 entries.

The checker does not implement or validate the behavior of normalization or
WordPiece tokenization, and no BGE vocabulary schema is registered yet. A
passing vocabulary section does not make a model valid or loadable.

## Draft provenance section

The section starts with a u32 source count (1–64). Six nonempty UTF-8 strings
follow, each prefixed by a little-endian u16 byte length: repository, immutable
revision, publisher, converter version, complete license text and copyright
line. Each source then has a similarly encoded name followed by 32 SHA-256
bytes. Sources must be sorted by unique name and the section must end exactly
after the final digest; the total section cap is 1 MiB.

The checker compares each value to a separately trusted manifest, including
the exact license and copyright line, and requires the license text to contain
that line. A matching provenance section does not verify the source files or
the origin of the trusted manifest. Conversion must separately verify source
bytes, license evidence and metadata/NOTICE requirements.
