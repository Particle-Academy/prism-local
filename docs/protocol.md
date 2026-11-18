# Stdio skeleton

The current sidecar accepts a sequence of four-byte big-endian length-prefixed
JSON frames over standard input and writes framed JSON-RPC responses to standard
output. The hard frame cap is 1 MiB. There is no network listener or download.
The process handles one frame at a time. It implements `hello`, `stats`, and
`shutdown`; all model and inference methods return method-not-found. JSON
batch requests are not supported. Invalid or truncated frames terminate the
stream. Invalid JSON gets a JSON-RPC parse error. Notifications receive no
response.

`hello` accepts optional `threads`, `deadlineMs`, and `maxMemoryMb` integers.
`threads` must be at least 1 and no more than the process's available
parallelism; it is reserved for future compute workers. A single fixed watchdog
thread is additional to that worker count. `deadlineMs` must be 100–300,000,
defaulting to 30,000 ms before and after `hello` unless set in `hello`.
Settings cannot be changed by a second hello. The deadline starts when the
first header byte arrives and ends when the response is flushed, so idle time
between frames is not counted. An expired in-flight request terminates the
process with status 124; the parent must restart it and enforce its own
deadline when accepting responses. Bytes already written cannot be retracted,
and OS scheduling can delay termination beyond the nominal limit.

The current skeleton starts no compute workers and loads no model.
`maxMemoryMb` limits **managed payload capacity**, measured in MiB
(1,048,576 bytes), and defaults to 1,024 MiB. The minimum accepted value is
4 MiB, the fixed bootstrap and current request reservation. Values that cannot
be represented safely are refused. `hello` and `stats` report the setting and
`"memoryScope":"managedPayload"`. The managed payload covers the frame, JSON
arena and response buffers; they are planned together before each request.
It is not an operating-system RSS limit and excludes allocator bookkeeping,
transient allocator over-allocation, standard I/O and runtime/OS memory,
watchdog and parser stacks, and other non-payload overhead. A caller needing an
RSS limit must enforce it independently. Future model memory and compute
workspaces must be included in the same plan before model methods are enabled.
Parsing has separate fixed frame, nesting, node and string limits, and no
request is silently truncated.

The core contains checked allocation planning and fixed-size numeric buffers
that charge their actual vector capacity against a consumed plan. Independent
plans have no shared limit; the sidecar uses one plan per in-flight request.
The sidecar reads each frame into a preplanned fixed byte buffer and refuses it
if that buffer's capacity exceeds 1 MiB. It parses JSON into a separate arena
whose node and decoded-text buffer capacities together are capped at 2 MiB.
Responses use a fixed buffer capped at 1 MiB. These three allocations use
disjoint portions of one 4 MiB request payload plan.
The reader also exposes an admission callback after validating the declared
length and before allocation. These fixed limits are not an operating-system
RSS limit: allocator overhead, runtime and stack memory are not included.
See [request memory budgeting](request-memory-budget.md) for the remaining
accounting and validation work.

CI sends a synthetic `hello`, `stats` and `shutdown` sequence to the debug and
release sidecars, checks the complete framed responses against fixed expected
bytes, and compares those bytes across Windows, GNU Linux and static musl
Linux. This checks the current skeleton only; it is not inference evidence.

CI checks for direct `std::net` references in engine source. This lexical
check does not prove the absence of all network access: aliases, other platform
APIs and transitive code still require review. The current workspace has only
local path dependencies, and the sidecar contains no network implementation.
