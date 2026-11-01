# TinyLang v2 SDK compatibility

| Surface | Status | Compatibility boundary |
| --- | --- | --- |
| Rust core | Existing compiler/VM implemented | Compile and verify source, binary artifacts, persistent function invocation |
| Experimental Rust scalar engine | Partial | Checked-i64 functions only; explicit backend preparation and fuel |
| C v2 ABI | Partial, existing tests pass | Versioned wire protocol and runtime/program/VM handles; scalar invocation only |
| Python ctypes | Compiler-artifact integration tested | 10 tests, including real DLL add(19,23)=42 and lifecycle; format-1 default preserved |
| Node wire codec | Compiler-artifact codec tested | 15 tests; format 2 supported; no native Node FFI execution SDK installed |
| Optional Python LLVM validation | Tested tooling, not an SDK dependency | LLVM 20.1.8 verifies/compiles/loads objects from the Rust emitter; 4 tests passed |
| General host registration | Not started in this milestone | No new native callback/typed aggregate ownership surface |

The frozen v1 ABI and inherited DLL export surface are preserved. No symbols
are filtered or renumbered. Compiler format and invocation protocol versions
are separate contracts; selecting compiler artifact format 2 does not change
the wire protocol version.

VM values currently support null, bool, integer, f32 and f64 scalar wire
records. Heap, aggregate, unit and opaque language-handle marshalling remain
incomplete. The scalar native profile uses only i64 and is not automatically
installed behind the C ABI's existing JIT-permission flag.

The integration example generates a genuine compiler `TINYONEB` version-5
artifact (230 bytes), then verifies it through the untrusted decoder.
Python invokes export 0 through the DLL, releases program and VM handles,
destroys the runtime and uses host-owned response buffers. Invalid selectors,
empty/oversized artifacts and format mismatches fail explicitly.

Host commands and results are in [test report](V2_TEST_REPORT.md) and
[SDK artifact documentation](../../ffi/v2-sdk-artifacts.md).
