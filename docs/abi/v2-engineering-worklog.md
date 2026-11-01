# ABI/FFI foundation engineering evidence

Status: **in progress; not a stable-release declaration**. Session date: 2026-10-11.
The supplied attachment ends at the incomplete bullet `Direct ABI` in pass 4.
The preceding requirements remain the scope; this log does not reduce them.

## Discovery and preservation

The authoritative project directory is `C:/Users/Conner Adams/Desktop/404connernotfound/TinyOne`.
Its Rust Git repository is the nested `TinyOne` directory. Siblings include
Authority, Arduino-Coprocessor, TPack, and Web; those projects are untouched.
The repository is on `main`, commit `523242d95eba2a4787ceefd5ce34f341f649a647`.
The relocated `TinyOne-v2-abi` worktree belongs to the same repository and
branch `v2/abi-binary-protocol`, at the same commit. Its `.git` pointer and
registered worktree path still name `C:/Users/conne/Desktop/...`.

Read-only inspection succeeded with:

```powershell
git --git-dir=.git/worktrees/TinyOne-v2-abi --work-tree='C:/Users/Conner Adams/Desktop/404connernotfound/TinyOne-v2-abi' status --short
git worktree list
git log -5 --oneline
```

No worktree metadata was repaired. The v2 worktree has no tracked changes;
its additions are `tinylang_v2/`, four v2 ABI specifications, and a CI workflow.
The main tree has extensive uncommitted compiler, runtime, allocator, source
location, decorator, profiling, and testing changes. They are preserved.
In particular its modified `ffi.rs` and ABI soundness tests were not replaced.
The shared committed v1 sources are the same revision; main's uncommitted
changes are absent from the v2 worktree.

The isolated 55-file v2 crate and four specifications were copied into main.
Only the imported manifest and lockfile differ from those originals so far;
`evidence/import-hashes.csv` records SHA-256 comparisons. The source worktree
remains untouched. File inventories exclude Git internals, target directories,
and node_modules; generated/ignored files are not source inventory claims.

Mechanical build repairs, exempt from three-way architectural prototyping:

1. Change the stale dependency `../TinyOne` to `../crates/tinyone_core`.
2. Add `[workspace]` to preserve the crate's intended independent workspace.
3. Cargo refreshes its old dependency lockfile against the actual runtime.
4. Add a crate-local stable `rustfmt.toml` matching imported formatting rather
   than reformatting it to the parent workspace's nightly settings.

## Inventory

| Surface | Current evidence and gaps |
| --- | --- |
| v1 | Frozen ABI version 1; JSON compile, lex, run, artifact, JIT listing; owned C strings; guarded panic/error envelopes; execution subprocess sandbox. |
| v2 C interface | Six exports: version, runtime create/destroy, buffer acquire/release, call. Explicit C buffer descriptors and opaque u64 handles. |
| Binary interchange | TLV2 32-byte header, little-endian fields, canonical 16-byte value headers, bounded frames/nesting, strict padding and UTF-8. |
| Values | Null, bool, sized signed/unsigned integers, f32/f64 bits, bytes, UTF-8, handles, enums, slices, sequences. Validation/echo exists; general VM argument/result conversion remains unfinished. |
| Ownership | Borrowed call buffers are not retained; owned buffers require release; runtime destroy returns BUSY with live children; registry checks runtime, kind, generation. |
| Dispatch | NOP, ABI_INFO, ECHO, PROGRAM_LOAD, VM_CREATE, HANDLE_RELEASE implemented. VM_INVOKE validates its schema but returns UNKNOWN_OPCODE. |
| Artifacts | TLBC profile 0 loads verified main-only code without metadata pools. Existing compiler's complete binary artifact format is separate; full compiler-produced program interoperability is unproven. |
| VM sessions | Retain verified program and requested limits. No execution memory or invoke path yet. Current VM consumes itself on run and exposes no fuel argument. |
| Bindings | C linked smoke, Python ctypes, Rust example, Node wire codec plus process bridge. Node is not a native in-process FFI binding. |
| Tests | Golden frames/artifacts, mutation tests, schema/capacity checks, stale/cross-runtime/kind/lifetime tests; fuzz targets present but not executed yet. |
| Compatibility | v1 sources retained. Separate v2 DLL also exports inherited v1 and allocator symbols; export policy needs review. |

## Baseline execution

Host: x86_64 Windows MSVC; rustc 1.99.0 (`b940084d7`, LLVM 23.1.1),
MSVC 14.44.35207, Node 26.8.2, Python 3.13.15 (uv-managed).

The original v2 command failed before compilation:

```text
cargo test --manifest-path <v2-worktree>/tinylang_v2/Cargo.toml --target-dir target/abi-v2-original
error: current package believes it's in a workspace when it's not
```

The old `../TinyOne` directory contains only target artifacts and no manifest.
Thus there is no runnable original baseline at that path. Measurements below
refer to the minimally repaired import against the current runtime.

| Check | Observed result |
| --- | --- |
| `cargo test -p tinylang --test abi_api_soundness` | 42 passed; build 32.35 s, tests 5.15 s. |
| `cargo test --manifest-path tinylang_v2/Cargo.toml --target-dir target/abi-v2` | 25 passed (2 unit + 23 integration); initial build 27.36 s. |
| `cargo clippy --manifest-path tinylang_v2/Cargo.toml --target-dir target/abi-v2 --all-targets -- -D warnings` | Passed, including baseline benchmark. |
| `cargo fmt --manifest-path tinylang_v2/Cargo.toml -- --check` | Initially failed inherited formatting differences/nightly settings; passes with crate-local stable configuration. No bulk formatting applied. |
| `cargo test --release --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/abi-v2` | All 25 tests pass in release too. |
| `node --test tinylang_v2/bindings/node/wire.test.mjs` | 4 passed. Sandbox process-spawn denial resolved through approved escalation. |
| `node tinylang_v2/bindings/node/programs.mjs target/abi-v2/debug/examples/abi_stdio_bridge.exe` | Three artifacts load/create/release successfully; invocation explicitly unavailable. |
| `python tinylang_v2/bindings/python/smoke.py target/abi-v2/debug/tinylang_v2.dll` | Passed using uv-managed interpreter's absolute path. |
| `python tinylang_v2/bindings/python/programs.py target/abi-v2/debug/tinylang_v2.dll` | Three artifacts load/create/release successfully; invocation explicitly unavailable. |
| `target/abi-v2/debug/examples/interop_rust.exe` | Three artifacts decode, execute through Rust API, load, create sessions; outputs empty, 42, 42. This is not foreign invocation evidence. |
| C11 `/W4 /WX` linked `bindings/c/smoke.c` | Compiled and executed successfully through MSVC developer PowerShell. |
| `dumpbin /exports target/abi-v2/debug/tinylang_v2.dll` | Six v2, nine v1, five allocator symbols. Debug DLL file size 3,747,328 bytes. |

Build times above are single observations, not performance comparisons. The
two crates currently resolve different dependency versions; no timing claim
between them is justified. DLL file size excludes its separate PDB and is not
peak memory (PE virtual data includes allocator storage).

Baseline dispatch measurements and raw samples are in
[v2-baseline-performance.md](v2-baseline-performance.md). The initial research
notebook is [v2-research.md](v2-research.md). No architectural candidate has
been selected, and no full-foundation completion claim is made.

## Remaining completion gates

Update 2026-10-13: completed the three-candidate
[runtime/object generation experiment](v2-handle-lifetimes.md), integrated
slot retirement, and passed 28 release tests plus Clippy. This closes the
runtime/object generation-wrap defect, not the entire handle-safety gate.
Owned-buffer tokens still need an independent audit. Earlier inventory and
baseline tables above describe the original import, not current completion.

All original requirements remain active. In particular:

Heap-conversion prerequisite: added the [Rust heap adapter](v2-heap-adapter.md)
that forwards owned string/buffer/array imports, snapshots and explicit release
to the current VM heap. Three new normal-profile tests pass, including compiled
identity calls, nested/stale references and heap limits. The existing 111
persistent/fuel/parity/v1-ABI regression tests and core Clippy also pass. This
does not solve foreign handle provenance or retained-object ownership; the
three-way invocation comparison still needs heap cases and result encoding.
The three heap-adapter tests subsequently passed in release too, along with
all 33 v2 release tests; the manual registry benchmark remains ignored by
default. The release build processes completed successfully.

Update 2026-10-13: integrated the
[instruction-budget prerequisite](v2-instruction-budget.md) after three
accounting-kernel candidates. The existing interpreter now offers
`VM::run_report_with_fuel`, with shared accounting across nested calls.
Four new tests plus 59 runtime-parity, 42 v1 ABI and 28 v2 release tests pass.
This does not enable VM_INVOKE or persistent execution yet.

Update 2026-10-16: completed the [persistent session ownership
comparison](v2-session-execution.md). Direct ownership, mutex ownership and
worker-thread ownership compile and pass equivalent stateful fixtures. Direct
ownership is integrated, including nonconsuming execution and validated heap
and call-depth limits. Six persistent-VM tests pass in debug and release;
the regression and v2 checks listed in that report pass. VM_INVOKE remains
unimplemented pending safe leasing, export mapping and value conversion.

The same pass corrected status-detail domains: INVALID_ARTIFACT and VM_TRAP
encode domain 2 as specified; protocol failures retain domain 1. Existing
artifact/opcode tests now check domain, detail code and absent fault offset.
All 28 v2 release tests, Clippy with warnings denied and formatting pass after
the correction. This is a mechanical contract repair, exempt from the
three-prototype requirement; it makes no performance claim.

Follow-up: integrated the selected direct session ownership into registry
execution with explicit running/failed states and panic quarantine. Execution
does not hold the registry mutex; running sessions reject release and duplicate
entry with BUSY. Three new tests cover reentrancy, concurrent release and panic
isolation. All 31 v2 release tests pass. An optional seven-sample integrated
Rust-path benchmark is recorded in the session report. Foreign dispatch and
argument/result conversion remain open; this is not an end-to-end FFI claim.

Compiler interoperability follow-up: PROGRAM_LOAD format 2 now delegates to
the existing `VerifiedProgram::from_binary_artifact` loader. Format 1 remains
TLBC profile 0; unknown format IDs are rejected at schema parsing. This is a
mechanical adapter to the authoritative existing decoder, exempt from new
codec prototyping: no binary layout, verification algorithm, or authority
policy was replaced. No performance superiority is claimed. Compiler format
version 5 and the existing 8 MiB bound remain independent of TLV2 framing.

The new C-boundary test covers load/release, capacity preflight, wrong format,
truncation, unknown compiler version, trailing bytes and source-text rejection.
A separate adapter/runtime test executes decoded functions using string and
array pools, and proves serialized root policy cannot authorize filesystem
access. These tests do not demonstrate foreign invocation or all metadata
variants; structured/export conversion and cross-language execution are still
required.

Invocation comparison follow-up: implemented and measured the [scalar phase
of tagged, packed and argument-stack candidates](v2-invocation-values.md).
All execute the same compiler-produced two-argument function and pass width,
order, rejection/recovery tests plus Clippy. Seven warmed samples, build times
and executable sizes are recorded. No winner is selected: heap-backed values,
result conversion and the full foreign call path remain unimplemented.

- Finish authoritative implementation research and record tradeoffs.
- Design, implement, test, and measure three isolated candidates for each
  substantial change before selecting and integrating it.
- Complete actual foreign invocation, arguments/results, resource limits,
  compiler/artifact interoperability, and ownership/error semantics.
- Verify threading, reentrancy, feature negotiation, widths/layouts and export
  policy; expand negative/adversarial tests and run fuzzing.
- Measure equivalent baseline/A/B/C release workloads, allocations, copying,
  memory, initialization, compilation/linking cost and binary sizes where
  meaningful. Unmeasured values must remain unavailable.
- Repair documentation/tooling paths; integrate CI only
  when its commands match the integrated tree.
- Run post-integration regression and foreign-language tests, compatibility
  checks and final requirement-by-requirement audit. Linux/macOS/32-bit checks
  remain unexecuted, not implicitly covered by Windows x64 results.
