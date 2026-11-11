use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use crate::language::compile_cache;
use crate::{
    CompileCacheStatus, Compiler, CompilerSharedState, Lexer, ModuleCapabilities, ModulePermissions, ModuleResolver,
    PeepholeOptimizer, Program, Result, SourceMap, TinyLangError, VerifiedProgram, module_source_hash, read_source_file,
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
    VerifiedProgram::verify(compile_source_unoptimized_program(source, filename)?)
}

fn compile_source_unoptimized_program(source: &str, filename: &str) -> Result<Program> {
    let shared = Rc::new(RefCell::new(CompilerSharedState::default()));
    let compiler = Compiler::new(
        source,
        filename,
        None,
        false,
        "",
        ModulePermissions::from_capabilities(ModuleCapabilities::all()),
        shared,
    )?;
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
    let program = PeepholeOptimizer::optimize(compile_source_unoptimized_program(source, filename)?);
    VerifiedProgram::verify(program)
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
        .ok_or_else(|| TinyLangError::compile("authorized execution requires a sha256:<lowercase-hex> digest"))?;
    if encoded.len() != 64
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(TinyLangError::compile(
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
    validate_authorized_digest(expected_digest)?;
    let digest = module_source_hash(source_bytes);
    if digest != expected_digest {
        return Err(TinyLangError::compile(format!(
            "authorized execution refused: source digest mismatch (expected {expected_digest}, got {digest})"
        )));
    }
    let text = String::from_utf8(source_bytes.to_vec())
        .map_err(|error| TinyLangError::compile(format!("authorized source must be UTF-8: {error}")))?;
    compile_source_verified_with_filename(&text, filename)
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
        .map_err(|error| TinyLangError::compile(format!("File error: {error}")))?;
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
        .map_err(|error| TinyLangError::compile(format!("File error: {error}")))?;
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
                .ok_or_else(|| TinyLangError::compile("Incremental cache source missing"))?;
            if let Ok((replacement, resolver)) =
                compile_module_fragment(&candidate.module_path, &candidate.module_name, module_source, optimize)
            {
                let cached = candidate
                    .program
                    .take()
                    .ok_or_else(|| TinyLangError::compile("Incremental cache program missing"))?;
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

    const AUTHORIZED_SOURCE: &str = "fn add(left, right) {\n  return left + right\n}\nlet answer = add(40, 2)\nprint answer\n";

    fn digest_of(source: &str) -> String {
        module_source_hash(source.as_bytes())
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
        let plain = compile_source_verified(AUTHORIZED_SOURCE).expect("plain compile succeeds");
        assert_eq!(program.fingerprint(), plain.fingerprint());
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
}
