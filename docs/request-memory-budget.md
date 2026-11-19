# Request memory budgeting (skeleton)

`maxMemoryMb` limits managed payload capacity in MiB, not process RSS. The
current skeleton uses one precomputed request plan: up to 1 MiB for the frame,
2 MiB for the JSON arena and 1 MiB for the response. It rejects limits below
4 MiB or values above the process startup budget; the default startup budget is
1,024 MiB. See [the protocol](protocol.md) for the additional process cap.
The core provides fixed-capacity numeric buffers and frame-length admission.

The managed payload boundary includes these three buffers and charges their
actual vector capacities. It excludes allocator bookkeeping, transient
allocator over-allocation, standard I/O and runtime/OS memory, watchdog and
parser stacks, and other non-payload overhead. It is not an operating-system
RSS limit. No model is currently loaded and no compute worker is started.

The bootstrap `hello` is read before its proposed budget is known, using the
same fixed 4 MiB reservation. A proposed limit below that reservation is
refused. After hello, the sidecar verifies the complete 4 MiB payload plan
against the selected limit before reading each frame. It refuses over-budget
requests without truncation.

The runtime handles one request at a time and partitions its plan into
disjoint frame, parser and response scopes. Independent plans have no shared
limit. Allocation and worker failures
must not leave charged resources reachable as a successful result. Model loads,
all compute paths, error responses and cancellation need the same accounting;
a successfully allocated output buffer alone is insufficient. The parent must
still enforce its own deadline independently.

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

Before enabling model methods, include resident model bytes, tensor workspaces
and every additional managed payload in the same checked plan. Audit every
allocation path and test exact-limit and one-byte-over cases, overflow,
allocation failures, large and malicious JSON, repeated calls, worker/thread
overhead, error paths, cancellation and model loads on both supported OSes.
The operating-system cap is additional to this accounting; its startup
allowances and mapped-weight behavior must be extended and tested before model
loading. Parent-imposed limits and deadlines can add defence in depth.
