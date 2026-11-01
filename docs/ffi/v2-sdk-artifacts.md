# TinyLang v2 SDK compiler artifacts

Python and Node support explicit `PROGRAM_LOAD` format selection. Existing
calls keep format1, including their positional correlation ID. There is no
magic-based format detection or decoder fallback.

| Format | Python name | Node name | Artifact limit |
| --- | --- | --- | --- |
| 1, TLBC v2 profile 0 | `ArtifactFormat.TLBC` | `ARTIFACT_FORMAT_TLBC` | 1 to 16 MiB |
| 2, compiler binary | `ArtifactFormat.COMPILER_BINARY` | `ARTIFACT_FORMAT_COMPILER_BINARY` | 1 to 8 MiB |

Format2 currently accepts the compiler's `TINYONEB` version-5 binary. This
compiler version is independent of the TLV2 wire version. It is experimental
interoperability, not a portable or frozen compiler-artifact schema.

## SDK APIs

Python, with `tinylang_v2/bindings/python` on the module search path:

```python
from pathlib import Path
from tinylang_v2 import ArtifactFormat, make_program_load_request, program_load_payload

tlbc_request = make_program_load_request(Path("program.tlbc").read_bytes(), 7)
compiler_request = make_program_load_request(
    Path("add.compiler.bin").read_bytes(),
    8,
    artifact_format=ArtifactFormat.COMPILER_BINARY,
)
compiler_payload = program_load_payload(
    Path("add.compiler.bin").read_bytes(), artifact_format=2
)
```

`artifact_format` is keyword-only. Only integer `1`/`2` or the corresponding
enum members are accepted. Booleans, floats, strings, `None`, and unknown
integers raise `ValueError`.

Node ESM, relative to `tinylang_v2/bindings/node`:

```javascript
import { readFileSync } from "node:fs";
import {
  ARTIFACT_FORMAT_COMPILER_BINARY,
  makeProgramLoadRequest,
  programLoadPayload,
} from "./tinylang_v2_wire.mjs";

const tlbcRequest = makeProgramLoadRequest(readFileSync("program.tlbc"), 7n);
const artifact = readFileSync("add.compiler.bin");
const compilerRequest = makeProgramLoadRequest(artifact, 8n, ARTIFACT_FORMAT_COMPILER_BINARY);
const compilerPayload = programLoadPayload(artifact, ARTIFACT_FORMAT_COMPILER_BINARY);
```

Only Number `1`/`2` selects a format. BigInt, boolean, string, null, fractional,
object, and unknown integer selectors raise `RangeError`. An omitted or
`undefined` selector uses format1. Correlation IDs remain full-width BigInt.

Python accepts bytes, bytearray, and memoryview artifacts. Node accepts
ArrayBuffer and typed-array/DataView views, including offset Buffer views.
Empty or over-limit artifacts are rejected before wire allocation. The
bindings do not verify compiler bytecode or authenticate serialized authority;
that remains the runtime's untrusted artifact decoder's responsibility.

Both payload helpers emit exactly a U32 format record followed by a Bytes
artifact record. Lengths exclude value headers and zero padding. The request
builders wrap this payload in the unchanged 32-byte TLV2 header. Artifact bytes
are copied unchanged, not converted to text or JSON.

## Reproduce the compiler artifact and DLL test

These PowerShell commands start in the outer workspace, use its working
Arduino virtual environment, and build a fresh DLL in a dedicated target
directory. They do not install or modify Python or Node dependencies.

```powershell
Set-Location 'C:\Users\Conner Adams\Desktop\404connernotfound\TinyOne\TinyOne'
$python = (Resolve-Path '..\Arduino-Coprocessor\.venv\Scripts\python.exe').Path

cargo build --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/sdk-artifacts --lib --example sdk_artifact
if ($LASTEXITCODE -ne 0) { throw 'v2 DLL/example build failed' }
& '.\target\sdk-artifacts\debug\examples\sdk_artifact.exe' '.\target\sdk-artifacts\add.compiler.bin'
if ($LASTEXITCODE -ne 0) { throw 'compiler artifact generation failed' }

$env:TINYLANG_V2_LIBRARY = (Resolve-Path '.\target\sdk-artifacts\debug\tinylang_v2.dll').Path
$env:TINYLANG_V2_COMPILER_ARTIFACT = (Resolve-Path '.\target\sdk-artifacts\add.compiler.bin').Path
& $python -m unittest discover -s tinylang_v2/bindings/python -p 'test_*.py' -v
node --test tinylang_v2/bindings/node/*.test.mjs

cargo test --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/sdk-artifacts --no-fail-fast
cargo clippy --locked --manifest-path tinylang_v2/Cargo.toml --target-dir target/sdk-artifacts --example sdk_artifact -- -D warnings
rustfmt --check --edition 2024 --config-path tinylang_v2/rustfmt.toml tinylang_v2/examples/sdk_artifact.rs
node --check tinylang_v2/bindings/node/tinylang_v2_wire.mjs
```

The dedicated Rust example calls `compile_source_unoptimized_verified` on
`fn add(a, b) { return a + b }`, calls `to_binary_artifact`, then round-trips
through `VerifiedProgram::from_binary_artifact`. It checks that the artifact
contains only `add`, with two parameters at function-table index 0. No TLBC
instructions or compiler containers are hand-assembled.

Observed SDK verification: Python 3.12.14, Node 26.8.2, Windows x64;
10 Python tests and 15 Node tests passed with no skips. The example generated
a 230-byte version-5 compiler binary. Integration output:

```text
PY compiler artifact: version=5 bytes=230 format=2 DLL=OK add(19,23)=42 program_release=OK vm_release=OK runtime_destroy=OK buffers=host-owned
NODE compiler artifact: version=5 bytes=230 format=2 codec=OK native-FFI=UNAVAILABLE
```

Python exercises capacity preflight, invokes after releasing the host's Program
reference, checks the canonical I64 result, and releases VM/runtime ownership.
It also checks default-format TLBC loading, no format1 fallback for compiler
bytes, and rejection of TLBC, trailing bytes, and old compiler versions under
format2. Request/response buffers are ctypes host-owned storage scoped to each
call; no runtime transfer buffer is acquired.

Without the environment paths, Python skips its integration class and Node
skips its real-artifact test. Setting only one Python path is an error, not a
skip. Unit-only results are not evidence of DLL execution.

The broader default-v2 snapshot during concurrent backend work was not green:
`cargo test --no-fail-fast` reported 67 passed, 2 failed, and 1 ignored. The
failures were `inferred_loop_frames_preserve_vm_semantics` in `engine_contract`
and `public_execution_selection_runs_actual_compiler_output` in
`execution_pipeline`, both rejected by the compiler for scalar `set` assignment
syntax. Clippy reached an out-of-scope `unnecessary_lazy_evaluations` error in
`src/engine/cli.rs`. These findings do not replace the SDK test results above
and were not changed by this SDK slice.

## Limits

Node remains a wire codec, not an in-process DLL binding. Its artifact test
uses the actual generated compiler binary but does not invoke a VM. No
arbitrary FFI package was installed. Python ctypes supplies the actual DLL
execution evidence.

This slice adds artifact loading, not a high-level invocation/value API. The
Python integration constructs canonical I64 argument records inside its test.
The current runtime invocation adapter supports null, bool, signed/unsigned
integers, f32, and f64 scalars only. Strings, bytes, heap objects, handles,
enums, slices, nested sequences/aggregates as arguments, and unit results are
not marshalled. A Sequence is the invocation argument envelope, not general
aggregate support. The legacy TLBC profile-0 fixtures have main code only,
not exported functions.

Normative read-only contracts:
[artifact specification](../abi/v2-artifact-spec.md),
[binary ABI](../abi/v2-binary-spec.md),
[opcodes](../abi/v2-opcodes.md), and
[invocation values](../abi/v2-invocation-values.md).
