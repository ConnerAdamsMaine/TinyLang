mod origins;

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};

use self::origins::SourceInput;
use crate::{
    ModuleResolver, ResolverInput, Result, TinyLangError, VerifiedProgram, content_digest, load_verified_artifact,
    write_binary_artifact,
};

// v9 requires a private, snapshot-bound source-origin sidecar. v8's signature
// expiry checks remain authoritative; source metadata never grants authority.
const CACHE_FORMAT_VERSION: u32 = 10;
const MAX_CACHE_METADATA_BYTES: usize = 1024 * 1024;
const MAX_CACHE_INPUT_BYTES: usize = 1024 * 1024;
const SMALL_CACHE_MAX_MODULES: usize = 2;
const SMALL_CACHE_MAX_SOURCE_BYTES: usize = 4 * 1024;
const WINDOWS_MEDIUM_CACHE_MAX_MODULES: usize = 16;
const WINDOWS_MEDIUM_CACHE_MAX_SOURCE_BYTES: usize = 64 * 1024;
const BYPASS_MEMORY_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileCacheStatus {
    /// A dependency-validated binary artifact was loaded.
    Hit,
    /// One changed module was rebuilt and patched into a validated artifact.
    Incremental,
    /// No reusable cache entry existed, so a new entry was written after compilation.
    Miss,
    /// The graph was compiled directly because cache validation would cost more than rebuilding it.
    Bypassed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheRecord {
    format_version: u32,
    compiler_version: String,
    optimize: bool,
    root: PathBuf,
    fingerprint: String,
    module_count: usize,
    input_bytes: usize,
    /// Earliest expiry among module signatures verified during compilation.
    /// `None` means the source graph did not require module signatures.
    signature_expires_at: Option<u64>,
    inputs: Vec<CacheInput>,
    resolutions: Vec<CacheResolution>,
    modules: Vec<CacheModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheInput {
    path: PathBuf,
    digest: Option<String>,
    identity: Option<CacheInputIdentity>,
}

/// A cheap filesystem identity used only to detect a definitely stale record.
/// A matching identity never authorizes a cache hit; content digests remain the
/// trust decision because timestamps and lengths can be preserved or spoofed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CacheInputIdentity {
    size_bytes: u64,
    modified_unix_nanos: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheResolution {
    candidate: PathBuf,
    canonical: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheModule {
    path: PathBuf,
    name: String,
}

pub(crate) struct IncrementalCandidate {
    pub(crate) program: Option<VerifiedProgram>,
    pub(crate) module_path: PathBuf,
    pub(crate) module_name: String,
    pub(crate) module_source: Option<String>,
    pub(crate) module_digest: [u8; 16],
    record: CacheRecord,
}

pub(crate) enum CacheLookup {
    Hit(VerifiedProgram),
    Incremental(Box<IncrementalCandidate>),
    Bypass,
    Miss,
}

pub(crate) fn lookup(root: &Path, optimize: bool) -> Result<CacheLookup> {
    let paths = cache_paths(root, optimize);
    let Some(record) = load_record(&paths.metadata) else {
        return Ok(CacheLookup::Miss);
    };
    if !record_matches_request(&record, root, optimize) {
        return Ok(CacheLookup::Miss);
    }
    if !signature_cache_entry_is_current(record.signature_expires_at)? {
        return Ok(CacheLookup::Miss);
    }
    if should_bypass_shape(record.module_count, record.input_bytes) {
        return Ok(CacheLookup::Bypass);
    }

    // This prefilter may only reject a record. A matching identity deliberately
    // falls through to full digest validation below.
    let metadata_changed = record
        .inputs
        .iter()
        .filter(|input| input_metadata_match(input) == InputMetadataMatch::Changed)
        .collect::<Vec<_>>();
    if metadata_changed.len() > 1
        || metadata_changed
            .first()
            .is_some_and(|input| input.path == root || input.digest.is_none())
    {
        return Ok(CacheLookup::Miss);
    }

    let Some(verified) = match_cached_program(&paths.artifact, &record.fingerprint) else {
        return Ok(CacheLookup::Miss);
    };
    if record.modules.len() != verified.program().modules.len() {
        return Ok(CacheLookup::Miss);
    }
    let Ok(origin_bytes) = read_bounded(&paths.origins, origins::encoded_length(&verified)) else {
        return Ok(CacheLookup::Miss);
    };
    // Keep only source bytes needed for diagnostics, not manifest/signature
    // bytes. The verified module bound and per-file limit cap aggregate
    // retention even if malformed metadata repeats the same source input.
    let source_byte_limit = (verified.program().modules.len() + 1) * MAX_CACHE_INPUT_BYTES;
    let mut retained_source_bytes = 0usize;
    let mut source_inputs = Vec::with_capacity(record.inputs.len());
    let mut changed = None;
    for (index, input) in record.inputs.iter().enumerate() {
        let source = match match_input(input) {
            InputMatch::Same(bytes)
                if input.path == root || record.modules.iter().any(|module| module.path == input.path) =>
            {
                retained_source_bytes = retained_source_bytes.saturating_add(bytes.capacity());
                if retained_source_bytes > source_byte_limit {
                    return Ok(CacheLookup::Miss);
                }
                SourceInput::Bytes(bytes)
            }
            InputMatch::Same(_) | InputMatch::Absent => SourceInput::Unavailable,
            InputMatch::Changed { bytes, digest } => {
                if changed.is_some() || input.path == root {
                    return Ok(CacheLookup::Miss);
                }
                changed = Some((index, bytes, digest));
                SourceInput::Unavailable
            }
            InputMatch::Invalid => return Ok(CacheLookup::Miss),
        };
        source_inputs.push(source);
    }
    if !resolutions_match(&record.resolutions) {
        return Ok(CacheLookup::Miss);
    }

    let Some((index, bytes, digest)) = changed else {
        return Ok(origins::restore(verified, &origin_bytes, &record, source_inputs, None)
            .map_or(CacheLookup::Miss, CacheLookup::Hit));
    };
    let path = &record.inputs[index].path;
    let Ok(source) = String::from_utf8(bytes) else {
        return Ok(CacheLookup::Miss);
    };
    let Some(module) = record.modules.iter().find(|module| module.path == *path) else {
        return Ok(CacheLookup::Miss);
    };
    let module_name = module.name.clone();
    let Some(verified) = origins::restore(verified, &origin_bytes, &record, source_inputs, Some(&module_name)) else {
        return Ok(CacheLookup::Miss);
    };
    Ok(CacheLookup::Incremental(Box::new(IncrementalCandidate {
        program: Some(verified),
        module_path: path.clone(),
        module_name,
        module_source: Some(source),
        module_digest: digest,
        record,
    })))
}

fn match_cached_program(path: &Path, expected_fingerprint: &str) -> Option<VerifiedProgram> {
    // Cache files live beside project sources and are not an authenticated
    // authority store. Decode them as untrusted data; any entry that dispatches
    // a builtin or requests coprocessor placement is rebuilt from current source/configuration rather than
    // allowed to re-grant serialized policy.
    let program = load_verified_artifact(path).ok()?;
    (!program.program().needs_runtime_host_permissions() && program.fingerprint() == expected_fingerprint)
        .then_some(program)
}

pub(crate) fn should_bypass(root_source: &str, resolver: &ModuleResolver) -> bool {
    resolver.has_capability_grants()
        || should_bypass_shape(
            resolver.module_count(),
            root_source.len().saturating_add(resolver.existing_input_bytes()),
        )
}

fn should_bypass_shape(module_count: usize, input_bytes: usize) -> bool {
    (module_count <= SMALL_CACHE_MAX_MODULES && input_bytes <= SMALL_CACHE_MAX_SOURCE_BYTES)
        || (cfg!(windows)
            && (3..=WINDOWS_MEDIUM_CACHE_MAX_MODULES).contains(&module_count)
            && input_bytes <= WINDOWS_MEDIUM_CACHE_MAX_SOURCE_BYTES)
}

pub(crate) fn should_bypass_filesystem(root: &Path) -> bool {
    #[cfg(target_os = "linux")]
    {
        static IS_WSL: OnceLock<bool> = OnceLock::new();
        let is_wsl = *IS_WSL.get_or_init(|| {
            fs::read_to_string("/proc/sys/kernel/osrelease")
                .or_else(|_| fs::read_to_string("/proc/version"))
                .is_ok_and(|text| text.to_ascii_lowercase().contains("microsoft"))
        });
        if is_wsl {
            let path = root.to_string_lossy();
            if let Some(suffix) = path.strip_prefix("/mnt/") {
                let bytes = suffix.as_bytes();
                return bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b'/');
            }
        }
    }
    let _ = root;
    false
}

pub(crate) fn is_known_bypass(root: &Path, optimize: bool) -> bool {
    let mut entries = bypass_entries()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(index) = entries
        .iter()
        .position(|entry| entry.root == root && entry.optimize == optimize)
    else {
        return false;
    };
    let entry = entries.remove(index).expect("known bypass entry");
    entries.push_back(entry);
    true
}

pub(crate) fn remember_bypass(root: &Path, optimize: bool) {
    let mut entries = bypass_entries()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(index) = entries
        .iter()
        .position(|entry| entry.root == root && entry.optimize == optimize)
    {
        entries.remove(index);
    }
    entries.push_back(BypassEntry {
        root: root.to_path_buf(),
        optimize,
    });
    while entries.len() > BYPASS_MEMORY_LIMIT {
        entries.pop_front();
    }
}

#[derive(Debug)]
struct BypassEntry {
    root: PathBuf,
    optimize: bool,
}

fn bypass_entries() -> &'static Mutex<VecDeque<BypassEntry>> {
    static ENTRIES: OnceLock<Mutex<VecDeque<BypassEntry>>> = OnceLock::new();
    ENTRIES.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(crate) fn store(
    root: &Path,
    root_source: &str,
    optimize: bool,
    verified: &VerifiedProgram,
    resolver: &ModuleResolver,
) -> Result<()> {
    let paths = cache_paths(root, optimize);
    let mut inputs = BTreeMap::new();
    inputs.insert(root.to_path_buf(), Some(content_digest(root_source.as_bytes())));
    for ResolverInput { path, digest } in resolver.inputs() {
        inputs.insert(path, digest);
    }
    let record = CacheRecord {
        format_version: CACHE_FORMAT_VERSION,
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        optimize,
        root: root.to_path_buf(),
        fingerprint: verified.fingerprint().to_string(),
        module_count: resolver.module_count(),
        input_bytes: root_source.len().saturating_add(resolver.existing_input_bytes()),
        signature_expires_at: resolver.signed_module_valid_until(),
        inputs: inputs
            .into_iter()
            .map(|(path, digest)| CacheInput {
                identity: digest.as_ref().and_then(|_| cache_input_identity(&path)),
                path,
                digest: digest.map(hex::encode),
            })
            .collect(),
        resolutions: resolver
            .canonical_resolutions()
            .into_iter()
            .map(|(candidate, canonical)| CacheResolution { candidate, canonical })
            .collect(),
        modules: resolver
            .module_names()
            .into_iter()
            .map(|(path, name)| CacheModule { path, name })
            .collect(),
    };
    store_record(&paths, verified, &record)
}

pub(crate) fn store_incremental(
    root: &Path,
    optimize: bool,
    verified: &VerifiedProgram,
    mut candidate: IncrementalCandidate,
    resolver: &ModuleResolver,
) -> Result<()> {
    let paths = cache_paths(root, optimize);
    let mut inputs = candidate
        .record
        .inputs
        .into_iter()
        .map(|input| (input.path.clone(), input))
        .collect::<BTreeMap<_, _>>();
    for ResolverInput { path, digest } in resolver.inputs() {
        inputs.insert(
            path.clone(),
            CacheInput {
                identity: digest.as_ref().and_then(|_| cache_input_identity(&path)),
                path,
                digest: digest.map(hex::encode),
            },
        );
    }
    inputs.insert(
        candidate.module_path.clone(),
        CacheInput {
            identity: cache_input_identity(&candidate.module_path),
            path: candidate.module_path.clone(),
            digest: Some(hex::encode(candidate.module_digest)),
        },
    );
    candidate.record.inputs = inputs.into_values().collect();
    candidate.record.fingerprint = verified.fingerprint().to_string();
    candidate.record.signature_expires_at =
        earliest_expiry(candidate.record.signature_expires_at, resolver.signed_module_valid_until());

    let mut resolutions = candidate
        .record
        .resolutions
        .into_iter()
        .map(|item| (item.candidate.clone(), item))
        .collect::<BTreeMap<_, _>>();
    for (candidate_path, canonical) in resolver.canonical_resolutions() {
        resolutions.insert(
            candidate_path.clone(),
            CacheResolution {
                candidate: candidate_path,
                canonical,
            },
        );
    }
    candidate.record.resolutions = resolutions.into_values().collect();

    let mut modules = candidate
        .record
        .modules
        .into_iter()
        .map(|item| (item.path.clone(), item))
        .collect::<BTreeMap<_, _>>();
    for (path, name) in resolver.module_names() {
        modules.insert(path.clone(), CacheModule { path, name });
    }
    candidate.record.modules = modules.into_values().collect();
    candidate.record.module_count = candidate.record.modules.len();
    candidate.record.input_bytes = candidate
        .record
        .inputs
        .iter()
        .filter_map(|input| input.identity.as_ref().map(|identity| identity.size_bytes))
        .fold(0_usize, |total, bytes| total.saturating_add(usize::try_from(bytes).unwrap_or(usize::MAX)));
    store_record(&paths, verified, &candidate.record)
}

fn store_record(paths: &CachePaths, verified: &VerifiedProgram, record: &CacheRecord) -> Result<()> {
    let origins = origins::encode(verified, record)?;
    let metadata = serde_json::to_vec(record)
        .map_err(|error| TinyLangError::compile(format!("Compile cache metadata error: {error}")))?;
    if metadata.len() > MAX_CACHE_METADATA_BYTES {
        return Err(TinyLangError::compile("Compile cache metadata exceeds byte size limit"));
    }
    fs::create_dir_all(&paths.directory)
        .map_err(|error| TinyLangError::compile(format!("Compile cache directory error: {error}")))?;
    write_binary_artifact(verified.program(), &paths.artifact)?;
    fs::write(&paths.origins, origins)
        .map_err(|error| TinyLangError::compile(format!("Compile cache source-origin write error: {error}")))?;
    fs::write(&paths.metadata, metadata)
        .map_err(|error| TinyLangError::compile(format!("Compile cache write error: {error}")))
}

enum InputMatch {
    Same(Vec<u8>),
    Absent,
    Changed { bytes: Vec<u8>, digest: [u8; 16] },
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMetadataMatch {
    Same,
    Changed,
    Unavailable,
}

fn input_metadata_match(input: &CacheInput) -> InputMetadataMatch {
    match (&input.digest, &input.identity) {
        (Some(_), Some(expected)) => match cache_input_identity(&input.path) {
            Some(actual) if actual == *expected => InputMetadataMatch::Same,
            Some(_) => InputMetadataMatch::Changed,
            None => InputMetadataMatch::Unavailable,
        },
        (Some(_), None) => InputMetadataMatch::Unavailable,
        (None, _) if input.path.exists() => InputMetadataMatch::Changed,
        (None, _) => InputMetadataMatch::Same,
    }
}

fn cache_input_identity(path: &Path) -> Option<CacheInputIdentity> {
    let metadata = fs::metadata(path).ok()?;
    let modified_unix_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok());
    Some(CacheInputIdentity {
        size_bytes: metadata.len(),
        modified_unix_nanos,
    })
}

fn match_input(input: &CacheInput) -> InputMatch {
    match &input.digest {
        Some(expected) => {
            let Ok(bytes) = read_bounded(&input.path, MAX_CACHE_INPUT_BYTES) else {
                return InputMatch::Invalid;
            };
            let digest = content_digest(&bytes);
            if digest_matches(expected, digest) {
                InputMatch::Same(bytes)
            } else {
                InputMatch::Changed { bytes, digest }
            }
        }
        None if input.path.exists() => InputMatch::Invalid,
        None => InputMatch::Absent,
    }
}

fn digest_matches(expected: &str, actual: [u8; 16]) -> bool {
    let mut decoded = [0_u8; 16];
    hex::decode_to_slice(expected, &mut decoded).is_ok() && decoded == actual
}

fn resolutions_match(resolutions: &[CacheResolution]) -> bool {
    resolutions.iter().all(|resolution| {
        resolution
            .candidate
            .canonicalize()
            .is_ok_and(|path| path == resolution.canonical)
    })
}

fn read_bounded(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let file =
        fs::File::open(path).map_err(|error| TinyLangError::compile(format!("Compile cache read error: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| TinyLangError::compile(format!("Compile cache read error: {error}")))?;
    if metadata.len() > max_bytes as u64 {
        return Err(TinyLangError::compile(format!("Compile cache input exceeds byte size limit {max_bytes}")));
    }
    // The bound above proves the file size fits usize. Preserve the known
    // size hint that wrapping File in Take would otherwise lose; the read
    // limit and post-read check still reject a file that grows past the cap.
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| TinyLangError::compile(format!("Compile cache read error: {error}")))?;
    if bytes.len() > max_bytes {
        return Err(TinyLangError::compile(format!("Compile cache input exceeds byte size limit {max_bytes}")));
    }
    Ok(bytes)
}

fn load_record(path: &Path) -> Option<CacheRecord> {
    let metadata = read_bounded(path, MAX_CACHE_METADATA_BYTES).ok()?;
    serde_json::from_slice(&metadata).ok()
}

fn record_matches_request(record: &CacheRecord, root: &Path, optimize: bool) -> bool {
    record.format_version == CACHE_FORMAT_VERSION
        && record.compiler_version == env!("CARGO_PKG_VERSION")
        && record.optimize == optimize
        && record.root == root
}

fn earliest_expiry(existing: Option<u64>, replacement: Option<u64>) -> Option<u64> {
    match (existing, replacement) {
        (Some(existing), Some(replacement)) => Some(existing.min(replacement)),
        (Some(existing), None) => Some(existing),
        (None, Some(replacement)) => Some(replacement),
        (None, None) => None,
    }
}

fn signature_cache_entry_is_current(expires_at: Option<u64>) -> Result<bool> {
    let Some(expires_at) = expires_at else {
        return Ok(true);
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| TinyLangError::compile("System clock is before the Unix epoch"))?;
    Ok(now <= expires_at)
}

struct CachePaths {
    directory: PathBuf,
    metadata: PathBuf,
    artifact: PathBuf,
    origins: PathBuf,
}

fn cache_paths(root: &Path, optimize: bool) -> CachePaths {
    let directory = root.parent().unwrap_or_else(|| Path::new(".")).join(".tinylang-cache");
    let mut hasher = Blake2b512::new();
    hasher.update(root.to_string_lossy().as_bytes());
    hasher.update([u8::from(optimize)]);
    hasher.update(CACHE_FORMAT_VERSION.to_le_bytes());
    let digest = hasher.finalize();
    let key = hex::encode(&digest[..12]);
    CachePaths {
        metadata: directory.join(format!("{key}.json")),
        artifact: directory.join(format!("{key}.tob")),
        origins: directory.join(format!("{key}.origins")),
        directory,
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_CACHE_INPUT_BYTES, earliest_expiry, read_bounded, signature_cache_entry_is_current};

    #[test]
    fn bounded_cache_reads_preserve_bytes_and_enforce_file_limits() {
        for length in [0, 1, 31, 32, 33, 4096, 5000, 65_537, MAX_CACHE_INPUT_BYTES] {
            let file = tempfile::NamedTempFile::new().expect("short-lived cache input");
            let expected = vec![0x5a; length];
            std::fs::write(file.path(), &expected).expect("write cache input");
            let read = read_bounded(file.path(), MAX_CACHE_INPUT_BYTES).expect("bounded read");
            assert_eq!(read, expected);
            eprintln!("cache input: length={length} retained_capacity={}", read.capacity());
            assert_eq!(read_bounded(file.path(), length).expect("exact limit"), expected);
            if length > 0 {
                assert!(read_bounded(file.path(), length - 1).is_err());
            }
            file.close().expect("remove short-lived cache input");
        }
    }

    #[test]
    fn signature_expiry_invalidates_cache_entries_after_expiry() {
        assert!(signature_cache_entry_is_current(None).expect("unsigned cache entry"));
        assert!(signature_cache_entry_is_current(Some(u64::MAX)).expect("future signature"));
        assert!(!signature_cache_entry_is_current(Some(0)).expect("expired signature"));
        assert_eq!(earliest_expiry(Some(30), Some(20)), Some(20));
        assert_eq!(earliest_expiry(Some(30), None), Some(30));
    }
}
