#![cfg(feature = "testing-hooks")]

#[path = "allocation/host.rs"]
mod host;

use host::HostAllocationInterval;
use ralloc::testing::instrumentation_snapshot;
use tinylang::testing::{reset_runtime_cost_counters, runtime_cost_counters};
use tinylang::{JitOptions, JitProgram, compile_source_verified, run_verified_program};

#[derive(Clone, Copy, Debug)]
enum KeyKind {
    Integer,
    String,
    Heap,
    Pointer,
}

impl KeyKind {
    fn source(self, count: usize) -> (String, &'static str) {
        match self {
            Self::Integer => (String::new(), "i"),
            Self::String => {
                let literals = (0..count)
                    .map(|index| format!("\"key_{index}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                (format!("let source_keys = [{literals}]"), "source_keys[i]")
            }
            Self::Heap => (String::new(), "alloc(i)"),
            Self::Pointer => (String::new(), "ptr(alloc(i))"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Removal {
    Front,
    Back,
    Reinsert,
}

impl Removal {
    fn source(self, key_kind: KeyKind, count: usize) -> String {
        let (setup, key) = key_kind.source(count);
        let index = match self {
            Self::Front | Self::Reinsert => "j".to_owned(),
            Self::Back => format!("{count} - j - 1"),
        };
        let reinsert = match self {
            Self::Front | Self::Back => String::new(),
            Self::Reinsert => {
                format!(
                    "let k = 0 while k < {count} {{ \
                 let restored = map_set(values, keys[k], k) \
                 total = total + map_get(values, keys[k]) k = k + 1 }}"
                )
            }
        };
        format!(
            "{setup} let values = map_new() let keys = vec_new() \
             let i = 0 while i < {count} {{ let key = {key} \
             let pushed = push(keys, key) let inserted = map_set(values, key, i) i = i + 1 }} \
             let removed = 0 let missing = 0 let j = 0 while j < {count} {{ \
             removed = removed + map_del(values, keys[{index}]) \
             missing = missing + map_del(values, keys[{index}]) j = j + 1 }} \
             let total = 0 {reinsert} print removed print missing print map_len(values) print total"
        )
    }

    fn expected_output(self, count: usize) -> String {
        let (len, total) = match self {
            Self::Front | Self::Back => (0, 0),
            Self::Reinsert => (count, count * count.saturating_sub(1) / 2),
        };
        format!("{count}\n0\n{len}\n{total}\n")
    }
}

/// Run alone: host and Ralloc counters aggregate the entire process. The
/// optimizer may eliminate allocations; counts describe this build, not a
/// language guarantee. Compilation, output reservation and reporting are
/// outside each interval; heap construction, mutation and cleanup are inside.
#[test]
#[ignore = "map-removal allocation attribution; run alone with --ignored --nocapture --test-threads=1"]
fn report_map_removal_allocation_profiles() {
    for key_kind in [KeyKind::Integer, KeyKind::String, KeyKind::Heap, KeyKind::Pointer] {
        for removal in [Removal::Front, Removal::Back, Removal::Reinsert] {
            for count in [0, 1, 256] {
                let verified =
                    compile_source_verified(&removal.source(key_kind, count)).expect("map fixture compilation");
                let expected = removal.expected_output(count);
                let mut reference = Vec::with_capacity(expected.len());
                drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM reference"));
                assert_eq!(reference, expected.as_bytes(), "{key_kind:?}/{removal:?}/{count}");
                for threshold in [0, 8] {
                    let mut compiled = JitProgram::compile_verified_with_options(
                        &verified,
                        JitOptions::new().with_hot_back_edge_threshold(threshold),
                    )
                    .expect("map fixture lowering");
                    for phase in ["cold", "warm"] {
                        let mut output = Vec::with_capacity(reference.len());
                        reset_runtime_cost_counters();
                        let interval = HostAllocationInterval::start();
                        drop(compiled.run_report(&mut output, Vec::new()).expect("map execution"));
                        let host = interval.finish();
                        let runtime = runtime_cost_counters();
                        let ralloc = instrumentation_snapshot();
                        assert_eq!(output, reference, "{key_kind:?}/{removal:?}/{count}/{threshold}/{phase}");
                        assert_eq!(ralloc.allocations, ralloc.deallocations, "Ralloc cleanup");
                        assert_eq!(ralloc.live_payload_bytes, 0, "fresh heap after execution");
                        assert_eq!(runtime.ralloc_growth_events, ralloc.growth_events);
                        assert_eq!(runtime.ralloc_bytes_copied, ralloc.bytes_copied);
                        eprintln!(
                            "{key_kind:?}/{removal:?}/{count}/{threshold}/{phase}: \
                             host_allocations={} host_deallocations={} host_reallocations={} \
                             host_bytes={} host_reallocated_bytes={} buckets_64_256_1024_large={:?}",
                            host.allocations,
                            host.deallocations,
                            host.reallocations,
                            host.allocated_bytes,
                            host.reallocated_bytes,
                            host.size_buckets,
                        );
                        eprintln!("  runtime={runtime:?}");
                        eprintln!("  ralloc={ralloc:?}");
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum QueryKey {
    Integer,
    String { bytes: usize },
    FieldPointer,
}

impl QueryKey {
    fn source(self) -> String {
        match self {
            Self::Integer => "let key = 7 let absent = 8".to_owned(),
            Self::String { bytes } => {
                let text = "k".repeat(bytes);
                let missing = "m".repeat(bytes);
                format!("let key = \"{text}\" let absent = \"{missing}\"")
            }
            Self::FieldPointer => "struct Pair { left, right } let pair = Pair(7, 8) \
                 let key = fieldptr(pair, \"right\") let absent = fieldptr(pair, \"left\")"
                .to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Query {
    Get,
    Has,
    Missing,
    DeleteMissing,
    Update,
}

impl Query {
    fn source(self, key: QueryKey, count: usize) -> String {
        let operation = match self {
            Self::Get => "map_get(values, key)",
            Self::Has => "map_has(values, key)",
            Self::Missing => "map_has(values, absent)",
            Self::DeleteMissing => "map_del(values, absent)",
            Self::Update => "map_set(values, key, 7)",
        };
        format!(
            "{} let values = map_new() let inserted = map_set(values, key, 7) \
             let i = 0 let total = 0 while i < {count} {{ \
             total = total + {operation} i = i + 1 }} print total print map_len(values)",
            key.source(),
        )
    }

    fn expected_output(self, count: usize) -> String {
        let result = match self {
            Self::Get | Self::Update => 7,
            Self::Has => 1,
            Self::Missing | Self::DeleteMissing => 0,
        };
        format!("{}\n1\n", count * result)
    }
}

/// Run alone, like the removal probe above. The fixed one-entry map separates
/// query-key copies from index growth. Short/long strings expose copied bytes;
/// field pointers also include the runtime's other pointer-metadata copies.
#[test]
#[ignore = "map-query allocation attribution; run alone with --ignored --nocapture --test-threads=1"]
fn report_map_query_allocation_profiles() {
    for key in [
        QueryKey::Integer,
        QueryKey::String { bytes: 8 },
        QueryKey::String { bytes: 256 },
        QueryKey::FieldPointer,
    ] {
        for query in [
            Query::Get,
            Query::Has,
            Query::Missing,
            Query::DeleteMissing,
            Query::Update,
        ] {
            for count in [0, 1, 4096] {
                let verified = compile_source_verified(&query.source(key, count)).expect("map query compilation");
                let expected = query.expected_output(count);
                let mut reference = Vec::with_capacity(expected.len());
                drop(run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM reference"));
                assert_eq!(reference, expected.as_bytes(), "{key:?}/{query:?}/{count}");
                for threshold in [0, 8] {
                    let mut compiled = JitProgram::compile_verified_with_options(
                        &verified,
                        JitOptions::new().with_hot_back_edge_threshold(threshold),
                    )
                    .expect("map query lowering");
                    for phase in ["cold", "warm"] {
                        let mut output = Vec::with_capacity(reference.len());
                        reset_runtime_cost_counters();
                        let interval = HostAllocationInterval::start();
                        drop(
                            compiled
                                .run_report(&mut output, Vec::new())
                                .expect("map query execution"),
                        );
                        let host = interval.finish();
                        let runtime = runtime_cost_counters();
                        let ralloc = instrumentation_snapshot();
                        assert_eq!(output, reference, "{key:?}/{query:?}/{count}/{threshold}/{phase}");
                        assert_eq!(ralloc.allocations, ralloc.deallocations, "Ralloc cleanup");
                        assert_eq!(ralloc.live_payload_bytes, 0, "fresh heap after execution");
                        assert_eq!(runtime.ralloc_growth_events, ralloc.growth_events);
                        assert_eq!(runtime.ralloc_bytes_copied, ralloc.bytes_copied);
                        eprintln!(
                            "{key:?}/{query:?}/{count}/{threshold}/{phase}: \
                             host_allocations={} host_deallocations={} host_reallocations={} \
                             host_bytes={} host_reallocated_bytes={} buckets_64_256_1024_large={:?}",
                            host.allocations,
                            host.deallocations,
                            host.reallocations,
                            host.allocated_bytes,
                            host.reallocated_bytes,
                            host.size_buckets,
                        );
                        eprintln!("  runtime={runtime:?}");
                        eprintln!("  ralloc={ralloc:?}");
                    }
                }
            }
        }
    }
}
