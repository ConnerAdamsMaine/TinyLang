# TinyLang v2 experimental bindings

These bindings are isolated from the frozen v1 ABI and consume the same
little-endian golden vectors as the Rust implementation.

- `c/smoke.c` is a zero-dependency linked C11 consumer of `tinylang_v2.h`.
- `python/tinylang_v2.py` is a dependency-free `ctypes` wrapper around the raw
  runtime lifecycle and call interface; `python/smoke.py` exercises it against
  a built shared library. `python/programs.py` loads the committed TLBC example
  programs and exercises Program and VM handle ownership. The Python wire
  helpers encode and decode canonical UTF-8 value records and build
  `VM_INVOKE` request frames from pre-encoded argument records.
- `node/tinylang_v2_wire.mjs` builds and parses canonical `ArrayBuffer` frames.
  Envelope parsing rejects reserved flags and nonzero request status; Handle
  response parsing additionally requires the response flag. UTF-8 helpers
  encode and decode canonical text records, and the `VM_INVOKE` builder accepts
  pre-encoded argument records. Offset typed-array views and full-width
  `BigInt` handles are preserved.
  `node/programs.mjs` sends those frames through the binary-only
  `abi_stdio_bridge` Rust example so multiple calls share one runtime. Native
  in-process Node dispatch remains an adapter concern (N-API or another native
  loader); the wire codec does not pretend JavaScript can call a C ABI directly.

All integer serialization uses explicit little-endian operations. Bindings
must treat handles as opaque 64-bit values (`BigInt` in JavaScript) and must not
retain borrowed raw-call buffers after `tinylang_v2_call` returns.

Runtime and object handles now occupy disjoint internal domains: passing a
runtime value as an object, or an object value as a runtime, returns
`INVALID_HANDLE`. This changes generated object numbers, not the u64 wire type,
C declarations, symbols or ABI version. Earlier untagged object numbers are
not accepted as object handles; recreate objects when replacing the library.
Never construct or persist handles, or transfer them between library instances.
The [binary specification](../../docs/abi/v2-binary-spec.md#5-handles-and-provenance)
describes the compatibility boundary and unchanged generation retirement.

`interop_rust`, `python/programs.py`, and `node/programs.mjs` all load the same
profile-0 artifacts, create a VM session, release the Program reference, and
release the retained VM reference. The reference dispatcher implements
`VM_INVOKE` for exported functions with scalar arguments and results, and the
Python and Node wire helpers can build those requests. These lifecycle examples
use profile-0 artifacts, which contain main code but no callable function
exports, so they do not invoke the VM.
