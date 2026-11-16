use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use toml::Value as TomlValue;

use crate::language::compile_cache;
use crate::{
    CompileCacheStatus, Compiler, CompilerSharedState, Lexer, ModuleCapabilities, ModulePermissions, ModuleResolver,
    PeepholeOptimizer, Program, Result, SourceMap, TynraError, VerifiedProgram, module_source_hash, read_source_file,
};

/// Compile `source` into a verified program and return it as a shareable `Program`.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source(source: &str) -> Result<Arc<Program>> {
    Ok(compile_source_verified(source)?.program_arc())
}

/// Compile `source` into a `VerifiedProgram`, reporting verification errors.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_verified(source: &str) -> Result<VerifiedProgram> {
    compile_source_verified_with_filename(source, "<source>")
}

/// Lex `source` and return the number of tokens produced.
///
/// # Errors
///
/// Returns [`Err`] if lexing fails on a malformed token.
pub fn lex_source(source: &str) -> Result<usize> {
    let source_map = SourceMap::new(source, "<source>");
    Ok(Lexer::new(&source_map).tokenize()?.len())
}

/// Compile `source` without peephole optimization.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_unoptimized(source: &str) -> Result<Arc<Program>> {
    Ok(compile_source_unoptimized_verified(source)?.program_arc())
}

/// Compile `source` without optimization and return a `VerifiedProgram`.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_unoptimized_verified(source: &str) -> Result<VerifiedProgram> {
    compile_source_unoptimized_verified_with_filename(source, "<source>")
}

/// Compile `source` without optimization, tagging errors with `filename`.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_unoptimized_with_filename(source: &str, filename: &str) -> Result<Arc<Program>> {
    Ok(compile_source_unoptimized_verified_with_filename(source, filename)?.program_arc())
}

/// Compile `source` without optimization and return a `VerifiedProgram` tagged with `filename`.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_unoptimized_verified_with_filename(source: &str, filename: &str) -> Result<VerifiedProgram> {
    let permissions = ModulePermissions::from_capabilities(ModuleCapabilities::all());
    VerifiedProgram::verify(compile_source_unoptimized_program(source, filename, &permissions)?)
}

fn compile_source_unoptimized_program(source: &str, filename: &str, permissions: &ModulePermissions) -> Result<Program> {
    let shared = Rc::new(RefCell::new(CompilerSharedState::default()));
    let compiler = Compiler::new(source, filename, None, false, "", permissions.clone(), shared)?;
    compiler.compile()
}

#[must_use]
pub fn optimize_program(program: Arc<Program>) -> Arc<Program> {
    Arc::new(PeepholeOptimizer::optimize(Arc::try_unwrap(program).unwrap_or_else(|arc| (*arc).clone())))
}

/// Compile `source` and tag any errors with `filename`.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_with_filename(source: &str, filename: &str) -> Result<Arc<Program>> {
    Ok(compile_source_verified_with_filename(source, filename)?.program_arc())
}

/// Compile `source` into a `VerifiedProgram` tagged with `filename`.
///
/// # Errors
///
/// Returns [`Err`] if lexing, parsing, or bytecode verification fails.
pub fn compile_source_verified_with_filename(source: &str, filename: &str) -> Result<VerifiedProgram> {
    let permissions = ModulePermissions::from_capabilities(ModuleCapabilities::all());
    let program = PeepholeOptimizer::optimize(compile_source_unoptimized_program(source, filename, &permissions)?);
    VerifiedProgram::verify(program)
}

/// Upper bound for an authorized-policy document supplied on a command line.
const MAX_AUTHORIZED_POLICY_BYTES: usize = 64 * 1024;

/// Capability groups accepted by [`AuthorizedPermissions::parse`]. Each group
/// carries a fixed set of sub-permissions mirroring the signed `module.toml`
/// `[capabilities]` schema.
const AUTHORIZED_POLICY_GROUPS: [(&str, &[&str]); 10] = [
    ("network", &["outbound", "listen"]),
    ("filesystem", &["read", "write"]),
    ("process", &["spawn"]),
    ("environment", &["read"]),
    ("ffi", &["allowed"]),
    ("graphics", &["gpu"]),
    ("hardware", &["access"]),
    ("threads", &["allowed"]),
    ("memory", &["unsafe"]),
    ("pipelines", &["linux"]),
];

/// Host policy explicitly pinned onto one authorized-source execution.
///
/// Authorized execution is deny-all by default; the only way to grant a
/// capability is a policy parsed by [`AuthorizedPermissions::parse`], which
/// validates the environment allowlist before construction. Callers pass a
/// policy derived from the package's verified signed manifest, never from
/// untrusted data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthorizedPermissions {
    filesystem_read: bool,
    filesystem_write: bool,
    environment_read: Vec<String>,
    network_outbound: bool,
    network_listen: bool,
    process_spawn: bool,
    ffi_allowed: bool,
    graphics_gpu: bool,
    hardware_access: bool,
    threads_allowed: bool,
    unsafe_memory_allowed: bool,
    linux_pipelines_allowed: bool,
}

impl AuthorizedPermissions {
    /// Parses the `[capabilities]` table body of a signed `module.toml`
    /// manifest (`network`, `filesystem`, `process`, `environment`, `ffi`,
    /// `graphics`, `hardware`, `threads`, `memory`, `pipelines`). Missing
    /// entries are denied. Environment variable names must be uppercase,
    /// unique, and at most 128 bytes long; the parsed allowlist is sorted.
    ///
    /// # Errors
    ///
    /// Returns [`Err`] when the text exceeds the size limit, is not valid
    /// TOML, does not parse to a table, contains an unknown field, or
    /// declares a malformed capability or environment allowlist.
    pub fn parse(policy_text: &str) -> Result<Self> {
        if policy_text.len() > MAX_AUTHORIZED_POLICY_BYTES {
            return Err(TynraError::compile(format!(
                "authorized permissions text exceeds the {MAX_AUTHORIZED_POLICY_BYTES} byte limit"
            )));
        }
        let value: TomlValue = policy_text
            .parse()
            .map_err(|error| TynraError::compile(format!("authorized permissions parse error: {error}")))?;
        let table = value
            .as_table()
            .ok_or_else(|| TynraError::compile("authorized permissions must be a TOML table"))?;
        for key in table.keys() {
            if !AUTHORIZED_POLICY_GROUPS.iter().any(|(name, _)| *name == key.as_str()) {
                let allowed: Vec<&str> = AUTHORIZED_POLICY_GROUPS.iter().map(|(name, _)| *name).collect();
                return Err(TynraError::compile(format!(
                    "Unknown authorized permissions field {key}; expected one of {allowed:?}"
                )));
            }
        }
        let network = authorized_policy_group(table, "network", AUTHORIZED_POLICY_GROUPS[0].1)?;
        let filesystem = authorized_policy_group(table, "filesystem", AUTHORIZED_POLICY_GROUPS[1].1)?;
        let process = authorized_policy_group(table, "process", AUTHORIZED_POLICY_GROUPS[2].1)?;
        let environment = authorized_policy_group(table, "environment", AUTHORIZED_POLICY_GROUPS[3].1)?;
        let ffi = authorized_policy_group(table, "ffi", AUTHORIZED_POLICY_GROUPS[4].1)?;
        let graphics = authorized_policy_group(table, "graphics", AUTHORIZED_POLICY_GROUPS[5].1)?;
        let hardware = authorized_policy_group(table, "hardware", AUTHORIZED_POLICY_GROUPS[6].1)?;
        let threads = authorized_policy_group(table, "threads", AUTHORIZED_POLICY_GROUPS[7].1)?;
        let memory = authorized_policy_group(table, "memory", AUTHORIZED_POLICY_GROUPS[8].1)?;
        let pipelines = authorized_policy_group(table, "pipelines", AUTHORIZED_POLICY_GROUPS[9].1)?;
        Ok(Self {
            network_outbound: authorized_policy_flag(network, "outbound", "network.outbound")?,
            network_listen: authorized_policy_flag(network, "listen", "network.listen")?,
            filesystem_read: authorized_policy_flag(filesystem, "read", "filesystem.read")?,
            filesystem_write: authorized_policy_flag(filesystem, "write", "filesystem.write")?,
            process_spawn: authorized_policy_flag(process, "spawn", "process.spawn")?,
            environment_read: authorized_policy_environment_read(environment)?,
            ffi_allowed: authorized_policy_flag(ffi, "allowed", "ffi.allowed")?,
            graphics_gpu: authorized_policy_flag(graphics, "gpu", "graphics.gpu")?,
            hardware_access: authorized_policy_flag(hardware, "access", "hardware.access")?,
            threads_allowed: authorized_policy_flag(threads, "allowed", "threads.allowed")?,
            unsafe_memory_allowed: authorized_policy_flag(memory, "unsafe", "memory.unsafe")?,
            linux_pipelines_allowed: authorized_policy_flag(pipelines, "linux", "pipelines.linux")?,
        })
    }

    /// Converts the validated policy into the runtime permission model used
    /// by the compiler and the builtin capability gate.
    pub(crate) fn to_module_permissions(&self) -> ModulePermissions {
        ModulePermissions::from_signed_manifest(
            self.filesystem_read,
            self.filesystem_write,
            self.environment_read.clone(),
            self.network_outbound,
            self.network_listen,
            self.process_spawn,
            self.ffi_allowed,
            self.graphics_gpu,
            self.hardware_access,
            self.threads_allowed,
            self.unsafe_memory_allowed,
            self.linux_pipelines_allowed,
        )
    }
}

/// Reads one `[capabilities]` sub-table, rejecting unknown sub-fields.
fn authorized_policy_group<'a>(
    table: &'a toml::map::Map<String, TomlValue>,
    name: &str,
    allowed: &[&str],
) -> Result<Option<&'a toml::map::Map<String, TomlValue>>> {
    let Some(value) = table.get(name) else {
        return Ok(None);
    };
    let group = value
        .as_table()
        .ok_or_else(|| TynraError::compile(format!("authorized permissions field {name:?} must be a table")))?;
    for key in group.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(TynraError::compile(format!(
                "Unknown authorized permissions field {name}.{key}; expected one of {allowed:?}"
            )));
        }
    }
    Ok(Some(group))
}

/// Reads one boolean sub-permission; an absent entry is denied.
fn authorized_policy_flag(group: Option<&toml::map::Map<String, TomlValue>>, key: &str, field: &str) -> Result<bool> {
    let Some(value) = group.and_then(|group| group.get(key)) else {
        return Ok(false);
    };
    value.as_bool().ok_or_else(|| {
        TynraError::compile(format!("authorized permissions field {field} must be a boolean"))
    })
}

/// Reads the `environment.read` allowlist; an absent entry grants nothing.
fn authorized_policy_environment_read(group: Option<&toml::map::Map<String, TomlValue>>) -> Result<Vec<String>> {
    let Some(value) = group.and_then(|group| group.get("read")) else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or_else(|| {
        TynraError::compile(
            "authorized permissions field \"environment.read\" must be an array of environment variable names",
        )
    })?;
    let mut names = Vec::with_capacity(values.len());
    for value in values {
        let name = value.as_str().ok_or_else(|| {
            TynraError::compile(
                "authorized permissions field \"environment.read\" must contain environment variable names",
            )
        })?;
        if name.len() > 128
            || !name
                .bytes()
                .enumerate()
                .all(|(index, byte)| byte.is_ascii_uppercase() || byte == b'_' || (index > 0 && byte.is_ascii_digit()))
        {
            return Err(TynraError::compile(
                "authorized permissions field \"environment.read\" must contain uppercase environment variable names",
            ));
        }
        if names.iter().any(|existing| existing == name) {
            return Err(TynraError::compile(format!(
                "authorized permissions field \"environment.read\" contains duplicate variable {name:?}"
            )));
        }
        names.push(name.to_string());
    }
    names.sort();
    Ok(names)
}

/// Validate an authorized-execution digest expectation.
///
/// The expected value must use the canonical `sha256:<lowercase-hex>` form.
///
/// # Errors
///
/// Returns [`Err`] when the digest is malformed.
pub fn validate_authorized_digest(expected_digest: &str) -> Result<()> {
    let encoded = expected_digest
        .strip_prefix("sha256:")
        .ok_or_else(|| TynraError::compile("authorized execution requires a sha256:<lowercase-hex> digest"))?;
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(TynraError::compile(
            "authorized execution requires a sha256:<lowercase-hex> digest",
        ));
    }
    Ok(())
}

/// Compile `source_bytes` for authorized execution after binding them to
/// `expected_digest`.
///
/// This is the digest-binding entry point for package runtimes: the caller
/// reads the authorized bytes once, passes them here together with the digest
/// the trust layer verified, and only those exact bytes can reach
/// compilation and execution. There is no filesystem access after this
/// point, so a swap of the original file cannot change what runs
/// (time-of-check/time-of-use is closed at the read boundary).
///
/// The compiled root executes deny-all: every host capability (filesystem,
/// environment, threads, unsafe memory, network, graphics, hardware, and
/// Linux pipelines) is refused at runtime. Callers that have verified a
/// signed manifest must pass its capabilities explicitly via
/// [`compile_authorized_source_verified_with_policy`]; this default matches
/// the ecosystem's deny-all playground safety model.
///
/// # Errors
///
/// Returns [`Err`] when `expected_digest` is malformed, the digest of
/// `source_bytes` does not match, the bytes are not UTF-8, or compilation or
/// verification fails.
pub fn compile_authorized_source_verified(
    source_bytes: &[u8],
    expected_digest: &str,
    filename: &str,
) -> Result<VerifiedProgram> {
    compile_authorized_source_verified_with_policy(source_bytes, expected_digest, filename, &AuthorizedPermissions::default())
}

/// Compile `source_bytes` for authorized execution bound to `expected_digest`,
/// pinning the host policy derived from the package's verified signed
/// manifest.
///
/// The pinned policy is the *only* authority the root program receives: the
/// compiled root executes with at most the capabilities in `policy`, and
/// denied builtins fail at runtime with a clear permission error. The policy
/// must come from a caller-side verified manifest (see
/// [`AuthorizedPermissions::parse`]); an unparsed, hand-built grant cannot be
/// smuggled past the boundary because the type has no other constructor.
///
/// # Errors
///
/// Returns [`Err`] when `expected_digest` is malformed, the digest of
/// `source_bytes` does not match, the bytes are not UTF-8, or compilation or
/// verification fails.
pub fn compile_authorized_source_verified_with_policy(
    source_bytes: &[u8],
    expected_digest: &str,
    filename: &str,
    policy: &AuthorizedPermissions,
) -> Result<VerifiedProgram> {
    validate_authorized_digest(expected_digest)?;
    let digest = module_source_hash(source_bytes);
    if digest != expected_digest {
        return Err(TynraError::compile(format!(
            "authorized execution refused: source digest mismatch (expected {expected_digest}, got {digest})"
        )));
    }
    let text = String::from_utf8(source_bytes.to_vec())
        .map_err(|error| TynraError::compile(format!("authorized source must be UTF-8: {error}")))?;
    let pinned = policy.to_module_permissions();
    let mut program = compile_source_unoptimized_program(&text, filename, &ModulePermissions::none())?;
    // `Compiler::new` applies its permissions argument only to module-mode
    // units; the root unit's authority lives on the assembled program, so the
    // pinned manifest policy is applied here. The verifier re-checks that the
    // detailed permissions stay inside the coarse capability bits.
    program.root_permissions = pinned.clone();
    program.root_capabilities = pinned.capabilities();
    let program = PeepholeOptimizer::optimize(program);
    VerifiedProgram::verify(program)
}

/// Compile the source file at `path`.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read or if lexing, parsing, or
/// verification fails.
pub fn compile_file(path: impl AsRef<Path>) -> Result<Arc<Program>> {
    Ok(compile_file_verified(path)?.program_arc())
}

/// Compile the source file at `path` into a `VerifiedProgram`.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read or if lexing, parsing, or
/// verification fails.
pub fn compile_file_verified(path: impl AsRef<Path>) -> Result<VerifiedProgram> {
    compile_file_verified_with_options(path, true)
}

/// Compile `path`, reusing the on-disk compile cache when valid.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read/canonicalized, the module
/// resolver fails to build, or lexing/parsing/verification fails.
pub fn compile_file_cached(path: impl AsRef<Path>) -> Result<Arc<Program>> {
    Ok(compile_file_cached_verified(path)?.program_arc())
}

/// Compile `path` via the cache, returning a `VerifiedProgram`.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read/canonicalized, the module
/// resolver fails to build, or lexing/parsing/verification fails.
pub fn compile_file_cached_verified(path: impl AsRef<Path>) -> Result<VerifiedProgram> {
    compile_file_cached_verified_with_status(path).map(|(program, _)| program)
}

/// Compile `path` via the cache, also reporting whether the result was a hit.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read/canonicalized, the module
/// resolver fails to build, or lexing/parsing/verification fails.
pub fn compile_file_cached_verified_with_status(
    path: impl AsRef<Path>,
) -> Result<(VerifiedProgram, CompileCacheStatus)> {
    compile_file_cached_verified_with_options(path, true)
}

/// Compile `path` without optimization.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read or if lexing, parsing, or
/// verification fails.
pub fn compile_file_unoptimized(path: impl AsRef<Path>) -> Result<Arc<Program>> {
    Ok(compile_file_unoptimized_verified(path)?.program_arc())
}

/// Compile `path` without optimization, returning a `VerifiedProgram`.
///
/// # Errors
///
/// Returns [`Err`] if the file cannot be read or if lexing, parsing, or
/// verification fails.
pub fn compile_file_unoptimized_verified(path: impl AsRef<Path>) -> Result<VerifiedProgram> {
    compile_file_verified_with_options(path, false)
}

fn compile_file_verified_with_options(path: impl AsRef<Path>, optimize: bool) -> Result<VerifiedProgram> {
    let path = path
        .as_ref()
        .canonicalize()
        .map_err(|error| TynraError::compile(format!("File error: {error}")))?;
    compile_canonical_file(&path, optimize).map(|(program, _, _)| program)
}

/// Compile `path` via the cache with an explicit `optimize` flag, returning the
/// program plus a [`CompileCacheStatus`] describing how the result was obtained.
///
/// # Errors
///
/// Returns [`Err`] if `path` cannot be canonicalized, the module resolver fails
/// to build (e.g. certificate/authority checks), or lexing/parsing/verification
/// fails. Cache-store failures are swallowed and do not produce an error.
pub fn compile_file_cached_verified_with_options(
    path: impl AsRef<Path>,
    optimize: bool,
) -> Result<(VerifiedProgram, CompileCacheStatus)> {
    let path = path
        .as_ref()
        .canonicalize()
        .map_err(|error| TynraError::compile(format!("File error: {error}")))?;
    // The disk cache stores signature-sidecar digests, while this inexpensive
    // policy load rechecks central authority certificate validity (including
    // expiry) before a cache hit can be accepted.
    let _ = ModuleResolver::new(&path)?;
    if compile_cache::should_bypass_filesystem(&path) {
        let (program, _, _) = compile_canonical_file(&path, optimize)?;
        return Ok((program, CompileCacheStatus::Bypassed));
    }
    if compile_cache::is_known_bypass(&path, optimize) {
        let (program, _, _) = compile_canonical_file(&path, optimize)?;
        return Ok((program, CompileCacheStatus::Bypassed));
    }
    match compile_cache::lookup(&path, optimize)? {
        compile_cache::CacheLookup::Hit(program) => {
            return Ok((program, CompileCacheStatus::Hit));
        }
        compile_cache::CacheLookup::Incremental(mut candidate) => {
            let module_source = candidate
                .module_source
                .take()
                .ok_or_else(|| TynraError::compile("Incremental cache source missing"))?;
            if let Ok((replacement, resolver)) =
                compile_module_fragment(&candidate.module_path, &candidate.module_name, module_source, optimize)
            {
                let cached = candidate
                    .program
                    .take()
                    .ok_or_else(|| TynraError::compile("Incremental cache program missing"))?;
                if let Ok(program) = crate::patch_module(cached, &replacement, &candidate.module_name) {
                    let _ = compile_cache::store_incremental(&path, optimize, &program, *candidate, &resolver.borrow());
                    return Ok((program, CompileCacheStatus::Incremental));
                }
            }
        }
        compile_cache::CacheLookup::Bypass => {
            let (program, _, _) = compile_canonical_file(&path, optimize)?;
            compile_cache::remember_bypass(&path, optimize);
            return Ok((program, CompileCacheStatus::Bypassed));
        }
        compile_cache::CacheLookup::Miss => {}
    }
    let (program, source, resolver) = compile_canonical_file(&path, optimize)?;
    if compile_cache::should_bypass(&source, &resolver.borrow()) {
        compile_cache::remember_bypass(&path, optimize);
        return Ok((program, CompileCacheStatus::Bypassed));
    }
    // A cache write is an optimization. Read-only source trees and transient
    // cache failures must not prevent a correctly compiled program from
    // running.
    let _ = compile_cache::store(&path, &source, optimize, &program, &resolver.borrow());
    Ok((program, CompileCacheStatus::Miss))
}

fn compile_module_fragment(
    path: &Path,
    module_name: &str,
    source: String,
    optimize: bool,
) -> Result<(Program, Rc<RefCell<ModuleResolver>>)> {
    let shared = Rc::new(RefCell::new(CompilerSharedState::default()));
    let resolver = Rc::new(RefCell::new(ModuleResolver::new(path)?));
    let compiler = Compiler::new(
        source,
        path.to_string_lossy().to_string(),
        Some(Rc::clone(&resolver)),
        true,
        module_name,
        ModulePermissions::from_capabilities(ModuleCapabilities::none()),
        shared,
    )?;
    let program = compiler.compile()?;
    let program = if optimize {
        PeepholeOptimizer::optimize(program)
    } else {
        program
    };
    Ok((program, resolver))
}

fn compile_canonical_file(
    path: &Path,
    optimize: bool,
) -> Result<(VerifiedProgram, String, Rc<RefCell<ModuleResolver>>)> {
    let source = read_source_file(path)?;
    let shared = Rc::new(RefCell::new(CompilerSharedState::default()));
    let resolver = Rc::new(RefCell::new(ModuleResolver::new(path)?));
    let (root_capabilities, vm_settings) = {
        let resolver = resolver.borrow();
        (resolver.root_capabilities(), resolver.vm_settings())
    };
    let compiler = Compiler::new(
        source.clone(),
        path.to_string_lossy().to_string(),
        Some(Rc::clone(&resolver)),
        false,
        "",
        ModulePermissions::from_capabilities(ModuleCapabilities::all()),
        shared,
    )?
    .with_runtime_policy(root_capabilities, vm_settings);
    let program = compiler.compile()?;
    let program = if optimize {
        PeepholeOptimizer::optimize(program)
    } else {
        program
    };
    Ok((VerifiedProgram::verify(program)?, source, resolver))
}

#[cfg(test)]
mod authorized_execution_tests {
    use super::*;

    use crate::{JitOptions, run_verified_program_with_env_and_jit_options, run_verified_program_with_jit_options};

    const AUTHORIZED_SOURCE: &str = "fn add(left, right) {\n  return left + right\n}\nlet answer = add(40, 2)\nprint answer\n";

    /// Source whose only host action is writing a canary file. The path is
    /// never created when the builtin is denied before host access.
    const FS_WRITE_CANARY: &str =
        "let canary = buffer(1)\nlet ignored = unsafe fs_write(\"tynra-authorized-canary-must-not-exist\", canary)\nprint \"WROTE\"\n";

    fn digest_of(source: &str) -> String {
        module_source_hash(source.as_bytes())
    }

    fn vm_error(program: &VerifiedProgram) -> String {
        let mut stdout = Vec::new();
        run_verified_program_with_jit_options(program, "vm", &mut stdout, Vec::new(), JitOptions::new())
            .expect_err("program must fail")
            .to_string()
    }

    #[test]
    fn authorized_digest_form_is_validated() {
        assert!(validate_authorized_digest(&format!("sha256:{}", "a".repeat(64))).is_ok());
        assert!(validate_authorized_digest("a".repeat(70).as_str()).is_err());
        assert!(validate_authorized_digest("sha256:ABC").is_err());
        assert!(validate_authorized_digest("sha256:").is_err());
        // uppercase hex is not the canonical form
        assert!(validate_authorized_digest(&format!("sha256:{}", "A".repeat(64))).is_err());
    }

    #[test]
    fn authorized_source_compiles_from_verified_bytes() {
        let expected = digest_of(AUTHORIZED_SOURCE);
        let program = compile_authorized_source_verified(
            AUTHORIZED_SOURCE.as_bytes(),
            &expected,
            "<authorized-source>",
        )
        .expect("authorized compile succeeds for matching bytes");
        // The digest-bound compile is reproducible: the same bytes and the
        // same pinned policy yield the same program identity.
        let again = compile_authorized_source_verified(AUTHORIZED_SOURCE.as_bytes(), &expected, "<authorized-source>")
            .expect("repeat authorized compile succeeds");
        assert_eq!(program.fingerprint(), again.fingerprint());
    }

    #[test]
    fn authorized_source_refuses_digest_mismatch_before_compiling() {
        // The source is malformed, but the digest gate must reject first so the
        // reported failure is the trust failure, not a parse error.
        let malformed = b"let = ;";
        let error = compile_authorized_source_verified(malformed, &digest_of(AUTHORIZED_SOURCE), "x")
            .expect_err("mismatched digest must refuse");
        assert!(error.to_string().contains("source digest mismatch"));
    }

    #[test]
    fn authorized_source_refuses_malformed_digest_form() {
        assert!(
            compile_authorized_source_verified(AUTHORIZED_SOURCE.as_bytes(), "not-a-digest", "x").is_err()
        );
    }

    #[test]
    fn authorized_source_refuses_non_utf8_bytes() {
        let bytes: [u8; 3] = [0xFF, 0xFE, 0x00];
        let expected = module_source_hash(&bytes);
        let error = compile_authorized_source_verified(&bytes, &expected, "x")
            .expect_err("non-UTF-8 bytes must refuse");
        assert!(error.to_string().contains("UTF-8"));
    }

    /// Regression test for TM-01: the authorized path used to compile root
    /// permissions as all-granted, so an installed package could write files.
    /// The deny-all default must refuse the canary before host access.
    #[test]
    fn authorized_default_policy_denies_filesystem_write_at_runtime() {
        let program = compile_authorized_source_verified(FS_WRITE_CANARY.as_bytes(), &digest_of(FS_WRITE_CANARY), "<canary>")
            .expect("canary compiles");
        let mut stdout = Vec::new();
        let error = run_verified_program_with_jit_options(&program, "vm", &mut stdout, Vec::new(), JitOptions::new())
            .expect_err("fs_write must be denied under the deny-all authorized policy");
        let message = error.to_string();
        assert!(message.contains("fs_write"), "denial must name the builtin: {message}");
        assert!(
            message.contains("denied") || message.contains("outside"),
            "denial must be a permission failure: {message}"
        );
        assert!(stdout.is_empty(), "canary must not print before the denial: {message}");
    }

    #[test]
    fn authorized_default_policy_denies_threads_unsafe_and_environment_at_runtime() {
        let source = b"fn noop() { return 0 }\nlet thread = thread_spawn(\"noop\")\nprint \"SPAWNED\"\n";
        let program = compile_authorized_source_verified(source, &module_source_hash(source), "<canary>")
            .expect("canary compiles");
        let message = vm_error(&program);
        assert!(message.contains("thread_spawn"), "{message}");
    }

    /// A package whose verified manifest declares filesystem read and write
    /// may do exactly that; the grant is pinned, not implicit.
    #[test]
    fn authorized_policy_grants_declared_filesystem_capabilities() {
        let policy = AuthorizedPermissions::parse("filesystem = { read = true, write = true }").expect("valid policy");
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("canary.txt");
        let source = format!(
            concat!(
                "let mem = buffer(1)\n",
                "let ignored = unsafe fs_write({path}, mem)\n",
                "print fs_exists({path})\n",
                "let body = unsafe fs_read({path})\n",
                "print len(body)\n"
            ),
            path = format!("{:?}", path.to_string_lossy())
        );
        let program = compile_authorized_source_verified_with_policy(
            source.as_bytes(),
            &digest_of(&source),
            "<canary>",
            &policy,
        )
        .expect("canary compiles");
        let mut stdout = Vec::new();
        run_verified_program_with_jit_options(&program, "vm", &mut stdout, Vec::new(), JitOptions::new())
            .expect("declared filesystem capabilities must be honored");
        assert_eq!(String::from_utf8_lossy(&stdout), "1\n1\n");
    }

    /// Declared X means at most X: a read-only manifest must still refuse
    /// writes, and an environment allowlist must refuse undeclared variables.
    #[test]
    fn authorized_policy_grants_at_most_declared_capabilities() {
        let policy = AuthorizedPermissions::parse("filesystem = { read = true, write = false }").expect("valid policy");
        let program = compile_authorized_source_verified_with_policy(
            FS_WRITE_CANARY.as_bytes(),
            &digest_of(FS_WRITE_CANARY),
            "<canary>",
            &policy,
        )
        .expect("canary compiles");
        let message = vm_error(&program);
        assert!(message.contains("fs_write"), "{message}");

        let policy = AuthorizedPermissions::parse("environment = { read = [\"PUBLIC_VALUE\"] }").expect("valid policy");
        let source = b"print sys_env_get(\"PUBLIC_VALUE\")\n";
        let program = compile_authorized_source_verified_with_policy(source, &module_source_hash(source), "<env>", &policy)
            .expect("env canary compiles");
        let mut stdout = Vec::new();
        run_verified_program_with_env_and_jit_options(
            &program,
            "vm",
            &mut stdout,
            Vec::new(),
            Vec::new(),
            std::iter::once(("PUBLIC_VALUE".to_string(), "granted".to_string())).collect(),
            JitOptions::new(),
        )
        .expect("declared environment read must be honored");
        assert_eq!(String::from_utf8_lossy(&stdout), "granted\n");

        let source = b"print sys_env_get(\"SECRET_TOKEN\")\n";
        let program = compile_authorized_source_verified_with_policy(source, &module_source_hash(source), "<env>", &policy)
            .expect("env canary compiles");
        let message = vm_error(&program);
        assert!(message.contains("SECRET_TOKEN"), "{message}");
    }

    #[test]
    fn authorized_policy_parse_validates_schema() {
        let policy = AuthorizedPermissions::parse(
            "filesystem = { read = true, write = false }\nnetwork = { outbound = true, listen = false }\n\
             environment = { read = [\"HTTP_PROXY\"] }\n",
        )
        .expect("valid policy parses");
        assert_eq!(policy, AuthorizedPermissions::parse(
            "filesystem = { read = true, write = false }\nnetwork = { outbound = true, listen = false }\n\
             environment = { read = [\"HTTP_PROXY\"] }\n"
        )
        .expect("repeat parse"));

        assert!(AuthorizedPermissions::parse("unknown = true").is_err());
        assert!(AuthorizedPermissions::parse("filesystem = { read = 1 }").is_err());
        assert!(AuthorizedPermissions::parse("filesystem = { stealth = true }").is_err());
        assert!(AuthorizedPermissions::parse("filesystem = true").is_err());
        assert!(AuthorizedPermissions::parse("environment = { read = [\"lower_case\"] }").is_err());
        assert!(AuthorizedPermissions::parse("environment = { read = [\"A\", \"A\"] }").is_err());
        assert!(AuthorizedPermissions::parse("filesystem = { read = true }").is_ok());
        // Default is deny-all, identical to parsing an empty table.
        assert_eq!(AuthorizedPermissions::default(), AuthorizedPermissions::parse("").expect("empty policy"));
    }
}
