//! Private disk-cache origins, separate from the frozen portable artifacts.
//! Checksums detect stale/torn cache combinations; they do not grant authority.

use std::sync::Arc;

use blake2::{Blake2b512, Digest};

use super::CacheRecord;
use crate::source::SourceMap;
use crate::source::bytecode::{BytecodeSource, SourceOffset};
use crate::{Result, TinyLangError, VerifiedProgram, content_digest};

const MAGIC: &[u8; 4] = b"TOS1";
const DIGEST_BYTES: usize = 16;
const HEADER_BYTES: usize = MAGIC.len() + DIGEST_BYTES;

/// Bytes have already passed the dependency digest check. Conversion moves
/// their allocation once; all chunks from that input share the resulting map.
pub(super) enum SourceInput {
    Unavailable,
    Bytes(Vec<u8>),
    Ready(Arc<SourceMap>),
}

impl SourceInput {
    fn source(&mut self, filename: &std::path::Path) -> Option<Arc<SourceMap>> {
        if let Self::Ready(source) = self {
            return Some(Arc::clone(source));
        }
        let Self::Bytes(bytes) = std::mem::replace(self, Self::Unavailable) else {
            return None;
        };
        let text = String::from_utf8(bytes).ok()?;
        let source = Arc::new(SourceMap::new(text, filename.to_string_lossy().into_owned()));
        *self = Self::Ready(Arc::clone(&source));
        Some(source)
    }
}

#[derive(Clone, Copy)]
struct SourceInputId(u32);

impl SourceInputId {
    fn parse(index: usize, record: &CacheRecord) -> Option<Self> {
        let input = record.inputs.get(index)?;
        if input.digest.is_none()
            || !(input.path == record.root || record.modules.iter().any(|module| module.path == input.path))
        {
            return None;
        }
        Some(Self(u32::try_from(index).ok()?))
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Counts come from verified bytecode, never from an untrusted sidecar header.
pub(super) fn encoded_length(verified: &VerifiedProgram) -> usize {
    let program = verified.program();
    let instructions = program.code.len()
        + program
            .functions
            .iter()
            .map(|function| function.code.len())
            .sum::<usize>();
    HEADER_BYTES + DIGEST_BYTES + 4 * (program.functions.len() + 1 + instructions)
}

fn context_digest(record: &CacheRecord) -> [u8; DIGEST_BYTES] {
    let mut hasher = Blake2b512::new();
    hasher.update(b"tinylang-cache-origins-v1");
    hasher.update(record.fingerprint.as_bytes());
    hasher.update([u8::from(record.optimize)]);
    hasher.update((record.inputs.len() as u64).to_le_bytes());
    for input in &record.inputs {
        let path = input.path.to_string_lossy();
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        match &input.digest {
            Some(digest) => {
                hasher.update([1]);
                hasher.update((digest.len() as u64).to_le_bytes());
                hasher.update(digest.as_bytes());
            }
            None => hasher.update([0]),
        }
    }
    let digest = hasher.finalize();
    let mut context = [0; DIGEST_BYTES];
    context.copy_from_slice(&digest[..DIGEST_BYTES]);
    context
}

pub(super) fn encode(verified: &VerifiedProgram, record: &CacheRecord) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(encoded_length(verified));
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&context_digest(record));
    let program = verified.program();
    for (source, length) in std::iter::once((&program.source, program.code.len())).chain(
        program
            .functions
            .iter()
            .map(|function| (&function.source, function.code.len())),
    ) {
        let source = source
            .as_ref()
            .ok_or_else(|| TinyLangError::compile("Cache source origins missing"))?;
        if source.instruction_count() != length {
            return Err(TinyLangError::compile("Cache source origin extent mismatch"));
        }
        let input = record
            .inputs
            .iter()
            .position(|input| input.path.to_string_lossy() == source.source_owner().filename())
            .and_then(|index| SourceInputId::parse(index, record))
            .ok_or_else(|| TinyLangError::compile("Cache source owner is not a recorded source input"))?;
        bytes.extend_from_slice(&input.0.to_le_bytes());
        for &SourceOffset(offset) in source.offsets() {
            if offset > source.source_owner().source().len() {
                return Err(TinyLangError::compile("Cache source offset exceeds source length"));
            }
            let offset =
                u32::try_from(offset).map_err(|_| TinyLangError::compile("Cache source offset exceeds u32"))?;
            bytes.extend_from_slice(&offset.to_le_bytes());
        }
    }
    let checksum = content_digest(&bytes);
    bytes.extend_from_slice(&checksum);
    Ok(bytes)
}

/// A failed cache decode is a miss, not an execution error. No partial tables
/// are installed. Only the module about to be replaced may lack old sources.
pub(super) fn restore(
    verified: VerifiedProgram,
    bytes: &[u8],
    record: &CacheRecord,
    mut inputs: Vec<SourceInput>,
    replacement_module: Option<&str>,
) -> Option<VerifiedProgram> {
    if bytes.len() != encoded_length(&verified) || inputs.len() != record.inputs.len() {
        return None;
    }
    let checksum_start = bytes.len().checked_sub(DIGEST_BYTES)?;
    if bytes.get(..MAGIC.len())? != MAGIC
        || bytes.get(MAGIC.len()..HEADER_BYTES)? != context_digest(record)
        || bytes.get(checksum_start..)? != content_digest(&bytes[..checksum_start])
    {
        return None;
    }
    let mut remaining = &bytes[HEADER_BYTES..checksum_start];
    let program = verified.program();
    let mut sources = Vec::with_capacity(program.functions.len() + 1);
    for (name, length) in std::iter::once((None, program.code.len())).chain(
        program
            .functions
            .iter()
            .map(|function| (Some(function.name.as_str()), function.code.len())),
    ) {
        let input = SourceInputId::parse(read_u32(&mut remaining)? as usize, record)?;
        let path = &record.inputs[input.index()].path;
        let encoded = remaining.get(..length.checked_mul(4)?)?;
        remaining = &remaining[encoded.len()..];
        if matches!(inputs[input.index()], SourceInput::Unavailable) {
            let module = replacement_module?;
            if !name.is_some_and(|name| name.split_once('.').is_some_and(|(owner, _)| owner == module))
                || !record
                    .modules
                    .iter()
                    .any(|entry| entry.name == module && entry.path == *path)
            {
                return None;
            }
            sources.push(None);
            continue;
        }
        let source = inputs[input.index()].source(path)?;
        let mut offsets = Vec::with_capacity(length);
        for &encoded in encoded.as_chunks::<4>().0 {
            let offset = u32::from_le_bytes(encoded) as usize;
            if offset > source.source().len() {
                return None;
            }
            offsets.push(SourceOffset(offset));
        }
        sources.push(Some(BytecodeSource::new(source, offsets)));
    }
    if !remaining.is_empty() {
        return None;
    }
    verified.with_source_origins(sources).ok()
}

fn read_u32(bytes: &mut &[u8]) -> Option<u32> {
    let value = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?);
    *bytes = &bytes[4..];
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::compile_cache::{CACHE_FORMAT_VERSION, CacheInput};
    use crate::{Program, compile_source_verified_with_filename};

    fn fixture(text: &str) -> (VerifiedProgram, CacheRecord) {
        let root = std::path::PathBuf::from("cache-main.to");
        let verified = compile_source_verified_with_filename(text, &root.to_string_lossy()).expect("fixture");
        let record = CacheRecord {
            format_version: CACHE_FORMAT_VERSION,
            compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
            optimize: true,
            root: root.clone(),
            fingerprint: verified.fingerprint().to_owned(),
            module_count: 0,
            input_bytes: text.len(),
            signature_expires_at: None,
            inputs: vec![CacheInput {
                path: root,
                digest: Some(hex::encode(content_digest(text.as_bytes()))),
                identity: None,
            }],
            resolutions: Vec::new(),
            modules: Vec::new(),
        };
        (verified, record)
    }

    fn decode(verified: &VerifiedProgram) -> VerifiedProgram {
        let bytes = verified.program().to_binary_artifact().expect("artifact");
        VerifiedProgram::verify(Program::from_binary_artifact(&bytes).expect("decode")).expect("verify")
    }

    fn source_inputs(text: &str) -> Vec<SourceInput> {
        vec![SourceInput::Bytes(text.as_bytes().to_vec())]
    }

    fn refresh_checksum(bytes: &mut [u8]) {
        let start = bytes.len() - DIGEST_BYTES;
        let checksum = content_digest(&bytes[..start]);
        bytes[start..].copy_from_slice(&checksum);
    }

    #[test]
    fn roundtrip_reuses_validated_text_and_shares_one_source_owner() {
        let text = "fn first(value) { return value / 0 } fn second() { return 2 } print first(second())";
        let (verified, record) = fixture(text);
        let encoded = encode(&verified, &record).expect("encode origins");
        assert_eq!(encoded.len(), encoded_length(&verified));
        let input = text.as_bytes().to_vec();
        let text_pointer = input.as_ptr();
        let restored = restore(decode(&verified), &encoded, &record, vec![SourceInput::Bytes(input)], None)
            .expect("restore source origins");
        assert_eq!(restored.fingerprint(), verified.fingerprint());
        assert!(!restored.program().policy_trusted);
        let main = restored.program().source.as_ref().expect("main source");
        assert_eq!(main.source_owner().source().as_ptr(), text_pointer, "move, not copy, validated text");
        assert_eq!(main.source_owner().source(), text);
        assert_eq!(main.offsets(), verified.program().source.as_ref().expect("original main").offsets());
        for (original, restored) in verified.program().functions.iter().zip(&restored.program().functions) {
            let source = restored.source.as_ref().expect("restored function");
            assert_eq!(source.offsets(), original.source.as_ref().expect("original function").offsets());
            assert!(Arc::ptr_eq(main.source_owner(), source.source_owner()));
        }
        eprintln!(
            "cache origins: chunks={} sidecar_bytes={} restored_source_bytes={} verified_retained_bytes={}",
            restored.program().functions.len() + 1,
            encoded.len(),
            restored.program().source_allocation_bytes(),
            restored.retained_bytes(),
        );
    }

    #[test]
    fn truncated_extended_and_corrupt_sidecars_are_misses() {
        let text = "print 1 / 0";
        let (verified, record) = fixture(text);
        let encoded = encode(&verified, &record).expect("encode origins");
        for end in 0..encoded.len() {
            assert!(restore(decode(&verified), &encoded[..end], &record, source_inputs(text), None).is_none());
        }
        let mut extended = encoded.clone();
        extended.push(0);
        assert!(restore(decode(&verified), &extended, &record, source_inputs(text), None).is_none());
        for index in [0, MAGIC.len(), HEADER_BYTES, encoded.len() - 1] {
            let mut corrupted = encoded.clone();
            corrupted[index] ^= 1;
            assert!(restore(decode(&verified), &corrupted, &record, source_inputs(text), None).is_none());
        }
    }

    #[test]
    fn valid_checksums_do_not_bypass_origin_bounds_or_source_ownership() {
        let text = "print 1 / 0";
        let (verified, mut record) = fixture(text);
        let encoded = encode(&verified, &record).expect("encode origins");
        for start in [HEADER_BYTES, HEADER_BYTES + 4] {
            let mut corrupted = encoded.clone();
            corrupted[start..start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            refresh_checksum(&mut corrupted);
            assert!(restore(decode(&verified), &corrupted, &record, source_inputs(text), None).is_none());
        }

        record.inputs.push(CacheInput {
            path: "tinylang.json".into(),
            digest: Some(hex::encode(content_digest(b"{}"))),
            identity: None,
        });
        let mut encoded = encode(&verified, &record).expect("encode with a manifest input");
        encoded[HEADER_BYTES..HEADER_BYTES + 4].copy_from_slice(&1_u32.to_le_bytes());
        refresh_checksum(&mut encoded);
        let inputs = vec![
            SourceInput::Bytes(text.as_bytes().to_vec()),
            SourceInput::Bytes(b"{}".to_vec()),
        ];
        assert!(restore(decode(&verified), &encoded, &record, inputs, None).is_none());
    }

    #[test]
    fn identical_bytecode_cannot_reuse_origins_from_a_different_source_snapshot() {
        let (first, first_record) = fixture("print 1 / 0");
        let text = "\n\nprint 1 / 0";
        let (second, second_record) = fixture(text);
        assert_eq!(first.fingerprint(), second.fingerprint());
        let old = encode(&first, &first_record).expect("old origins");
        assert!(restore(decode(&second), &old, &second_record, source_inputs(text), None).is_none());
        let current = encode(&second, &second_record).expect("new origins");
        assert!(restore(decode(&second), &current, &second_record, source_inputs(text), None).is_some());
    }

    #[test]
    fn missing_or_invalid_source_inputs_cannot_publish_partial_origins() {
        let text = "fn fail() { return 1 / 0 } print fail()";
        let (verified, record) = fixture(text);
        let encoded = encode(&verified, &record).expect("origins");
        for inputs in [
            Vec::new(),
            vec![SourceInput::Unavailable],
            vec![SourceInput::Bytes(vec![0xff])],
        ] {
            assert!(restore(decode(&verified), &encoded, &record, inputs, Some("fail")).is_none());
        }
    }

    #[test]
    fn source_encoding_rejects_missing_origins_and_unrecorded_owners() {
        let (verified, mut record) = fixture("print 1 / 0");
        assert!(encode(&decode(&verified), &record).is_err());
        record.inputs[0].path = "different.to".into();
        assert!(encode(&verified, &record).is_err());
    }
}
