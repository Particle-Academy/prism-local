# Stdio skeleton

The current sidecar accepts a sequence of four-byte big-endian length-prefixed
JSON frames over standard input and writes framed JSON-RPC responses to standard
output. The hard frame cap is 1 MiB. There is no network listener or download.
The process handles one frame at a time. It implements `hello`, `stats`, and
`shutdown`; all model and inference methods return method-not-found. JSON
batch requests are not supported. Invalid or truncated frames terminate the
stream. Invalid JSON gets a JSON-RPC parse error. Notifications receive no
response.

`hello` accepts optional `threads` and `deadlineMs` integers. `threads` must be
at least 1 and no more than the process's available parallelism; it is reserved
for future compute workers. A single fixed watchdog thread is additional to
that worker count. `deadlineMs` must be 100–300,000, defaulting to 30,000 ms
before and after `hello` unless set in `hello`. Settings cannot be changed by a
second hello. The deadline starts when the first header byte arrives and ends
when the response is flushed, so idle time between frames is not counted. An
expired in-flight request terminates the process with status 124; the parent
must restart it and enforce its own deadline when accepting responses. Bytes
already written cannot be retracted, and OS scheduling can delay termination
beyond the nominal limit.

The current skeleton starts no compute workers and loads no model. **Memory
budgeting is not implemented:** `maxMemoryMb` is rejected rather than
accepted as a promise the engine cannot enforce. Request allocation budgeting
must be implemented and tested before inference is enabled; a request budget
does not by itself provide an operating system RSS limit. Parsing has separate
fixed frame, nesting, node and string limits, and no request is silently
truncated.

The core contains checked allocation planning and fixed-size numeric buffers
that charge their actual vector capacity against a consumed plan. This does
not account for allocator overhead, transient allocation, stacks or unlisted
allocations; separate allocators have no shared limit. It is not connected to
all sidecar allocations or evidence that a request or process cap exists.
The sidecar reads each frame into a preplanned fixed byte buffer and refuses it
if that buffer's capacity exceeds 1 MiB. It parses JSON into a separate arena
whose node and decoded-text buffer capacities together are capped at 2 MiB.
The reader also exposes an admission callback after validating the declared
length and before allocation. These fixed limits are not a complete request
memory budget: responses, allocator overhead and stacks are not included.
See [request memory budgeting](request-memory-budget.md) for the remaining
accounting and validation work.

CI checks for direct `std::net` references in engine source. This lexical
check does not prove the absence of all network access: aliases, other platform
APIs and transitive code still require review. The current workspace has only
local path dependencies, and the sidecar contains no network implementation.
