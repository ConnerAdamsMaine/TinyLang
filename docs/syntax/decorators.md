# Function decorators and stdx

```tiny
import "stdx" as ext

fn helper(x) { return x * 2 }

@ext.coprocessor(Arduino)
fn work(x) { return helper(x) }

print work(21)
```

This compiles and runs through the ordinary VM or JIT. Placement is metadata;
calls remain `CALL`. The compiler does not execute device calls or move functions.

## Grammar

```text
decorator = "@" identifier ("." identifier)*
            ("(" (argument ("," argument)*)? ")")?
argument  = string | signed-i64-literal | "true" | "false" | identifier
declaration = decorator* ("export")? "fn" ...
```

Bare identifiers are compile-time symbols. Expressions, arrays, structs,
closures, calls, floats, trailing commas, and runtime variable evaluation are
not supported. Decorators apply only to top-level functions in this phase.
Dangling decorators and decorated statements fail with source locations.

Parsing supports unqualified and qualified paths, omitted parentheses, and
ordered lists. Every decorator must also be registered. The production registry
currently contains only `stdx.coprocessor`; generic extension fixtures in unit
tests exercise the other forms without introducing extra language features.

## Namespace and registry

`import "stdx" as ext` binds a local compile-time namespace before declarations.
An omitted alias defaults to `stdx`. The namespace is available in both source
and file compilation; it does not require a filesystem stdlib, generate runtime
functions, or create a runtime module dependency. Each compilation unit must
import its own namespace. Aliases share collision rules with runtime imports
and declarations. `stdx` is never automatically injected.

`src/decorators/registry.rs` holds compiler-owned definitions: structured paths,
compatible declaration targets, repeatability, argument normalization, canonical
validation, and checks against existing permission grants. The parser only
collects metadata and invokes these rules. Artifact verification uses the same
definitions. New compiler extensions add definitions and their namespace
bindings here; runtime execution or source-defined decorators are not supported.
No artifact can register its own semantic rules.

`coprocessor` and target names are ordinary identifiers, not keywords.
`stdx.coprocessor` requires exactly one string or identifier argument, rejects
empty or whitespace-only targets, and may appear only once on a function,
including across namespace aliases. Identifiers normalize to strings. Target
spelling and case are preserved: Arduino, RP2350, ESP32, and FPGA0 are legal
metadata independently of backend availability.

## Metadata and lowerer handoff

`Function::decorators()` exposes an ordered slice of `Decorator` values.
`Decorator::path()` exposes canonical path segments; `arguments()` exposes
typed `DecoratorArgument::{String, Integer, Bool, Identifier}` values. Aliases
resolve to the canonical path `["stdx", "coprocessor"]` with one String target.
The generic metadata is the sole stored source of truth.

```rust
for function in verified.program().functions() {
    if let Some(target) = function.coprocessor_target() {
        println!("{} -> {}", function.name(), target);
    }
}
```

The getter describes placement without granting permission.
`VerifiedProgram::coprocessor_target(index)` also checks the function's effective
hardware grant and fails when it is unavailable. Future hardware execution
consumers must use this permission boundary. Optimizer and verifier preserve
the complete function metadata. Function equality includes decorators while
continuing to exclude diagnostic source ownership.

## Artifacts, budgets, identity, and cache

Binary `.tob` revision **5** replaces revision **4**; other revisions fail
explicitly. Each function stores a decorator count followed by path-segment
and argument tables before its parameter count. Argument tags are 0 string,
1 signed i64, 2 boolean (strict byte 0/1), and 3 identifier. Text uses bounded
UTF-8 length prefixes. Unknown tags, invalid booleans, truncation, excessive
counts, and malformed metadata return `TinyLangError`.

JSON stays at revision **2**, retaining legacy revision 1 policy behavior and
the existing enum metadata limitation. A missing function `decorators` field
means an empty list. Each decorator is `{"path":[...],"arguments":[...]}`;
arguments are `{"kind":"string|integer|bool|identifier","value":...}`.
Malformed objects, tags, values, paths, signatures, and duplicate placements
are rejected. JSON and binary round trips preserve canonical metadata.

Limits are 16 decorators per declaration, 8 segments per path, 16 arguments
per decorator, and 1 MiB total decorator text per program. Decoders check bounds
before allocating metadata. Verification checks the same budgets, identifier
spelling, registered definitions, signatures, duplicate rules, and permissions.
Retention accounting includes nested vector and string capacities.

Fingerprint domain **v5** includes decorator counts/order, segment counts/text,
argument counts, kinds, and values with length framing and domain separation.
Compile-cache format **10** invalidates previous compiler artifacts. Source
digests already detect decorator edits; incremental replacement copies complete
Function metadata, including the new target. Disk caches are unauthenticated:
programs carrying coprocessor metadata rebuild from current source and policy,
as existing host-facing builtin programs do. Imported modules with capability
grants retain the existing cache bypass policy.

## Hardware grants and frozen C ABI

Placement requires the owning compilation unit's externally established
`ModuleCapability::Hardware` and detailed hardware permission. Imported
functions cannot borrow their caller's grant. Root source compilation has the
embedding defaults; file compilation follows `Config.toml` policy. Decorators
never add grants. Untrusted artifact loaders retain declared permissions for
inspection but deny effective authority; callers can explicitly authenticate
artifacts through the existing trusted loader APIs.

The JSON C API at ABI version 2 retains its response envelope and function
response keys. C compilation responses reject decorated programs with a normal
compile error because that response shape cannot export the new metadata
without losing information or changing its function shape. Use Rust
artifact APIs to compile/export decorated programs. Ordinary C compile responses
omit the new empty field. Existing C execution entry points and host execution
semantics are unchanged.

## Existing Arduino reference

The sibling `../Arduino-Coprocessor` prototype was inspected without changes:
`test_tinylang_micro.py` and
`tinylang_micro_runtime/tinylang_arduino_runtime/tinylang_arduino_runtime.ino`.
Its TMB/2 runtime has indexed function frames, shared globals, syscalls,
separate `RUN_MAIN` and `CALL` lifecycles, and Uno limits of 640 module bytes,
8 functions, 16 slots, 24 operand values, and depth 4. A later lowerer can
enumerate placed roots from verified metadata and follow ordinary `CALL`
instructions to collect dependencies compatible with that runtime.

This feature adds no TMB encoder/lowerer, micro-profile verifier, firmware,
serial transport, COM discovery, RPC, automatic migration, or backend support.
Tests require no hardware or physical COM port.

## Implementation files and verification

Files changed for this feature, relative to `crates/tinylang_core`:

- `src/decorators.rs`, `src/decorators/metadata.rs`,
  `src/decorators/registry.rs`, `src/decorators/stdx.rs`: representation,
  shared semantic registry, fingerprint framing, and stdx rules.
- `src/syntax/token.rs`, `src/syntax/lexer.rs`, `src/compiler/parser.rs`,
  `src/compiler/parser/decorators.rs`: generic syntax, imports, attachment,
  source diagnostics, and namespace collision handling.
- `src/bytecode/program.rs`, `src/bytecode/verifier.rs`,
  `src/bytecode/binary.rs`, `src/bytecode/artifact.rs`,
  `src/bytecode/retention.rs`: metadata APIs, identity, artifacts, budgets,
  security checks, and retained storage charges.
- `src/compile_cache.rs`, `src/compiler/incremental.rs`, `src/lib.rs`,
  `src/ffi.rs`: cache revision, replacement regression, public reexports,
  and frozen response protection.
- `src/source.rs`: change the existing unsupported-character fixture from
  `@` to `$`, because `@` now lexes successfully.
- `tests/decorators.rs`, `tests/abi_api_soundness.rs`: complete source-to-artifact,
  VM/JIT parity, imports, permissions, cache, placement handoff, and ABI regressions.

Documentation changes are `docs/syntax/decorators.md`,
`docs/syntax/INDEX.md`, and `docs/bytecode.md`.
Unit tests alongside the parser, lexer, registry, artifact codecs, verifier,
retention accounting, and incremental compiler cover additional generic forms,
ordering, fingerprints, hostile tags/counts/text, canonical signatures, and grants.

Validation commands include `cargo fmt --all`, `cargo test`,
`cargo clippy --all-targets -- -D warnings`, and the authoritative
`cargo run --manifest-path crates/xtask/Cargo.toml -- release-gate`.
The gate also runs testing hooks, the language suite, allocator/xtask tests,
the release benchmark smoke test, Python tooling tests, and ABI manifest checks.
These were run with installed Rust 1.98.1 and the installed nightly rustfmt.
On this Windows checkout, `PYTHONCASEOK=1` is required because the physical
directory is `Tools` while Git and Python test imports use `tools`.
