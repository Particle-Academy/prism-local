# Stdio skeleton

The current sidecar accepts a sequence of four-byte big-endian length-prefixed
JSON frames over standard input and writes framed JSON-RPC responses to standard
output. The hard frame cap is 1 MiB. There is no network listener or download.
The process handles one frame at a time. It implements `hello`, `stats`, and
`shutdown`; all model and inference methods return method-not-found. JSON
batch requests are not supported. Invalid or truncated frames terminate the
stream. Invalid JSON gets a JSON-RPC parse error. Notifications receive no
response.

`hello` accepts optional `threads` and `maxMemoryMb` integer settings. Threads
must be at least 1 and no more than the process's available parallelism;
memory must be 64–4096 MiB. The settings cannot be changed by a second hello.
These are preliminary admission limits only: the current skeleton starts no
worker threads and loads no model. **A process-wide memory cap and model/request
allocation budgeting are not implemented.** They must be implemented and
tested before inference is enabled; the numeric option is not an operating
system RSS limit. Parsing has separate fixed frame, nesting, node and string
limits, and no request is silently truncated.

CI checks for direct `std::net` references in engine source. This lexical
check does not prove the absence of all network access: aliases, other platform
APIs and transitive code still require review. The current workspace has only
local path dependencies, and the sidecar contains no network implementation.
