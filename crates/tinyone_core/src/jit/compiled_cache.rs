//! Bounded retention of compiled programs, separate from exact-source caching.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

use crate::{JitOptions, JitProgram, Result, VerifiedProgram};

pub const DEFAULT_COMPILED_CACHE_MAX_ENTRIES: usize = 128;
pub const DEFAULT_COMPILED_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;

/// Compiled-cache retention and lifetime activity counters.
///
/// `retained_bytes` charges occupied entry/key headers and each
/// program's inclusive `JitProgram::retained_bytes()`. Shared verified data is
/// charged in full per entry. Unused hash-table buckets, opaque allocator/Arc
/// overhead and profiling-tree node slack are excluded, so this is not RSS.
/// Hash-table capacity is reported separately and released when disabled.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct JitCompiledCacheStats {
    pub programs:        usize,
    pub retained_bytes:  usize,
    /// One non-reusable result held for the borrowed compile-only API. It is
    /// outside the reusable-entry budget and cleared by the next mutation.
    pub transient_bytes: usize,
    pub table_capacity:  usize,
    pub hits:            u64,
    pub misses:          u64,
    pub compilations:    u64,
    pub evictions:       u64,
    pub bypasses:        u64,
}

/// Artifact content is not an authority grant. Options are immutable per
/// cache; input, environment, globals and heap objects are fresh on every run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum ExecutionIdentity {
    Untrusted([u8; 32]),
    Trusted([u8; 32]),
}

impl ExecutionIdentity {
    fn for_program(verified: &VerifiedProgram) -> Self {
        // Program::fingerprint encodes exactly 16 digest bytes as 32 hex bytes.
        // Keep that established identity unchanged, without allocating a key
        // String on every hit or parsing/rehashing the program's contents.
        let fingerprint = verified
            .fingerprint()
            .as_bytes()
            .try_into()
            .expect("program fingerprint is exactly 32 hexadecimal bytes");
        if verified.program().policy_trusted {
            Self::Trusted(fingerprint)
        } else {
            Self::Untrusted(fingerprint)
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum CacheLimits {
    Disabled,
    Bounded {
        entries: NonZeroUsize,
        bytes:   NonZeroUsize,
    },
}

impl CacheLimits {
    fn new(entries: usize, bytes: usize) -> Self {
        match (NonZeroUsize::new(entries), NonZeroUsize::new(bytes)) {
            (Some(entries), Some(bytes)) => Self::Bounded { entries, bytes },
            _ => Self::Disabled,
        }
    }

    fn entries(self) -> usize {
        match self {
            Self::Disabled => 0,
            Self::Bounded { entries, .. } => entries.get(),
        }
    }

    fn bytes(self) -> usize {
        match self {
            Self::Disabled => 0,
            Self::Bounded { bytes, .. } => bytes.get(),
        }
    }

    fn admits(self, bytes: usize) -> bool {
        // A saturated footprint is never proof that an allocation fits.
        bytes != usize::MAX && bytes <= self.bytes() && self.entries() != 0
    }
}

#[derive(Debug)]
struct CachedProgram {
    program:        JitProgram,
    retained_bytes: usize,
    last_used:      u64,
}

impl CachedProgram {
    /// The cache key already proved identical execution content and authority.
    /// Origins are indexed by that bytecode, but source text/filenames belong
    /// to the current request, not the first program to populate this entry.
    fn bind_source_context(&mut self, verified: &VerifiedProgram) {
        if !std::ptr::eq(self.program.verified_program.program(), verified.program()) {
            self.program.verified_program = verified.clone();
        }
    }

    fn charge(program: &JitProgram) -> usize {
        program
            .retained_bytes()
            .saturating_add(size_of::<(ExecutionIdentity, Self)>() - size_of::<JitProgram>())
    }
}

#[derive(Debug)]
pub(super) struct CompiledPrograms {
    entries:        HashMap<ExecutionIdentity, CachedProgram>,
    limits:         CacheLimits,
    retained_bytes: usize,
    access_clock:   u64,
    // The uncommon compile-only bypass must return a borrowed result. Box
    // keeps its large header out of every ordinary cache instance.
    transient:      Option<Box<JitProgram>>,
    hits:           u64,
    misses:         u64,
    compilations:   u64,
    evictions:      u64,
    bypasses:       u64,
}

impl Default for CompiledPrograms {
    fn default() -> Self {
        Self {
            entries:        HashMap::new(),
            limits:         CacheLimits::new(DEFAULT_COMPILED_CACHE_MAX_ENTRIES, DEFAULT_COMPILED_CACHE_MAX_BYTES),
            retained_bytes: 0,
            access_clock:   0,
            transient:      None,
            hits:           0,
            misses:         0,
            compilations:   0,
            evictions:      0,
            bypasses:       0,
        }
    }
}

impl Clone for CompiledPrograms {
    fn clone(&self) -> Self {
        let mut retained_bytes = 0;
        let entries = self
            .entries
            .iter()
            .map(|(key, entry)| {
                let key = *key;
                let program = entry.program.clone();
                let bytes = CachedProgram::charge(&program);
                retained_bytes += bytes;
                (
                    key,
                    CachedProgram {
                        program,
                        retained_bytes: bytes,
                        last_used: entry.last_used,
                    },
                )
            })
            .collect();
        Self {
            entries,
            limits: self.limits,
            retained_bytes,
            access_clock: self.access_clock,
            transient: None,
            hits: self.hits,
            misses: self.misses,
            compilations: self.compilations,
            evictions: self.evictions,
            bypasses: self.bypasses,
        }
    }
}

impl CompiledPrograms {
    pub(super) fn set_limits(&mut self, entries: usize, bytes: usize) {
        self.clear_transient();
        self.limits = CacheLimits::new(entries, bytes);
        self.enforce_limits();
        self.entries.shrink_to(0);
    }

    pub(super) fn max_entries(&self) -> usize {
        self.limits.entries()
    }

    pub(super) fn byte_limit(&self) -> usize {
        self.limits.bytes()
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &JitProgram> {
        self.entries.values().map(|entry| &entry.program)
    }

    pub(super) fn clear_transient(&mut self) {
        self.transient = None;
    }

    pub(super) fn clear(&mut self) {
        self.clear_transient();
        self.evictions = self.evictions.saturating_add(self.entries.len() as u64);
        self.entries = HashMap::new();
        self.retained_bytes = 0;
    }

    pub(super) fn stats(&self) -> JitCompiledCacheStats {
        JitCompiledCacheStats {
            programs:        self.entries.len(),
            retained_bytes:  self.retained_bytes,
            transient_bytes: self.transient.as_deref().map_or(0, JitProgram::retained_bytes),
            table_capacity:  self.entries.capacity(),
            hits:            self.hits,
            misses:          self.misses,
            compilations:    self.compilations,
            evictions:       self.evictions,
            bypasses:        self.bypasses,
        }
    }

    pub(super) fn compile(&mut self, verified: &VerifiedProgram, options: JitOptions) -> Result<&JitProgram> {
        self.clear_transient();
        let key = ExecutionIdentity::for_program(verified);
        let access = self.next_access();
        // Finish context rebinding and retention checks before lending a
        // result. A different source owner can make an existing entry too big.
        if let Some(entry) = self.entries.get_mut(&key) {
            self.hits = self.hits.saturating_add(1);
            entry.last_used = access;
            // Compile-only hits cannot grow code, feedback or scratch. With
            // the same immutable source owner, the stored charge still holds;
            // run(), rebinding and limit changes account their own mutations.
            if !std::ptr::eq(entry.program.verified_program.program(), verified.program()) {
                let previous = entry.retained_bytes;
                entry.program.verified_program = verified.clone();
                let bytes = CachedProgram::charge(&entry.program);
                entry.retained_bytes = bytes;
                let other_bytes = self.retained_bytes - previous;
                if !self.limits.admits(bytes) || other_bytes.checked_add(bytes).is_none() {
                    let entry = self.entries.remove(&key).expect("rebound cache entry exists");
                    self.retained_bytes = other_bytes;
                    self.evictions = self.evictions.saturating_add(1);
                    self.bypasses = self.bypasses.saturating_add(1);
                    self.release_empty_table();
                    return Ok(self.transient.insert(Box::new(entry.program)));
                }
                self.retained_bytes = other_bytes + bytes;
                self.enforce_limits();
            }
            return Ok(&self
                .entries
                .get(&key)
                .expect("most-recent fitting cache entry remains")
                .program);
        }
        self.misses = self.misses.saturating_add(1);
        let compiled = JitProgram::compile_verified_with_options(verified, options)?;
        self.compilations = self.compilations.saturating_add(1);
        let bytes = CachedProgram::charge(&compiled);
        if !self.limits.admits(bytes) {
            self.bypasses = self.bypasses.saturating_add(1);
            return Ok(self.transient.insert(Box::new(compiled)));
        }
        Ok(self.insert(key, compiled, bytes, access))
    }

    pub(super) fn run<T>(
        &mut self,
        verified: &VerifiedProgram,
        options: JitOptions,
        execute: impl FnOnce(&mut JitProgram) -> Result<T>,
    ) -> Result<T> {
        self.clear_transient();
        let key = ExecutionIdentity::for_program(verified);
        let access = self.next_access();
        if let Some(entry) = self.entries.get_mut(&key) {
            self.hits = self.hits.saturating_add(1);
            entry.last_used = access;
            let previous = entry.retained_bytes;
            entry.bind_source_context(verified);
            // Cleanup only, not error conversion: a host Write implementation
            // can unwind after lazy growth. Discard the partly mutated entry
            // before resuming that same panic; no asserted-unwind-safe state
            // is reused after this boundary.
            let result = match catch_unwind(AssertUnwindSafe(|| execute(&mut entry.program))) {
                Ok(result) => result,
                Err(panic) => {
                    self.entries.remove(&key);
                    self.retained_bytes -= previous;
                    self.evictions = self.evictions.saturating_add(1);
                    self.release_empty_table();
                    resume_unwind(panic);
                }
            };
            let bytes = CachedProgram::charge(&entry.program);
            entry.retained_bytes = bytes;
            // Lazy code, feedback and cleared scratch can grow even when the
            // run returns Err. Never skip accounting via an early `?`.
            if previous != bytes {
                self.retained_bytes -= previous;
                if self.limits.admits(bytes)
                    && let Some(total) = self.retained_bytes.checked_add(bytes)
                {
                    self.retained_bytes = total;
                    self.enforce_limits();
                } else {
                    self.entries.remove(&key);
                    self.evictions = self.evictions.saturating_add(1);
                    self.bypasses = self.bypasses.saturating_add(1);
                    self.release_empty_table();
                }
            }
            return result;
        }
        self.misses = self.misses.saturating_add(1);
        let mut compiled = JitProgram::compile_verified_with_options(verified, options)?;
        self.compilations = self.compilations.saturating_add(1);
        let result = execute(&mut compiled);
        let bytes = CachedProgram::charge(&compiled);
        if self.limits.admits(bytes) {
            self.insert(key, compiled, bytes, access);
        } else {
            self.bypasses = self.bypasses.saturating_add(1);
        }
        result
    }

    fn insert(&mut self, key: ExecutionIdentity, program: JitProgram, bytes: usize, last_used: u64) -> &JitProgram {
        // The caller proved a single entry fits. Subtraction avoids wrapping
        // when an embedding chooses usize::MAX as its configured byte limit.
        while self.entries.len() >= self.limits.entries() || self.retained_bytes > self.limits.bytes() - bytes {
            self.evict_lru();
        }
        self.retained_bytes += bytes;
        &self
            .entries
            .entry(key)
            .or_insert(CachedProgram {
                program,
                retained_bytes: bytes,
                last_used,
            })
            .program
    }

    fn enforce_limits(&mut self) {
        while self.entries.len() > self.limits.entries() || self.retained_bytes > self.limits.bytes() {
            self.evict_lru();
        }
        self.release_empty_table();
    }

    fn evict_lru(&mut self) {
        let key = self
            .entries
            .iter()
            .min_by_key(|(key, entry)| (entry.last_used, *key))
            .map(|(key, _)| *key)
            .expect("a nonempty compiled cache exceeds its limits");
        let entry = self.entries.remove(&key).expect("LRU compiled entry exists");
        self.retained_bytes -= entry.retained_bytes;
        self.evictions = self.evictions.saturating_add(1);
    }

    fn release_empty_table(&mut self) {
        if self.entries.is_empty() {
            self.entries = HashMap::new();
        }
    }

    fn next_access(&mut self) -> u64 {
        if self.access_clock == u64::MAX {
            // Extremely rare clock rollover preserves exact LRU ordering.
            let mut ordered: Vec<_> = self.entries.values_mut().collect();
            ordered.sort_unstable_by_key(|entry| entry.last_used);
            for (index, entry) in ordered.iter_mut().enumerate() {
                entry.last_used = index as u64 + 1;
            }
            self.access_clock = ordered.len() as u64;
        }
        self.access_clock += 1;
        self.access_clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compile_source_verified, compile_source_verified_with_filename, run_verified_program};

    #[test]
    fn fixed_identity_preserves_every_fingerprint_byte_and_separates_authority() {
        let trusted = compile_source_verified("print 42").expect("fixture");
        let key = ExecutionIdentity::for_program(&trusted);
        let ExecutionIdentity::Trusted(bytes) = key else {
            panic!("compiler result is trusted");
        };
        assert_eq!(bytes.as_slice(), trusted.fingerprint().as_bytes());
        assert!(!std::mem::needs_drop::<ExecutionIdentity>());
        let mut untrusted = trusted.program().clone();
        untrusted.policy_trusted = false;
        let untrusted = VerifiedProgram::verify(untrusted).expect("untrusted fixture");
        assert_eq!(trusted.fingerprint(), untrusted.fingerprint());
        assert_ne!(key, ExecutionIdentity::for_program(&untrusted));
    }

    fn assert_accounted(cache: &CompiledPrograms) {
        let mut total = 0;
        for entry in cache.entries.values() {
            let actual = CachedProgram::charge(&entry.program);
            assert_eq!(entry.retained_bytes, actual);
            total += actual;
        }
        assert_eq!(cache.stats().retained_bytes, total);
        assert!(total <= cache.byte_limit());
        assert!(cache.len() <= cache.max_entries());
    }

    #[test]
    fn compile_hits_preserve_warmed_storage_and_refresh_lru_at_the_exact_budget() {
        let first =
            compile_source_verified("fn count() { let i = 0 while i < 32 { i = i + 1 } return i } print count()")
                .expect("lazy function fixture");
        let alias = first.clone();
        let second = compile_source_verified("print 2").expect("second entry");
        let third = compile_source_verified("print 3").expect("replacement entry");
        let options = JitOptions::new()
            .with_hot_back_edge_threshold(1)
            .with_execution_profile(true);
        let mut cache = CompiledPrograms::default();
        cache
            .run(&first, options, |compiled| compiled.run(&mut Vec::new(), Vec::new()))
            .expect("warm code, feedback, profile and scratch");
        cache.compile(&second, options).expect("second entry");
        let bytes = cache.stats().retained_bytes;
        cache.set_limits(2, bytes);
        let key = ExecutionIdentity::for_program(&first);
        let warmed = &cache.entries[&key].program;
        let pointer = std::ptr::from_ref(warmed);
        let warmed_bytes = warmed.retained_bytes();
        let warmed_stats = warmed.stats();
        assert_eq!(warmed_stats.compiled_chunks, 2);
        assert!(warmed_stats.quickened_ops > 0);
        assert!(warmed_stats.cached_operand_stack_capacity > 0);

        for verified in [&first, &alias, &first, &alias] {
            let compiled = cache.compile(verified, options).expect("unchanged compile-only hit");
            assert!(std::ptr::eq(compiled, pointer));
            assert!(std::ptr::eq(compiled.verified_program.program(), verified.program()));
            assert_eq!(compiled.retained_bytes(), warmed_bytes);
            assert_eq!(compiled.stats().quickened_ops, warmed_stats.quickened_ops);
            assert_eq!(cache.stats().retained_bytes, bytes);
            assert_eq!(cache.stats().compilations, 2);
            assert_eq!(cache.stats().evictions, 0);
            assert_accounted(&cache);
        }
        assert_eq!(cache.stats().hits, 4);
        cache.compile(&third, options).expect("evict the older second entry");
        assert!(cache.entries.contains_key(&key));
        assert!(!cache.entries.contains_key(&ExecutionIdentity::for_program(&second)));
        assert!(cache.entries.contains_key(&ExecutionIdentity::for_program(&third)));
        assert_eq!(cache.stats().evictions, 1);
        assert_accounted(&cache);
    }

    #[test]
    fn compile_rebinding_accounts_growth_eviction_and_later_shrinkage() {
        let small = compile_source_verified_with_filename("print 42", "small.to").expect("small source");
        let large = compile_source_verified_with_filename(&format!("{}print 42", "\n".repeat(8192)), "large.to")
            .expect("large equivalent source");
        let other = compile_source_verified("print 7").expect("other entry");
        let options = JitOptions::default();
        let large_bytes = CachedProgram::charge(
            &JitProgram::compile_verified_with_options(&large, options).expect("large charge fixture"),
        );
        let mut cache = CompiledPrograms::default();
        cache.set_limits(2, large_bytes);
        cache.compile(&small, options).expect("small entry");
        cache.compile(&other, options).expect("other entry");
        assert_eq!(cache.len(), 2);
        cache
            .compile(&large, options)
            .expect("larger source fits after LRU eviction");
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.stats().retained_bytes, large_bytes);
        assert_eq!(cache.stats().evictions, 1);
        assert_eq!(cache.stats().bypasses, 0);
        assert_eq!(cache.stats().compilations, 2);
        assert_accounted(&cache);

        cache.compile(&small, options).expect("source owner shrinks");
        assert!(cache.stats().retained_bytes < large_bytes);
        cache
            .compile(&other, options)
            .expect("released bytes admit other entry again");
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.stats().compilations, 3);
        assert_eq!(cache.stats().hits, 2);
        assert_eq!(cache.stats().evictions, 1);
        assert_accounted(&cache);
    }

    #[test]
    fn identical_bytecode_hits_use_current_source_without_losing_lazy_code_or_quickening() {
        let source = "fn divide(value) {\n let total = 7 let i = 0\n while i < 32 { total = total * 1 i = i + 1 }\n return total / value\n}\nprint divide(0)\n";
        let first = compile_source_verified_with_filename(source, "first.to").expect("first source");
        let second =
            compile_source_verified_with_filename(&format!("\n\n{source}"), "second.to").expect("second source");
        let mut source_less = first.program().clone();
        source_less.source = None;
        for function in &mut source_less.functions {
            function.source = None;
        }
        let source_less = VerifiedProgram::verify(source_less).expect("source-less equivalent");
        assert_eq!(ExecutionIdentity::for_program(&first), ExecutionIdentity::for_program(&second));
        assert_eq!(ExecutionIdentity::for_program(&first), ExecutionIdentity::for_program(&source_less));
        let options = JitOptions::new().with_hot_back_edge_threshold(1);
        let mut cache = CompiledPrograms::default();
        for verified in [&first, &second, &source_less, &first] {
            let expected = run_verified_program(verified, "vm", &mut Vec::new(), Vec::new())
                .expect_err("VM division failure")
                .to_string();
            let actual = cache
                .run(verified, options, |compiled| compiled.run(&mut Vec::new(), Vec::new()))
                .expect_err("JIT division failure")
                .to_string();
            assert_eq!(actual, expected);
            assert_accounted(&cache);
            let compiled = cache.values().next().expect("retained entry");
            assert_eq!(compiled.stats().compiled_chunks, 2);
            assert!(compiled.stats().quickened_ops > 0);
        }
        assert_eq!(cache.stats().compilations, 1);
        assert_eq!(cache.stats().hits, 3);

        let key = ExecutionIdentity::for_program(&first);
        let origins = std::sync::Arc::clone(&cache.entries[&key].program.chunks[1].as_ref().expect("callee").origins);
        for verified in [&second, &source_less, &first] {
            let mut compiled = cache.compile(verified, options).expect("compile-only hit").clone();
            assert!(std::ptr::eq(compiled.verified_program.program(), verified.program()));
            assert!(std::sync::Arc::ptr_eq(&compiled.chunks[1].as_ref().expect("callee").origins, &origins));
            assert!(compiled.stats().quickened_ops > 0);
            let expected = run_verified_program(verified, "vm", &mut Vec::new(), Vec::new())
                .expect_err("VM division failure")
                .to_string();
            assert_eq!(
                compiled
                    .run(&mut Vec::new(), Vec::new())
                    .expect_err("JIT division failure")
                    .to_string(),
                expected
            );
            assert_accounted(&cache);
        }
        assert_eq!(cache.stats().compilations, 1);
        assert_eq!(cache.stats().hits, 6);
    }

    #[test]
    fn larger_source_owner_is_recharged_on_run_and_compile_hits() {
        let first = compile_source_verified_with_filename("print 42", "small.to").expect("small source");
        let second = compile_source_verified_with_filename(&format!("{}print 42", "\n".repeat(8192)), "large.to")
            .expect("large source");
        assert_eq!(ExecutionIdentity::for_program(&first), ExecutionIdentity::for_program(&second));
        let options = JitOptions::default();
        let mut cache = CompiledPrograms::default();
        cache.compile(&first, options).expect("first entry");
        let limit = cache.stats().retained_bytes;
        cache.set_limits(2, limit);
        let compiled = cache.compile(&second, options).expect("oversized compile-only hit");
        assert!(std::ptr::eq(compiled.verified_program.program(), second.program()));
        assert!(cache.stats().transient_bytes > limit);
        assert!(cache.is_empty());
        assert_eq!(cache.stats().table_capacity, 0);
        assert_eq!(cache.stats().compilations, 1);
        assert_eq!(cache.stats().bypasses, 1);
        assert_accounted(&cache);

        cache.compile(&first, options).expect("small entry recompiled");
        assert_eq!(cache.stats().transient_bytes, 0);
        let mut output = Vec::new();
        cache
            .run(&second, options, |compiled| compiled.run(&mut output, Vec::new()))
            .expect("oversized run hit");
        assert_eq!(output, b"42\n");
        assert!(cache.is_empty());
        assert_eq!(cache.stats().table_capacity, 0);
        assert_eq!(cache.stats().compilations, 2);
        assert_eq!(cache.stats().hits, 2);
        assert_eq!(cache.stats().bypasses, 2);
        assert_accounted(&cache);
    }

    #[test]
    fn rebinding_releases_the_previous_verified_owner() {
        let first = compile_source_verified_with_filename("print 42", "first.to").expect("first source");
        let second = compile_source_verified_with_filename("print 42", "second.to").expect("second source");
        let weak = std::sync::Arc::downgrade(&first.program_arc());
        let mut cache = CompiledPrograms::default();
        cache.compile(&first, JitOptions::default()).expect("first entry");
        drop(first);
        assert!(weak.upgrade().is_some());
        cache
            .compile(&second, JitOptions::default())
            .expect("source owner rebound");
        assert!(weak.upgrade().is_none());
        assert_eq!(cache.stats().compilations, 1);
        assert_accounted(&cache);
    }

    #[test]
    fn rollover_preserves_lru_order_and_clone_recounts_actual_storage() {
        let first = compile_source_verified("let i = 0 while i < 32 { i = i + 1 } print i").expect("loop");
        let second = compile_source_verified("print 2").expect("second");
        let third = compile_source_verified("print 3").expect("third");
        let mut cache = CompiledPrograms::default();
        cache.set_limits(2, usize::MAX);
        let options = JitOptions::new().with_execution_profile(true);
        cache
            .run(&first, options, |compiled| compiled.run(&mut Vec::new(), Vec::new()))
            .expect("warm first");
        cache.compile(&second, options).expect("cold second");
        assert_accounted(&cache);
        let original_bytes = cache.retained_bytes;
        let mut cloned = cache.clone();
        assert_accounted(&cloned);
        assert!(cloned.retained_bytes < original_bytes, "scratch and spare cloned code are not retained");
        assert!(
            cloned
                .values()
                .all(|program| program.stats().cached_operand_stacks == 0)
        );
        assert_eq!(cache.retained_bytes, original_bytes);
        cloned.access_clock = u64::MAX;
        cloned.compile(&first, options).expect("refresh first after rollover");
        cloned.compile(&third, options).expect("evict second");
        assert!(cloned.entries.contains_key(&ExecutionIdentity::for_program(&first)));
        assert!(!cloned.entries.contains_key(&ExecutionIdentity::for_program(&second)));
        assert!(cloned.entries.contains_key(&ExecutionIdentity::for_program(&third)));
        assert_accounted(&cloned);
        assert_accounted(&cache);
    }

    #[test]
    fn eviction_and_bypass_release_their_verified_program_owners() {
        let mut cache = CompiledPrograms::default();
        cache.set_limits(1, usize::MAX);
        let program = compile_source_verified("print 42").expect("fixture");
        let weak = std::sync::Arc::downgrade(&program.program_arc());
        cache.compile(&program, JitOptions::default()).expect("cached fixture");
        drop(program);
        assert!(weak.upgrade().is_some());
        let next = compile_source_verified("print 9").expect("next");
        cache.compile(&next, JitOptions::default()).expect("replacement");
        assert!(weak.upgrade().is_none());
        cache.set_limits(0, 0);
        let weak = std::sync::Arc::downgrade(&next.program_arc());
        cache.compile(&next, JitOptions::default()).expect("temporary result");
        drop(next);
        assert!(weak.upgrade().is_some());
        cache.clear_transient();
        assert!(weak.upgrade().is_none());
        assert_eq!(cache.stats().table_capacity, 0);
    }

    #[test]
    fn storage_saturation_is_not_admitted_even_with_maximum_limits() {
        let limits = CacheLimits::new(usize::MAX, usize::MAX);
        assert!(!limits.admits(usize::MAX));
        assert!(limits.admits(usize::MAX - 1));
        assert!(!CacheLimits::new(0, usize::MAX).admits(1));
        assert!(!CacheLimits::new(usize::MAX, 0).admits(1));
    }

    #[test]
    #[ignore = "compiled-cache retention diagnostic; run explicitly with --ignored --nocapture"]
    fn report_compiled_cache_retention() {
        use crate::{Instr, Op, Program};

        let mut cache = CompiledPrograms::default();
        for value in 0..257 {
            let program = VerifiedProgram::verify(Program::new(
                vec![
                    Instr::new(Op::PushInt, value, 0),
                    Instr::new(Op::Print, 0, 0),
                    Instr::new(Op::Halt, 0, 0),
                ],
                0,
            ))
            .expect("retention fixture");
            cache
                .run(&program, JitOptions::default(), |compiled| compiled.run(&mut Vec::new(), Vec::new()))
                .expect("retention execution");
            assert_accounted(&cache);
            if [0, 127, 128, 256].contains(&value) {
                eprintln!("compiled-cache after {} unique programs: {:?}", value + 1, cache.stats());
            }
        }
        cache.clear();
        assert_accounted(&cache);
        assert_eq!(cache.stats().table_capacity, 0);
    }
}
