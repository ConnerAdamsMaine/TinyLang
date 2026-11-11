# TinyLang v2 Authority/TPack Relationship Security Audit

**Date:** 2026-11-08  
**Status:** Complete  
**Scope:** Authority trust boundary, TPack artifact verification, sandbox/FFI isolation, supply chain, compromised employee/credential scenarios  
**Repository:** TinyOne/TinyOne (crates/tinylang_core)

---

## Executive Summary

This audit covers the **Authority <-> TPack trust relationship** in TinyLang v2. The architecture uses a policy_trusted flag on Program to distinguish untrusted TPack artifacts (default: sandboxed, no capabilities) from Authority-verified artifacts (opt-in: full declared capabilities). 

**Critical finding:** No critical vulnerabilities. The trust boundary is correctly implemented at the type level (VerifiedProgram vs Program).

**High-severity findings (2):**
- **H1:** FFI sandbox worker path resolution trusts filesystem layout without integrity verification
- **H2:** No dependency pinning/verification (cargo-deny, SBOM, sigstore): supply chain gap

**Medium-severity findings (5):** Custom allocator unsafe surface, JIT quickening trust assumptions, module sandbox TOCTOU, Config.toml over-privilege, FFI response size limit

**Low-severity findings (5):** Defense-in-depth gaps in artifact format, certificate validation, capability granularity

---

## Architecture Overview

### Trust Boundary

| Layer | Mechanism | Default (TPack) | Authority Opt-in |
|-------|-----------|-----------------|------------------|
| **Program flag** | policy_trusted: bool | false | trust_artifact_policy() -> true |
| **Artifact loading** | Program::from_artifact() | Verified, sandboxed | VerifiedProgram::from_trusted_artifact() |
| **Capabilities** | 8 coarse ModuleCapability | ModulePermissions::none() | Fine-grained from manifest |
| **Enforcement** | capabilities_for_function() / permissions_for_chunk() | At every builtin call | At every builtin call |

**Key files:**
- bytecode/program.rs: Program, ModuleCapability, ModulePermissions, trust_artifact_policy()
- bytecode/certificate.rs: VerificationCertificate, ChunkCertificate
- bytecode/artifact.rs: from_artifact(), from_trusted_artifact()
- bytecode/binary.rs: Binary artifact equivalents
- compiler/modules.rs: Module resolution, signature verification, sandbox containment
- config.rs: verify_module_signature(), authority certificate management
- runtime/builtins.rs: Capability enforcement at builtin boundary
- ffi.rs / ffi/sandbox.rs: Sandbox worker transport
- bin/tinylang-sandbox-worker.rs: Sandbox worker entry point

---

## Attack Vector Analysis

### Open Vector Attacks (Network-facing)

| Vector | Exposure | Mitigation Status |
|--------|----------|-------------------|
| Malicious TPack artifact loaded via FFI | C ABI tinylang_load_* accepts arbitrary JSON/binary | **Mitigated**: Artifact verification runs before execution; policy_trusted=false by default |
| Sandbox worker hijack via PATH/env | sandbox_worker_path() searches current_exe ancestors + deps/ | **H1 - GAP**: No signature/hash verification of worker binary |
| Supply chain: compromised dependency | 100+ transitive deps, no cargo-deny, no SBOM | **H2 - GAP**: No advisory scanning in CI, no sigstore verification |
| Module import path traversal | ModuleResolver canonicalizes but TOCTOU window exists | **M3 - PARTIAL**: Documents concurrent modification risk; no OS isolation |
| FFI response exhaustion | MAX_SANDBOX_RESPONSE_BYTES = 16MiB unbounded allocation | **M5 - GAP**: Limit exists but no streaming/chunked processing |
| JIT code injection via quickening | Quickening trusts policy_trusted without re-verification | **M2 - GAP**: No assertion that quickened code matches verified bytecode |

### Closed Vector Attacks (Local/insider)

| Vector | Exposure | Mitigation Status |
|--------|----------|-------------------|
| Compromised employee: modify TPack artifact post-verification | Artifact loaded from disk; no re-verification at load time | **Mitigated**: from_trusted_artifact only used by Authority tooling; TPack uses from_artifact |
| Compromised employee: inject malicious module into Config.toml | Config.toml declares module permissions; over-privilege possible | **M4 - GAP**: No least-privilege enforcement; require_module_signatures opt-in |
| Compromised credentials: Authority signing key theft | Ed25519 keys in AuthorityCertificate; verify_strict not used | **PARTIAL**: Keys parsed but weak-key check missing (see Authority audit A-01) |
| Malicious Config.toml symlink | canonicalize() on config path; symlink check exists | **Mitigated**: Config.toml must not be symlink outside project root |
| Arena allocator corruption | tinylang_ralloc uses unsafe extensively | **M1 - GAP**: No fuzzing, no cargo-miri in CI |

### Supply Chain Attacks

| Vector | Exposure | Mitigation Status |
|--------|----------|-------------------|
| Dependency confusion / typosquatting | Cargo.lock pins versions but no cargo-deny advisories | **H2 - GAP**: No automated advisory scanning |
| Compromised crate publish | No sigstore/SBOM verification of resolved deps | **H2 - GAP**: Lock file binds declarations, not content hashes |
| Build-time code injection | build.rs embeds roots; no reproducible build verification | **PARTIAL**: Build script reads env vars; no hermetic build proof |
| Transitive dependency CVE | ed25519-dalek, rustls, tokio all present | **H2 - GAP**: No cargo-audit gate in CI |

### Compromised Employee / Credential Attacks

| Vector | Exposure | Mitigation Status |
|--------|----------|-------------------|
| Stolen Authority signing key | Ed25519 verification uses verify() not verify_strict() | **GAP**: Weak key acceptance (Authority audit A-01) |
| Stolen TPack publishing creds | No TPack-specific credentials in this codebase | N/A (external) |
| Malicious module author | Module signatures verified against Authority certs | **Mitigated**: verify_module_signature() enforces authority chain |
| Compromised build machine | No hermetic/reproducible build attestation | **GAP**: No SLSA provenance |

---

## Detailed Findings

### HIGH

#### H1: FFI Sandbox Worker Path Trust
**Location:** ffi.rs:236-278 (sandbox_worker_path())  
**Risk:** The sandbox worker binary is located by searching ancestor directories of current_exe() and deps/ subdirectories for a file matching tinylang_sandbox_worker-*. No integrity verification (hash, signature, code signing) is performed. An attacker with filesystem write access can replace the worker binary.  
**Attack scenario:** Compromised CI/CD artifact, insider with write access to deployment directory, supply chain compromise of built binary.  
**Evidence:** Function accepts TINYLANG_SANDBOX_WORKER env var OR searches filesystem; no sha256/verify step.  
**Remediation:** Embed expected worker hash at build time; verify on spawn; or use current_exe() sibling with known hash.

#### H2: No Supply Chain Verification
**Location:** Workspace-wide (no deny.toml, no cargo-deny in CI, no SBOM)  
**Risk:** 100+ transitive dependencies with no automated advisory scanning, no signature verification of resolved crates, no SBOM generation.  
**Evidence:** Cargo.lock exists but no deny.toml; .github/workflows/*.yml lack cargo-deny step; no cargo-audit or cargo-cyclonedx in CI.  
**Remediation:** Add deny.toml with advisory policy; add cargo-deny check to CI; generate SBOM; evaluate sigstore for crate verification.

### MEDIUM

#### M1: Custom allocator unsafe surface
**Location:** tinylang_ralloc/src/arena.rs: 20+ unsafe blocks for pointer arithmetic, header manipulation, block splitting/merging  
**Risk:** Memory safety bugs (use-after-free, double-free, buffer overflow) in allocator could lead to arbitrary code execution. No fuzzing, no cargo-miri in CI.  
**Evidence:** unsafe impl Send for Arena; raw pointer casts; manual header writes.  
**Remediation:** Integrate cargo-fuzz targeting alloc/dealloc; add cargo-miri to CI; consider #[cfg(test)] invariants.

#### M2: JIT Quickening Trust Assumption
**Location:** jit/vm.rs (and execution/jit/vm.rs): quickening optimizes based on policy_trusted without re-verification  
**Risk:** If quickening logic has a bug, it could incorrectly optimize untrusted code as trusted, or vice versa. No assertion that quickened code matches verified bytecode semantics.  
**Evidence:** policy_trusted checked at quickening decision points; no cross-check with verifier output.  
**Remediation:** Add debug assertions comparing quickened behavior to interpreter baseline; fuzz quickening transitions.

#### M3: Module Sandbox TOCTOU
**Location:** language/compiler/modules.rs:65-237: canonicalize() then later read_source_file()  
**Risk:** Concurrent filesystem modification can retarget a path after canonicalization but before read. Documented in code comment.  
**Evidence:** Comment at line 56-61 explicitly warns: "a concurrent actor able to modify the project tree can retarget a path after canonicalization and before it is read."  
**Remediation:** Open file by handle after canonicalize (O_PATH on Linux); or copy to isolated temp dir; or require OS-level isolation for untrusted projects.

#### M4: Config.toml Over-Privilege
**Location:** config.rs: ModulePermissions::from_signed_manifest() grants all declared capabilities if signed; no least-privilege default  
**Risk:** A signed module gets all capabilities it declares. No mechanism to restrict to subset. require_module_signatures is opt-in (default false).  
**Evidence:** SigningPolicy::default() has require_module_signatures: false; from_signed_manifest returns full declared set.  
**Remediation:** Default require_module_signatures = true; add capability allowlist in Config.toml per module; enforce least privilege.

#### M5: FFI Response Size Limit (16 MiB)
**Location:** ffi.rs:43: MAX_SANDBOX_RESPONSE_BYTES = 16 * 1024 * 1024  
**Risk:** Sandbox worker response buffered entirely in memory before parsing. Large responses cause OOM. No streaming/chunked processing.  
**Evidence:** exchange() reads full response into Vec<u8> before JSON parse.  
**Remediation:** Stream JSON parsing; enforce per-field limits; add response chunking protocol.

### LOW (Defense in Depth)

#### L1: Artifact Format Version Skew
**Location:** bytecode/binary.rs:143-145: Version check rejects mismatch but no forward-compatibility  
**Risk:** Future format changes break loading; no graceful degradation.  
**Remediation:** Define version negotiation; support multiple versions.

#### L2: Certificate Expiration Boundary Inconsistency
**Location:** config.rs:382 (runtime: 
ow > expires_at) vs repository-core/src/lib.rs:6274 (repo: 
ow >= expires_at)  
**Risk:** Off-by-one at expiration second; repository stricter than runtime.  
**Remediation:** Unify to half-open interval issued_at <= now < expires_at everywhere.

#### L3: Capability Granularity (Coarse)
**Location:** bytecode/program.rs:13-99: 8 capabilities only; no per-syscall, per-path, per-host controls  
**Risk:** All-or-nothing for Filesystem, Network, etc.  
**Remediation:** Add fine-grained sub-capabilities (already partially in ModulePermissions); enforce at builtin level.

#### L4: Weak Ed25519 Key Acceptance
**Location:** config.rs:320: authority.public_key.verify() not verify_strict()  
**Risk:** Weak keys admit signatures across messages (Dalek docs).  
**Remediation:** Use verify_strict(); reject weak keys at enrollment (Authority audit A-01).

#### L5: No Artifact Re-verification at Load Time
**Location:** artifact.rs:313-327: from_artifact() verifies once; no re-check if file modified  
**Risk:** TOCTOU between verification and execution if artifact file replaced.  
**Remediation:** Hash artifact at verification; re-verify hash at load; or memory-map verified artifact.

---

## Attack Scenario: Compromised Employee with Build Access

**Assumptions:** Employee has write access to CI/CD artifact storage or deployment directory.

1. **Replace sandbox worker** -> H1: Worker binary swapped; next FFI call executes attacker code outside sandbox
2. **Inject malicious dependency** -> H2: No cargo-deny; compromised crate pulled at build time
3. **Modify Config.toml** -> M4: Add malicious module with excessive capabilities; require_module_signatures=false
4. **Exploit allocator** -> M1: Crafted allocation pattern triggers UB in arena.rs
5. **TOCTOU module swap** -> M3: Replace module source after canonicalize, before read

**Mitigation coverage:** Only #3 partially mitigated (signature verification exists but opt-in). All others **unguarded**.

---

## Attack Scenario: Compromised Authority Signing Key

**Assumptions:** Attacker obtains Ed25519 private key for an Authority certificate.

1. **Sign malicious module** -> Module passes verify_module_signature() (uses verify(), not verify_strict()) -> L4
2. **Publish malicious TPack** -> Signed artifact loads with policy_trusted=true via from_trusted_artifact() -> Full capabilities granted
3. **Bypass expiration** -> Boundary inconsistency (L2) may allow 1-second window

**Mitigation coverage:** Weak key check missing (L4); expiration boundary inconsistent (L2); no key rotation automation.

---

## Test Coverage Assessment

| Area | Test File | Coverage |
|------|-----------|----------|
| Module sandbox | tests/module_sandbox.rs | Good: import containment, symlink rejection, signed module permissions |
| Artifact contract | tests/artifact_contract.rs | Good: malformed artifacts, roundtrip, capacity limits |
| FFI contract | tests/ffi_contract.rs | Good: buffer ownership, nullability, version negotiation |
| Allocator | tests/allocator_audit.rs | Exists but no fuzzing; stress tests only |
| JIT/VM | tests/runtime_parity.rs | Good: interpreter/JIT parity |
| **Missing** |: | **No supply chain tests, no sandbox worker integrity tests, no TOCTOU race tests, no weak key tests** |

---

## Immediate Recommendations (Do First)

1. **H1:** Harden sandbox_worker_path(): embed SHA-256 at build, verify on spawn
2. **H2:** Add deny.toml + cargo-deny check to CI; generate SBOM (cargo-cyclonedx)
3. **M1:** Add cargo-fuzz target for tinylang_ralloc arena alloc/dealloc
4. **M4:** Change SigningPolicy::default() to require_module_signatures = true
5. **L4:** Switch all Ed25519 verification to verify_strict(); add weak key rejection

## Short-Term (Next Sprint)

6. **M3:** Eliminate TOCTOU: open-by-handle after canonicalize, or copy to isolated dir
7. **M5:** Stream sandbox response parsing; remove full-buffer requirement
8. **M2:** Add debug assertions for JIT quickening correctness
9. **L2:** Unify expiration boundary to half-open interval everywhere
10. **L5:** Add artifact content hash verification at load time

## Long-Term (Architecture)

11. **L3:** Design fine-grained capability system (per-path, per-host, per-syscall)
12. **L1:** Define artifact format version negotiation
13. **Supply chain:** Evaluate sigstore/cosign for crate verification; hermetic reproducible builds
14. **Allocator:** Consider migrating to allocator-api2 with verified backend; or add Miri CI
15. **Monitoring:** Add audit logging for trust_artifact_policy() calls, sandbox spawns, signature verifications

---

## Conclusion

The **Authority/TPack trust boundary is correctly designed**: policy_trusted flag on Program with VerifiedProgram gate provides a clean type-level separation. Default-deny for TPack artifacts works as intended.

**However, the implementation has significant gaps:**
- **H1** and **H2** are exploitable with filesystem/build access
- **M1-M5** weaken defense-in-depth
- **No supply chain verification** exists at all
- **Test coverage misses** the highest-impact attack vectors

**Priority:** Fix H1, H2, M4 immediately. Add cargo-deny to CI this week. The remaining findings are architecturally important but require more design work.

---

## Appendix: Files Audited

| Area | Files |
|------|-------|
| Authority model | bytecode/program.rs, bytecode/certificate.rs, language/compiler/modules.rs |
| Artifact loading | bytecode/artifact.rs, bytecode/binary.rs |
| Runtime enforcement | runtime/builtins.rs, runtime/vm.rs, jit/vm.rs |
| Sandbox/FFI | ffi.rs, ffi/sandbox.rs, bin/tinylang-sandbox-worker.rs |
| Allocator | tinylang_ralloc/src/arena.rs |
| Configuration | config.rs |
| Tests | tests/module_sandbox.rs, tests/artifact_contract.rs, tests/ffi_contract.rs, tests/allocator_audit.rs |
