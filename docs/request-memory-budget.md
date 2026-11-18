# Request memory budgeting (work in progress)

`maxMemoryMb` is currently rejected. The pieces in the core library are not an
enforced sidecar budget: they plan caller-declared payloads, provide optional
fixed-capacity numeric buffers and provide frame-length admission. The sidecar
uses a preplanned frame buffer capped at 1 MiB, but no runtime path accounts
for every live allocation yet.

Before accepting a memory setting, the engine needs one documented accounting
boundary and an audit covering at least resident model bytes, incoming frame,
decoded request, output, tensor workspaces, worker handles and worker stacks.
Each included payload must be charged by its actual capacity, with checked
arithmetic; overlapping lifetimes must be summed. Allocator bookkeeping,
runtime and OS overhead require explicit treatment, and a payload budget alone
must not be described as an operating-system RSS limit.

The bootstrap `hello` is read before its proposed budget is known. Its bounded
bootstrap allowance and the minimum accepted budget must therefore be explicit;
a budget too small to cover required live state must be refused. After hello,
read and validate the frame header, calculate a conservative complete plan
before allocating the body or dispatching work, and refuse an over-budget
request without truncation. The current JSON parser's maps, strings, vectors,
formatting and response copies are not tracked: they must be converted to
accounted storage or otherwise bounded with evidence before enabling the cap.

The runtime must enforce one in-flight request and prevent independent budget
objects from oversubscribing the same limit. Allocation and worker failures
must not leave charged resources reachable as a successful result. Model loads,
all compute paths, error responses and cancellation need the same accounting;
a successfully allocated output buffer alone is insufficient. The parent must
still enforce any desired OS process limit and deadline independently.

Before enabling `maxMemoryMb`, test exact-limit and one-byte-over cases,
overflow and allocation failures, large and malicious JSON, concurrent or
repeated calls, worker/thread overhead, error paths, cancellation and model
loads. Audit every allocation path and retain evidence from both supported
operating systems. Until then, rejection is the only implemented contract.
