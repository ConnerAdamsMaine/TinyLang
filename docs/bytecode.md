---
title: Bytecode Reference
---

# TinyOne Bytecode Reference

This document covers the TinyOne bytecode instruction set, the JSON artifact
format, the verifier's acceptance rules, and the JIT adaptive tier.

## Instruction Format

Every instruction is a fixed-size struct:

```rust
Instr { op: Op, arg: i64, arg2: i64 }
```

`arg` and `arg2` carry operands whose meaning is opcode-specific. Fields unused
by a particular opcode are zero. In the JSON artifact representation each
instruction is:

```json
{ "op": "OPCODE_NAME", "arg": 0, "arg2": 0 }
```

Source compilation also retains optional, fixed-length byte-offset tables
beside main/function bytecode. These are cold diagnostic metadata, not fields
in `Instr`, executable fingerprints, or either artifact format. Peephole
compaction preserves the corresponding source origin. Artifact decoding has
no source table unless a separate source-aware owner restores one. The private
disk cache does this using a snapshot-bound `.origins` sidecar and validated
source inputs; it does not extend the portable artifact formats or restore
serialized authority.
JIT lowering retains a second cold mapping from executable operations back to
original bytecode indexes, including distinct failure sites within fused
operations. Neither mapping changes when an instruction is quickened.

## Opcodes

There are 34 opcodes. Opcode ordinals are serialized into artifacts and are
part of program fingerprints. Ordinals 1-29 are the frozen Phase-1 range;
ordinals 30+ are Phase-2 extensions.

| Ordinal | Mnemonic | `arg` | `arg2` | Stack effect | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | `PUSH_INT` | integer value | — | `→ int` | Push a literal integer |
| 2 | `LOAD` | slot index | — | `→ value` | Load from stack-frame slot |
| 3 | `STORE` | slot index | — | `value →` | Store to stack-frame slot |
| 4 | `ADD` | — | — | `a b → a+b` | Integer addition |
| 5 | `SUB` | — | — | `a b → a-b` | Integer subtraction |
| 6 | `MUL` | — | — | `a b → a*b` | Integer multiplication |
| 7 | `DIV` | — | — | `a b → a//b` | Floor division; runtime error on divide-by-zero |
| 8 | `NEG` | — | — | `a → -a` | Unary integer negation |
| 9 | `PRINT` | — | — | `value →` | Print value to stdout, with newline |
| 10 | `LT` | — | — | `a b → bool` | Less-than comparison |
| 11 | `LTE` | — | — | `a b → bool` | Less-than-or-equal |
| 12 | `GT` | — | — | `a b → bool` | Greater-than |
| 13 | `GTE` | — | — | `a b → bool` | Greater-than-or-equal |
| 14 | `EQ` | — | — | `a b → bool` | Equality |
| 15 | `NE` | — | — | `a b → bool` | Not-equal |
| 16 | `JUMP` | target pc | — | — | Unconditional jump to `arg` |
| 17 | `JUMP_IF_ZERO` | target pc | — | `cond →` | Jump to `arg` if top is zero; pop condition |
| 18 | `CALL` | function index | arg count | `args… → ret` | Call function `arg`; `arg2` args popped from stack |
| 19 | `RETURN` | — | — | `value →` | Return top of stack from current function |
| 20 | `HALT` | — | — | — | Terminate the main chunk |
| 21 | `PUSH_STRING` | string table index | — | `→ string` | Push string heap reference |
| 22 | `MAKE_ARRAY` | element count | — | `elems… → array` | Pop `arg` values; construct heap array |
| 23 | `INDEX` | — | — | `arr idx → value` | Array or string index |
| 24 | `SET_INDEX` | — | — | `arr idx value →` | Mutate array element |
| 25 | `MAKE_STRUCT` | struct def index | field count | `fields… → struct` | Construct heap struct |
| 26 | `GET_FIELD` | field table index | — | `struct → value` | Read named field |
| 27 | `SET_FIELD` | field table index | — | `struct value →` | Write named field |
| 28 | `BUILTIN` | builtin table index | arg count | `args… → result` | Call builtin or stdlib bridge |
| 29 | `PUSH_NULL` | — | — | `→ null` | Push the null raw pointer |
| 30 | `POP` | — | — | `value →` | Discard expression-statement result |
| 31 | `LOAD_GLOBAL` | slot index | — | `→ value` | Load from the main top-level frame |
| 32 | `MAKE_ENUM` | enum variant index | field count | `fields… → enum` | Construct heap enum value |
| 33 | `PUSH_BOOL` | 0 or 1 | — | `→ bool` | Push a literal `true`/`false` |
| 34 | `PUSH_FLOAT` | `f64` bit pattern (as `i64`) | — | `→ float` | Push a literal float (always `fp64`) |

### Notes on specific opcodes

**`LT` / `LTE` / `GT` / `GTE` / `EQ` / `NE`** — all six comparison opcodes
produce a `Value::Bool`, printed as `true`/`false` (not an integer `0`/`1`).
The bytecode-level peephole optimizer's constant folder must match this
exactly — folding `PUSH_INT a, PUSH_INT b, LT` (etc.) into `PUSH_BOOL`, not
`PUSH_INT` — otherwise a compile-time-constant comparison and the identical
comparison through variables would print differently.

**`PUSH_FLOAT`** — `arg` is the literal's `f64` bit pattern from
`f64::to_bits()`, reinterpreted as `i64` (matching the raw-bits convention
already used for floats crossing the JSON artifact format). There is no
float constant pool. Float literals are always `TypeKind::Fp64` — there is
no literal syntax for `fp8`/`fp16`/`fp32`; the only way to produce one is the
matching cast builtin (`fp8(x)`/`fp16(x)`/`fp32(x)`, see `docs/stdlib.md`),
which rounds to that format's precision (`src/runtime/minifloat.rs`: `fp32`
uses Rust's native `f32` round-trip; `fp16` is IEEE-754 binary16; `fp8` is
E4M3 — 1 sign + 4 exponent + 3 mantissa bits, bias 7, OCP/Nvidia-style,
saturating to ±448 on overflow rather than producing an infinity). Storage
is always a full `f64` (`Value::Float { kind, bits }`) regardless of `kind`
— precision is enforced by rounding on cast and after every arithmetic op,
not by bit-packed width. `ADD`/`SUB`/`MUL`/`DIV`/`NEG` all promote an integer
operand to `f64` when paired with a float operand, and round the result to
the wider operand's precision when mixing float kinds; division by zero is
a runtime error for floats too (not IEEE-754 infinity/NaN), matching
integer division's behavior.

**`CALL`** — `arg` is a zero-based index into `program.functions`. The callee's
frame is initialized with the top `arg2` values popped from the caller's stack.
Additional slots are zero-initialized. The return value is pushed after the call
returns.

**`LOAD_GLOBAL`** — `arg` is a slot in the main top-level frame. Function chunks
use this when reading a top-level variable declared before the function. There
is no `STORE_GLOBAL`; top-level slot assignment from a function is intentionally
rejected by the compiler.

**`BUILTIN`** — Phase-1 builtins occupy slots 0–34 in the canonical table.
Phase-2 stdlib bridge builtins follow after slot 34. Slot order within each
group is stable and must not be reordered without a bytecode version bump.

**`MAKE_STRUCT`** — `arg` indexes into `program.structs` for the field names;
`arg2` is the number of fields (and values to pop). The compiler always sets
`arg2 == structs[arg].fields.len()`.

**`GET_FIELD` / `SET_FIELD`** — `arg` is an index into `program.fields`, the
global field-name intern table shared across all structs and enum variants.
For enum values, the field name `"tag"` is a virtual field (the variant's
0-based declaration index) — it is never stored among the variant's own
fields, and `SET_FIELD` rejects writes to it.

**`MAKE_ENUM`** — `arg` indexes into `program.enum_variants`, a flat,
program-global table where each entry denormalizes its enum name, variant
name, 0-based tag, and field names; `arg2` is the number of fields (and
values to pop). The compiler always sets
`arg2 == enum_variants[arg].fields.len()`. Enum declarations are file-local
(no cross-module export) and do not currently round-trip through the JSON
artifact format — `Program::from_artifact` always sets `enum_variants` to
empty, so artifacts containing `MAKE_ENUM` fail verification rather than
running incorrectly.

**`JUMP` / `JUMP_IF_ZERO`** — targets are absolute instruction indices, not
relative offsets. The verifier bounds-checks all targets before execution.

## Program Structure

A `Program` contains:

```
Program {
  code:       Vec<Instr>        — main chunk
  slot_count: usize             — main frame slot count
  names:      Vec<String>       — variable name table (debug only)
  functions:  Vec<Function>     — named function chunks
  strings:    Vec<String>       — string literal intern table
  structs:    Vec<StructDef>    — struct definitions (name + field names)
  fields:     Vec<String>       — global field-name intern table
  modules:    Vec<ModuleDef>    — import/export metadata
  enum_variants: Vec<EnumVariantDef> — flat enum variant table (enum name, variant name, tag, field names)
}

Function {
  name:        String
  param_count: usize
  code:        Vec<Instr>
  slot_count:  usize
  names:       Vec<String>
}
```

Functions are indexed from zero. The main chunk is not in `functions`; it is
`program.code`. Module metadata records the original import strings and export
lists. It is security-relevant: the verifier derives declaration ownership
from qualified names and checks calls, function values, constructors, globals,
exports, imports, and dependency cycles before either backend can execute.

## JSON Artifact Format

```json
{
  "format":     "tinyone-bytecode",
  "version":    1,
  "code":       [ {"op": "PUSH_INT", "arg": 42, "arg2": 0}, … ],
  "slot_count": 0,
  "names":      [],
  "functions":  [
    {
      "name":        "add",
      "param_count": 2,
      "code":        [ … ],
      "slot_count":  2,
      "names":       ["a", "b"]
    }
  ],
  "strings":    ["hello", "world"],
  "structs":    [ {"name": "Point", "fields": ["x", "y"]} ],
  "fields":     ["x", "y"],
  "modules":    [
    {
      "name":               "math",
      "path":               "math",
      "imports":            [],
      "exported_functions": ["add"],
      "exported_structs":   []
    }
  ]
}
```

### Artifact resource limits

These are checked by `Program::from_artifact` before any `Vec::collect`:

| Field | Limit |
| --- | --- |
| Total artifact bytes | 8 MiB |
| `functions` count | 4,096 |
| `structs` count | 4,096 |
| `code` ops per chunk | 65,536 |
| Total ops across all chunks | 262,144 |
| `strings` count | 65,536 |
| `fields` count | 65,536 |
| `slot_count` (per chunk) | 65,536 |
| `modules` count | 256 |
| `imports` per module | 4,096 |
| `exports` per module | 4,096 |
| Fields per struct | 256 |
| `names` count | 65,536 |
| Total string/name/field text bytes | 1 MiB |

A hostile artifact that exceeds any limit receives a structured `TinyOneError`
before any allocation for the program body.

## Compact Binary Artifact Format

`.tob` files use the magic `TINYONEB`, a little-endian version field, bounded
length-prefixed tables, numeric opcode ordinals, and fixed-width instruction
operands. The current binary revision is **5**; it includes the complete
program metadata, including enum variants, execution permissions, and ordered
function decorators. Revision 5 adds decorators to revision 4's layout.
All other revisions are rejected. Decoding applies byte, table, text, per-chunk, and
total-instruction limits before verification and rejects unknown versions,
unknown opcodes, truncation, invalid UTF-8, and trailing bytes.

`load_verified_artifact` auto-detects compact binary versus JSON by magic.
The CLI emits binary when the `--emit-bytecode` path ends in `.tob`; other
extensions retain the readable JSON format.

## Program Fingerprint

`Program::fingerprint()` hashes the complete program with Blake2b512 and
returns the first 16 bytes as a lowercase hex string. The domain-separated v5
hash covers opcodes, operands, code/table lengths, slot and local names,
function declarations and ordered typed decorators, string literals, structs, fields, modules (including
their capability grants), and enum variants. `VerifiedProgram` computes it once in a shared `OnceLock`, so clones
do not re-hash metadata. It keys both the in-memory JIT cache and integrity
checks for dependency-validated disk artifacts. It identifies artifact content:
generic artifact loading preserves the fingerprint while removing policy trust.
JIT execution identity additionally includes whether the embedding trusted the
policy, so an untrusted artifact cannot reuse a trusted program's authority.

## Verifier Rules

`BytecodeVerifier::verify(program)` accepts a program if and only if all of the
following hold for every chunk (main and all functions):

1. **Terminal opcode** — the chunk's last reachable instruction is `HALT` (main)
   or `RETURN` (function).
2. **Stack balance** — every reachable instruction has all its required input
   operands before producing any results. The inferred stack depth is
   non-negative and consistent across all paths reaching that instruction.
3. **Stack depth** — stack depth never exceeds 65,536.
4. **Branch targets** — all `JUMP` and `JUMP_IF_ZERO` targets are within
   `[0, code.len())`.
5. **Slot indexes** — all `LOAD` and `STORE` operands are in
   `[0, chunk.slot_count)`. `LOAD_GLOBAL` operands are in
   `[0, program.slot_count)`.
6. **String indexes** — all `PUSH_STRING` operands are in
   `[0, program.strings.len())`.
7. **Field indexes** — all `GET_FIELD` and `SET_FIELD` operands are in
   `[0, program.fields.len())`.
8. **Struct indexes** — all `MAKE_STRUCT` operands are in
   `[0, program.structs.len())`.
9. **Function indexes** — all `CALL` operands are in
   `[0, program.functions.len())`, and the arity matches `function.param_count`.
10. **Builtin indexes** — all `BUILTIN` operands are in
    `[0, BUILTINS.len())`, and the argument count is within the builtin's
    declared range `[min_args, max_args]`.
11. **Budget limits** — total ops across all chunks ≤ 262,144; function count ≤
    4,096; per-chunk slot count ≤ 65,536; string/field/name count ≤ 65,536;
    struct count ≤ 4,096; module count ≤ 256; imports/exports per module ≤ 4,096;
    total text bytes ≤ 1 MiB.
12. **Work limit** — traversal step count ≤ 10,000,000. Prevents timeout on adversarial
    jump graphs.
13. **Module metadata integrity** — module names and logical paths agree;
    declarations have valid unique qualified names; imports resolve to known
    modules with consistent targets and unique aliases; exports name existing
    owned declarations; and the dependency graph is acyclic.
14. **Module visibility** — main/root bytecode may call, take function values
    for, or construct only exported module declarations. Module functions may
    access their own declarations and exported declarations of directly
    imported modules. Module functions cannot read root global slots. Private
    structs/functions and unexported cross-module declarations are rejected.
    Module enums remain file-local.

These rules are applied by `VerifiedProgram` before VM execution and before
JIT lowering, so the adaptive JIT never receives bytecode with a wider module
authority than the source compiler would grant.

The immutable verification certificate retains per-chunk stack peaks, numeric
permission ownership and boxed basic-block metadata. A block records its
half-open bytecode range and optional reachable entry depth in 12 bytes;
unreachable is distinct from an empty stack. Chunk entry, branch targets and
instructions following terminators define boundaries. Offsets use `u32`, so
the complete instruction budget remains representable. Verification-token
clones share these facts rather than rebuilding them during JIT lowering.

## JIT Adaptive Tier

### Compilation

`JitProgram::compile` translates verified TinyOne bytecode into `JitOp` — a
Rust enum with all operands decoded to native types. Main is lowered eagerly;
function chunks are lowered into reserved slots on their first call:

- `LOAD 3` → `JitOp::Load(3usize)` — no conversion at runtime
- `PUSH_INT 42` → `JitOp::PushInt(42i64)`
- `CALL 1 2` → `JitOp::Call(1usize, 2usize)`

Operand widths follow static verifier guarantees, not just reachable stack
depths. Slot indexes fit `u16`, but a slot count or function arity can be
65,536. Branch offsets can exceed 65,535. On 64-bit hosts, unreachable
`MAKE_ARRAY` and `CALL_VALUE` instructions may even have statically valid
arities above `u32::MAX`; lowering preserves those operands without narrowing
or attempting to execute them. Any future compact encoding must account for
these cases independently of reachable operand-stack limits.

Within each verified block, symbolic constants, local reads and binary
expressions pass through ordered constant/copy propagation, slot-version
propagation, constant and branch folding, dead-temporary elimination, and safe
dead-store elimination before final instruction fusion. These direct encodings
apply when the expression has not already folded to a literal; they no longer
require adjacent scalar source patterns:

| Bytecode sequence | JIT superinstruction |
| --- | --- |
| `PUSH_INT n, STORE s` | `StoreInt(s, n)` |
| `LOAD a, STORE b` | `MoveSlot(a, b)` |
| `LOAD s, PUSH_INT n, ADD, STORE s` | `AddSlotInt(s, n)` |
| `LOAD s, PUSH_INT n, SUB, STORE s` | `SubSlotInt(s, n)` |
| `LOAD s, PUSH_INT n, MUL, STORE s` | `MulSlotInt(s, n)` |
| `LOAD s, PUSH_INT n, DIV, STORE s` | `DivSlotInt(s, n)` |
| `LOAD s, PUSH_INT n, MUL` | `PushMulSlotInt(s, n)` |
| `LOAD s, PUSH_INT n, DIV` | `PushDivSlotInt(s, n)` |
| `LOAD s, PUSH_INT n, CMP, JUMP_IF_ZERO t` | `CompareSlotIntJumpIfZero(s, n, cmp, t)` |
| `LOAD s, JUMP_IF_ZERO t` | `JumpIfZeroSlot(s, t)` |
| `LOAD a, LOAD b, ADD, STORE c` | `BinarySlotsStore(Add, a, b, c)` |
| `LOAD a, PUSH_INT n, ADD, STORE b` | `BinarySlotIntStore(Add, a, n, b)` |
| `LOAD a, PUSH_FLOAT bits, ADD, STORE b` | `BinarySlotFloatStore(Add, a, bits, b)` |

Block partitioning prevents branches into a fused expression. Effects and
block exits materialize pending values in original evaluation order; non-empty
incoming stacks remain physical. Potentially failing arithmetic is not removed
merely because its result is discarded. Only successful I64 arithmetic and
comparisons are constant-folded; float arithmetic and failures remain runtime
operations. Versioned copies preserve snapshots after writes, and slot facts
never cross opaque effects or basic-block boundaries. Dead stores are limited
to adjacent non-failing literal writes without intervening reads or effects.
The direct scalar store families use
the shared checked I64 handlers with actual-tag guards and generic fallbacks.
The float-immediate family reconstructs the exact Fp64 literal from its stored
bits and reaches shared float/mixed-numeric semantics, including promotion,
comparison booleans and division-by-zero errors. It does not fold float
arithmetic, infer a source-slot type, or change portable bytecode.
The multiply/divide expression forms leave one result on the operand stack;
move, scalar stores and direct branches avoid temporary operands entirely.

### Hot-path quickening

Every backward branch in a compiled chunk increments an edge counter. When a
counter first reaches its configured threshold (8 by default), the range
`[branch_target, branch_instruction + 1)` is armed for sampling. Type evidence
can then select the following faster variants in place:

| Cold | Quickened | Effect |
| --- | --- | --- |
| `Add` | `AddInt` | Checked direct I64 path; generic fallback otherwise |
| `Sub` | `SubInt` | Same |
| `Mul` | `MulInt` | Same |
| `Div` | `DivInt` | Same |
| `Compare(op)` | `CompareInt(op)` | Direct I64 comparison; generic fallback otherwise |
| `PushMulSlotInt(s, n)` | `PushMulSlotIntHot(s, n)` | Reads an encoded I64 slot directly and performs checked multiplication |
| `PushDivSlotInt(s, n)` | `PushDivSlotIntHot(s, n)` | Reads an encoded I64 slot directly and preserves floor division and errors |
| `MulSlotInt(s, n)` | `MulSlotIntHot(s, n)` | Updates an encoded I64 slot in place; generic numeric fallback otherwise |
| `DivSlotInt(s, n)` | `DivSlotIntHot(s, n)` | Updates an encoded I64 slot in place with checked floor division |
| `CompareSlotIntJumpIfZero(...)` | `CompareSlotIntJumpIfZeroHot(...)` | Compares an encoded I64 slot and branches without operand-stack traffic |
| `JumpIfZeroSlot(s, t)` | `JumpIfZeroSlotHot(s, t)` | Tests an encoded I64 slot for zero, with generic truthiness fallback |
| `Jump(t)` | `JumpHot(t)` | Skips back-edge counter increment |
| `JumpIfZero(t)` | `JumpIfZeroHot(t)` | Same |

Typed quickening follows bounded per-site observations, not hotness alone.
Eight consecutive actual-I64 samples select these typed handlers. Sampling
stops after 32 observations or a third representation shape; unsupported stable
types remain generic. A miss uses shared generic semantics and records its
representation. Four misses without 32 consecutive hits trigger resampling;
each site allows at most two promotions before repeated instability makes it
permanently generic. Counter-eliding jumps need no type evidence.

Fixed-length feedback tables are allocated only for heated chunks, under one
64-KiB program-wide byte budget. Tables that cannot fit are skipped; this does
not reject an otherwise valid program. Metadata retains representation tags,
not runtime values. All rewrites preserve instruction extent and operands.

The default hot-edge threshold is 8. `JitOptions` can select another `u16`
threshold for `JitProgram::compile_with_options`, `JitCache::with_options`, or
the configured runner APIs; zero disables quickening.

### Cache

`JitCache` stores `JitProgram` instances keyed by content fingerprint and policy
trust. Both must match. A cache hit reuses the compiled and potentially
already-quickened program, so hot ranges from a previous run carry over.
Compiled retention uses independent LRU limits (128 entries and 16 MiB of
charged entry data by default), configured with `with_compiled_cache_limits`.
Either zero disables retention, not execution. Charges include verified
metadata and buffer capacities, lazy chunks, feedback and idle operand storage;
they are updated after successful and failing runs. Oversized results bypass
retention. The borrowed compile-only API can keep one temporary result outside
that budget until the next mutation. `compiled_cache_stats()` reports it
separately. Charges exclude opaque allocator overhead and unused hash buckets;
see [Rust API](ffi/rust-api.md) for the full accounting boundary.

Source-based cache calls additionally reuse an exact-source `VerifiedProgram`;
the first call still compiles and verifies normally. That source cache is a
deterministic least-recently-used cache bounded by both entry count and retained
source bytes (128 entries and 8 MiB by default). Configure it with
`JitCache::with_source_cache_limits`; setting either limit to zero disables
source retention without disabling the compiled-program
cache. `JitCacheStats` reports source hits, misses, compilations, evictions,
bypasses, and currently retained source bytes.

Compatibility methods accepting `Program` verify before insertion. The
`compile_verified` and `run_verified_program*` methods accept the capability
returned by verified compiler/artifact APIs and skip redundant verification
and fingerprint calculation.

## Peephole Optimizer

The optimizer runs shrinking forward passes before verification. It folds
constant-only sequences within each straight-line control-flow region:

| Input sequence | Output |
| --- | --- |
| `PUSH_INT a, PUSH_INT b, ADD` | `PUSH_INT (a+b)` |
| `PUSH_INT a, PUSH_INT b, SUB` | `PUSH_INT (a-b)` |
| `PUSH_INT a, PUSH_INT b, MUL` | `PUSH_INT (a*b)` |
| `PUSH_INT a, PUSH_INT b, DIV` | `PUSH_INT (a//b)` (if b≠0) |
| `PUSH_INT a, NEG` | `PUSH_INT (-a)` |
| `PUSH_INT a, PUSH_INT b, LT/LTE/GT/GTE/EQ/NE` | `PUSH_BOOL (true or false)` |

Folding never consumes an instruction that is an interior branch target. When
a fold shortens a chunk, every jump target is remapped before the next pass.
The optimizer does not reorder expressions or analyze across basic-block
boundaries.

Each pass compacts an owned instruction buffer in place; it does not allocate
a second instruction vector. Chunks without a constant-fold candidate skip
scratch allocation, and the remapping table is built only after a branch-safe
fold is found. The output is compacted once at completion so removed instructions and
input builder slack do not remain retained. Negative and out-of-range branch
operands remain invalid for authoritative verification. Integer overflow and
division failures remain runtime operations.
