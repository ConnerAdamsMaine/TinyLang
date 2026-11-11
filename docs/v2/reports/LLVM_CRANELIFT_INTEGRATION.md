# LLVM and Cranelift integration

Status: **Cranelift and LLVM scalar object execution tested on Windows
x86-64; full-language/native Rust LLVM integration remains partial**.

Cranelift is optional behind `--features cranelift`. `tinylang_core`
declares the 0.125 family; the root workspace lockfile records the resolved versions.
It consumes the same checked-i64 IR as ASM; no independent language parser
or implicit interpreter fallback is permitted.

LLVM lowering emits IR for the same function profile and, since this
milestone, for the whole call-closure **program profile**
(`llvm/program.rs::emit_program_llvm_ir`). The program emitter mirrors the
Cranelift program ABI exactly: one shared invocation context (depth@0 u64,
trap@8 u32, fuel@16 u64, bank@24 ptr, pc@32 u64), a uniform frame bank
addressed by call depth, the same trap tags (1 overflow, 2 division by zero,
3 fuel exhausted, 4 call-stack overflow) and the same argument order and
trap propagation. It emits every closure member as
`define i64 @tinylang_program_N(ptr %context)`; the header comment documents
the entry symbol, stride and depth limit. The checked binary/floor-division
lowering is shared with the leaf profile through the `TrapSink` trait, so
trap categories cannot drift between profiles. The CLI escalates a leaf
`CALL` rejection to program emission instead of reporting a gap.

`--backend llvm` means **IR emission**, not execution or a claim of a
linked LLVM JIT. Actual LLVM verification, object compilation and host
execution require an available LLVM toolchain and are reported separately
from emitted text. The llvmlite harness (`test_llvm_execution.py`) executes
the actual emitted program IR through MCJIT and checks parity with the real
VM: recursion and zero-argument call results, trap tags 1/2/4, the
shared-fuel budget boundary (minimum-1 traps with tag 3, minimum succeeds)
and RX-only generated pages. Exact per-fuel equality with the interpreter is
covered by the Rust program-parity suites.

Default builds select no Cranelift dependencies for compilation. Requesting
Cranelift without the feature returns an explicit availability error.
`--backend interpreter` is a deliberate correct alternative, not a silent
fallback.

Cranelift emits actual host instructions, checked arithmetic, floor division,
control flow and instruction fuel. A retained custom memory owner publishes
code RX and releases storage on drop. Its Windows unit test verified initial
RW `0x04`, final RX `0x20` and deallocation. Five external-backend tests passed
with the feature (four real Cranelift differential tests plus LLVM emission).
The separate Unix mapping path is untested.

No clang, llvm-as, llc or lli was available. The actual LLVM execution test
is explicitly ignored, not a success-shaped missing-tool pass:

```powershell
cargo test -p tinylang-v2 --test external_backends llvm_execution::llvm_compiles_and_executes_against_vm -- --ignored --exact
```

Explicit invocation fails with the missing-clang diagnostic. Installing a
compatible external LLVM toolchain and running that test is a remaining
clang-specific acceptance gate. LLVM text is not certified merely because it contains a
function definition. It uses checked overflow intrinsics and the five-pointer
scalar frame ABI described in [architecture](COMPILER_ARCHITECTURE.md).

## Verified alternative LLVM execution path

The missing clang tool was not treated as the final execution limit.
The optional [validation test](../../../crates/tinylang_core/tests/test_llvm_execution.py) uses
the pinned [llvmlite dependency](../../../crates/tinylang_core/requirements-llvm.txt) in an isolated
environment, without changing the SDK or a global Python environment.
llvmlite 0.46.0 embeds LLVM 20.1.8.

The test invokes the real Rust compiler/IR emitter, calls LLVM parse/verify,
emits real AMD64 object bytes, loads those bytes through MCJIT, executes
the generated function and compares it with the actual core VM CLI.
Four tests passed with 27 compiled/object-loaded invocations: add=42,
negative and extreme floor division, 16 generated branch programs, a loop,
overflow/divide-zero status and instruction location, and exact fuel.
Every published function was checked RX (`0x20`) through VirtualQuery.

```powershell
# From the TinyLang Rust repository; use a working CPython 3.12 bootstrap.
& '..\Arduino-Coprocessor\.venv\Scripts\python.exe' -m venv '.\target\llvm-validation'
& '.\target\llvm-validation\Scripts\python.exe' -m pip install --only-binary=:all: -r '.\crates\tinylang_core\requirements-llvm.txt'
cargo build -p tinylang --bin tinylang --locked
cargo build --release -p tinylang-v2 --bin tinylang-v2 --locked
& '.\target\llvm-validation\Scripts\python.exe' '.\crates\tinylang_core\tests\test_llvm_execution.py' -v
```

The current llvmlite Windows target is explicitly
`x86_64-pc-windows-msvc-elf`: its target-machine API selects ELF even when
COFF is requested. Tests verify ELF64/little-endian/AMD64 and execute the
actual emitted objects. **This is not Windows COFF or executable-linking
certification.** The Rust CLI still emits IR rather than silently selecting
this optional test provider as its executor. Object preparation and ctypes
warm costs have a [separate baseline](V2_PERFORMANCE_BASELINE.md); native
Rust embedding and general-language LLVM lowering remain unimplemented.

Unsupported or unverified: general dynamic values, native host registration,
module/object linkage, cross-target AOT, executable production, DWARF/PDB
debug information, full language optimization and cross-platform certification.

See [architecture](COMPILER_ARCHITECTURE.md), [tests](V2_TEST_REPORT.md) and
[measured baseline](V2_PERFORMANCE_BASELINE.md).
