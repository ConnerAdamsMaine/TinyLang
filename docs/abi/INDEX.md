---
title: ABI Reference
---

TinyLang's C ABI is the interface between the compiled `libtinylang`
shared library and any host application. It covers the JSON response
contract, the panic boundary, ownership rules, and the versioning
policy.

**The JSON C API declares ABI version 2 with `tinylang_*` exports.** The v1
ABI is maintained on `release/v1`; the experimental binary ABI remains
separate. See [versioning.md](versioning.md) for the compatibility policy.

Send ABI comments, concerns, and questions to the [TinyLang community
forum](https://tl.404connernotfound.dev).

For how to link and call the library from C, see
[ffi/c-integration.md](../ffi/c-integration.md).

# Documents in This Area

- [contract.md](contract.md) -- Runtime invariants callers can rely on
  today: panic boundary, null safety, ownership, thread safety, and the
  verification guarantee.
- [ABI versioning reference](versioning.md) -- What constitutes a
  breaking change, current stability status per area, and the v2
  JSON C API contract.
- [schemas.md](schemas.md) -- Exact JSON `value` schemas for every
  entry point's success response.
