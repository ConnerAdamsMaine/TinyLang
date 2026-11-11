use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use crate::language::bytecode::artifact::MAX_ARTIFACT_BYTES;
use crate::language::bytecode::binary::{BINARY_ARTIFACT_MAGIC, MAX_BINARY_ARTIFACT_BYTES};
use crate::{Program, Result, TinyLangError, VerifiedProgram};

/// Maximum accepted artifact size in bytes (larger of the JSON and binary
/// format limits). Authorized readers bound their reads to this value.
pub fn artifact_byte_limit() -> usize {
    MAX_ARTIFACT_BYTES.max(MAX_BINARY_ARTIFACT_BYTES)
}

/// Loads a program artifact from disk and returns the verified [`Program`].
///
/// # Errors
///
/// Returns an error if the file cannot be read from `path`, the bytes are not
/// valid UTF-8 or JSON, the binary magic header is invalid, or bytecode
/// verification fails.
pub fn load_artifact(path: impl AsRef<Path>) -> Result<Program> {
    load_verified_artifact(path).map(VerifiedProgram::into_program)
}

/// Loads and verifies a program artifact from disk, returning a [`VerifiedProgram`].
///
/// # Errors
///
/// Returns an error if the file cannot be read from `path`, the bytes are not
/// valid UTF-8 or JSON, the binary magic header is invalid, or bytecode
/// verification fails.
pub fn load_verified_artifact(path: impl AsRef<Path>) -> Result<VerifiedProgram> {
    load_verified_untrusted_artifact(path.as_ref())
}

/// Parse and verify a program artifact from in-memory bytes.
///
/// This is the digest-binding entry point for package runtimes: verify the
/// bytes' authorized digest with [`load_authorized_artifact`] (or a trust
/// layer) and only those bytes can reach execution. No filesystem access
/// occurs here.
///
/// # Errors
///
/// Returns an error if the bytes are not valid UTF-8 or JSON, the binary
/// magic header is invalid, or bytecode verification fails.
pub fn load_verified_artifact_bytes(bytes: &[u8]) -> Result<VerifiedProgram> {
    enforce_artifact_byte_limit(bytes.len())?;
    if bytes.starts_with(BINARY_ARTIFACT_MAGIC) {
        return VerifiedProgram::from_binary_artifact(bytes);
    }
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|error| TinyLangError::compile(format!("Artifact must be UTF-8: {error}")))?;
    let data =
        serde_json::from_str(&text).map_err(|error| TinyLangError::compile(format!("Artifact JSON error: {error}")))?;
    VerifiedProgram::from_artifact(data)
}

/// Load a program artifact from in-memory bytes only after binding them to
/// the authorized digest `expected_digest`.
///
/// The bytes executed are exactly the bytes whose digest is verified; there
/// is no filesystem re-read, so a swap of the original file cannot change
/// what runs.
///
/// # Errors
///
/// Returns an error if `expected_digest` is malformed, the digest of `bytes`
/// does not match, the artifact size limit is exceeded, or parsing and
/// verification fail.
pub fn load_authorized_artifact(bytes: &[u8], expected_digest: &str) -> Result<VerifiedProgram> {
    if !expected_digest.starts_with("sha256:") || expected_digest.len() != 64 + "sha256:".len() {
        return Err(TinyLangError::compile(
            "authorized execution requires a sha256:<lowercase-hex> digest",
        ));
    }
    if !expected_digest.bytes().skip(7).all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
        return Err(TinyLangError::compile(
            "authorized execution requires a sha256:<lowercase-hex> digest",
        ));
    }
    let digest = crate::module_source_hash(bytes);
    if digest != expected_digest {
        return Err(TinyLangError::compile(format!(
            "authorized execution refused: artifact digest mismatch (expected {expected_digest}, got {digest})"
        )));
    }
    load_verified_artifact_bytes(bytes)
}

fn enforce_artifact_byte_limit(len: usize) -> Result<()> {
    let max_bytes = artifact_byte_limit();
    if len > max_bytes {
        return Err(TinyLangError::compile(format!(
            "Artifact rejected: byte size limit {max_bytes} exceeded"
        )));
    }
    Ok(())
}

fn load_verified_untrusted_artifact(path: &Path) -> Result<VerifiedProgram> {
    let bytes = read_limited_artifact(path)?;
    if bytes.starts_with(BINARY_ARTIFACT_MAGIC) {
        return VerifiedProgram::from_binary_artifact(&bytes);
    }
    let text =
        String::from_utf8(bytes).map_err(|error| TinyLangError::compile(format!("Artifact must be UTF-8: {error}")))?;
    let data =
        serde_json::from_str(&text).map_err(|error| TinyLangError::compile(format!("Artifact JSON error: {error}")))?;
    VerifiedProgram::from_artifact(data)
}

/// Serializes `program` to a pretty-printed JSON artifact and writes it to `path`.
///
/// # Errors
///
/// Returns an error if `program` cannot be serialized to JSON, or if the file
/// cannot be written to `path`.
pub fn write_artifact(program: &Program, path: impl AsRef<Path>) -> Result<()> {
    let text = serde_json::to_string_pretty(&program.to_artifact())
        .map_err(|error| TinyLangError::compile(format!("Artifact JSON error: {error}")))?;
    fs::write(path, format!("{text}\n"))
        .map_err(|error| TinyLangError::compile(format!("Artifact write error: {error}")))
}

/// Serializes `program` to its binary artifact format and writes it to `path`.
///
/// # Errors
///
/// Returns an error if `program` cannot be encoded to binary (for example if
/// an encoded field exceeds the format's `u32` size limit, or the artifact
/// exceeds [`MAX_BINARY_ARTIFACT_BYTES`]), or if the file cannot be written to
/// `path`.
pub fn write_binary_artifact(program: &Program, path: impl AsRef<Path>) -> Result<()> {
    let bytes = program.to_binary_artifact()?;
    fs::write(path, bytes).map_err(|error| TinyLangError::compile(format!("Binary artifact write error: {error}")))
}

fn read_limited_artifact(path: &Path) -> Result<Vec<u8>> {
    let mut file = File::open(path).map_err(|error| TinyLangError::compile(format!("Artifact read error: {error}")))?;
    let size = file
        .metadata()
        .map_err(|error| TinyLangError::compile(format!("Artifact metadata error: {error}")))?
        .len();
    let max_bytes = MAX_ARTIFACT_BYTES.max(MAX_BINARY_ARTIFACT_BYTES);
    if size > max_bytes as u64 {
        return Err(TinyLangError::compile(format!(
            "Artifact rejected: byte size limit {max_bytes} exceeded (got {size})"
        )));
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| TinyLangError::compile(format!("Artifact read error: {error}")))?;
    if bytes.len() > max_bytes {
        return Err(TinyLangError::compile(format!("Artifact rejected: byte size limit {max_bytes} exceeded")));
    }
    Ok(bytes)
}

#[cfg(test)]
mod authorized_artifact_tests {
    use super::*;
    use crate::module_source_hash;

    const SOURCE: &str = "fn add(left, right) {\n  return left + right\n}\nlet answer = add(40, 2)\nprint answer\n";

    #[test]
    fn authorized_artifact_roundtrip_executes_verified_bytes() {
        let program = crate::compile_source_verified(SOURCE).expect("compile");
        let bytes = program.program().to_binary_artifact().expect("binary encode");
        let expected = module_source_hash(&bytes);
        let loaded = load_authorized_artifact(&bytes, &expected).expect("authorized artifact load");
        assert_eq!(loaded.fingerprint(), program.fingerprint());
        // JSON artifacts bind the same way.
        let json = serde_json::to_vec(&program.program().to_artifact()).expect("json encode");
        let expected_json = module_source_hash(&json);
        load_authorized_artifact(&json, &expected_json).expect("authorized json artifact load");
    }

    #[test]
    fn authorized_artifact_refuses_tampered_bytes() {
        let program = crate::compile_source_verified(SOURCE).expect("compile");
        let mut bytes = program.program().to_binary_artifact().expect("binary encode");
        let expected = module_source_hash(&bytes);
        // Flip one payload byte far from the magic header.
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        let error = load_authorized_artifact(&bytes, &expected).expect_err("tampered bytes must refuse");
        assert!(error.to_string().contains("artifact digest mismatch"));
    }

    #[test]
    fn authorized_artifact_refuses_malformed_digest() {
        let program = crate::compile_source_verified(SOURCE).expect("compile");
        let bytes = program.program().to_binary_artifact().expect("binary encode");
        assert!(load_authorized_artifact(&bytes, "sha256:zz").is_err());
        assert!(load_authorized_artifact(&bytes, &format!("sha256:{}", "A".repeat(64))).is_err());
    }
}
