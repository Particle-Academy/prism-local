# Request memory budgeting (work in progress)

`maxMemoryMb` is currently rejected. The pieces in the core library do not yet
provide a configurable or process-wide limit: they plan caller-declared
payloads, provide optional fixed-capacity numeric buffers and provide
frame-length admission. The sidecar
uses a preplanned frame buffer capped at 1 MiB and a separate JSON arena capped
at 2 MiB, with a response buffer capped at 1 MiB. Those use one request plan,
but the runtime does not account for every live allocation yet.

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
request without truncation. Any additional request storage must be accounted
for or otherwise bounded with evidence before enabling the configurable cap.

The runtime handles one request at a time and partitions a single 4 MiB plan
for frame, parser and response buffers.
Any future negotiated limit must also prevent independent budget objects from
oversubscribing that limit. Allocation and worker failures
must not leave charged resources reachable as a successful result. Model loads,
all compute paths, error responses and cancellation need the same accounting;
a successfully allocated output buffer alone is insufficient. The parent must
still enforce any desired OS process limit and deadline independently.

JSON string escaping calculates its exact output length and writes into fixed
storage without growth. The shared string decoder can also write to fixed
storage, rejecting invalid strings or insufficient space. These functions do
not account for allocator, runtime or stack overhead.

The sidecar's JSON arena stores nodes and decoded strings in fixed buffers
charged to a precomputed plan. It sorts object members in place and rejects
duplicate decoded keys. The input and response buffers use separate portions of
the same plan; recursion stack and other runtime state remain separate.
End-to-end resource behavior and deadline
handling still need review and coverage-guided fuzzing.

Before enabling `maxMemoryMb`, test exact-limit and one-byte-over cases,
overflow and allocation failures, large and malicious JSON, concurrent or
repeated calls, worker/thread overhead, error paths, cancellation and model
loads. Audit every allocation path and retain evidence from both supported
operating systems. Until then, rejection is the only implemented contract.
