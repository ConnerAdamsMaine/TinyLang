//! Reproducible performance baselines for the tynra-v2 engine (R2 line 125).
//!
//! Measures, over a fixed scalar-only workload set that matches the v2.1
//! release scope:
//! - compile latency (p50/p95/p99)
//! - warm execution latency (p50/p95/p99) for the VM and each engine backend,
//!   both the single-function path and the whole-program path
//! - sustained throughput (invocations/second) for the same invoke rows
//! - process cold start (spawn the engine CLI on a fixed source, p50/p95)
//! - process peak RSS at exit (Windows working set / Linux VmHWM)
//! - allocator activity per workload (only with `--features testing-hooks
//!   -- --alloc`; the instrumentation hook itself perturbs timing, so timing
//!   rows must be captured from a default-features build)
//!
//! Output is CSV on stdout. Correctness is asserted once per workload before
//! measuring; a wrong result aborts the run.

use std::hint::black_box;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use tynra::engine::{ExecutionBackend, ScalarFunction, ScalarProgram};
use tynra::{RuntimeValue, VM, VmExecutionLimits, compile_source_verified};

const SAMPLES: usize = 101;
const SAMPLE_INDEXES: [usize; 3] = [50, 95, 99];
const BATCH: usize = 64;
const FUEL: u64 = 1_000_000;
const COLD_SAMPLES: usize = 21;

struct Workload {
    name: &'static str,
    source: &'static str,
    input: i64,
    expected: i64,
    /// Single-function scalar lowering rejects `Call`; workloads with
    /// recursion measure the program path and VM only.
    has_calls: bool,
}

const WORKLOADS: [Workload; 4] = [
    Workload {
        name: "sum100",
        source: "fn sum(n) { let t=0 let i=0 while i<n { t=t+i i=i+1 } return t }",
        input: 100,
        expected: 4_950,
        has_calls: false,
    },
    Workload {
        name: "fact10",
        source: "fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }",
        input: 10,
        expected: 3_628_800,
        has_calls: true,
    },
    Workload {
        name: "sel50",
        source: "fn sel(n) { if n < 10 { return n + 1 } if n < 100 { return n + 2 } return n + 3 }",
        input: 50,
        expected: 52,
        has_calls: false,
    },
    Workload {
        name: "arith20",
        source: "fn arith(n) { let a = n * 2 + 1 let b = (a + 3) * (a - 2) return b - n }",
        input: 20,
        expected: 1_696,
        has_calls: false,
    },
];

/// Times `SAMPLES` runs of `operation`, each timed over `batch` calls, and
/// returns per-call nanoseconds (matching `engine_bench`'s methodology).
fn measure(
    operation: &mut dyn FnMut() -> Result<(), Box<dyn std::error::Error>>,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    for _ in 0..8 {
        operation()?;
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..BATCH {
            operation()?;
        }
        samples.push(start.elapsed().as_nanos() as f64 / BATCH as f64);
    }
    samples.sort_unstable_by(|a, b| a.total_cmp(b));
    Ok(samples)
}

fn print_row(metric: &str, workload: &str, samples: &[f64]) {
    let p = SAMPLE_INDEXES.map(|index| format!("{:.0}", samples[index]));
    let ops_per_s = 1e9 / (samples.iter().sum::<f64>() / samples.len() as f64);
    println!("{metric},{workload},{},{},{},{ops_per_s:.0}", p[0], p[1], p[2]);
}

#[cfg(feature = "testing-hooks")]
fn print_alloc_row(metric: &str, workload: &str, batch: u64) {
    let snapshot = ralloc::testing::instrumentation_snapshot();
    println!(
        "{metric},{workload},{batch},{},{},{},{},{},{},{}",
        snapshot.allocations,
        snapshot.deallocations,
        snapshot.reallocations,
        snapshot.requested_bytes,
        snapshot.peak_payload_bytes,
        snapshot.growth_events,
        snapshot.bytes_copied,
    );
}

fn cold_start_rows(cli: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::temp_dir().join("perf_baseline_cold.tl");
    std::fs::write(&source, "fn main() { return 7 * 6 }\n")?;
    for backend in ["interpreter", "asm"] {
        // One untimed warm spawn validates the contract (prints 42, exit 0).
        let probe = Command::new(cli).arg("--backend").arg(backend).arg(&source).arg("main").output()?;
        if !(probe.status.success() && probe.stdout == b"42\n") {
            return Err(format!(
                "cold-start contract failed for {backend}: status={:?} stdout={:?}",
                probe.status, probe.stdout
            )
            .into());
        }
        let mut samples = Vec::with_capacity(COLD_SAMPLES);
        for _ in 0..COLD_SAMPLES {
            let start = Instant::now();
            let output = Command::new(cli)
                .arg("--backend")
                .arg(backend)
                .arg(&source)
                .arg("main")
                .output()?;
            samples.push(start.elapsed().as_secs_f64() * 1e9);
            if !(output.status.success() && output.stdout == b"42\n") {
                return Err(format!("cold-start run failed for {backend}: {:?}", output.status).into());
            }
        }
        samples.sort_unstable_by(|a, b| a.total_cmp(b));
        let p50 = samples[COLD_SAMPLES / 2];
        let p95 = samples[COLD_SAMPLES - 2];
        println!("cold_start,{backend},,{p50:.0},{p95:.0},{COLD_SAMPLES}");
    }
    Ok(())
}

fn peak_rss_bytes() -> Option<u64> {
    #[cfg(windows)]
    unsafe fn query() -> Option<u64> {
        #[repr(C)]
        struct ProcessMemoryCounters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set: usize,
            working_set: usize,
            quota_peak_paged: usize,
            quota_paged: usize,
            quota_peak_non_paged: usize,
            quota_non_paged: usize,
            pagefile: usize,
            peak_pagefile: usize,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn K32GetProcessMemoryInfo(
                process: *mut std::ffi::c_void,
                counters: *mut ProcessMemoryCounters,
                size: u32,
            ) -> i32;
        }
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            page_fault_count: 0,
            peak_working_set: 0,
            working_set: 0,
            quota_peak_paged: 0,
            quota_paged: 0,
            quota_peak_non_paged: 0,
            quota_non_paged: 0,
            pagefile: 0,
            peak_pagefile: 0,
        };
        // SAFETY: the pseudo-handle (-1) is the current process and `counters`
        // is initialized with its own size for the duration of the call.
        let ok = unsafe { K32GetProcessMemoryInfo((-1isize) as *mut std::ffi::c_void, &mut counters, counters.cb) };
        (ok != 0).then_some(counters.peak_working_set as u64)
    }

    #[cfg(windows)]
    {
        // SAFETY: `query` owns the call contract and takes no inputs.
        unsafe { query() }
    }

    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
        let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kb * 1024)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}

fn cli_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Examples live in target/release/examples/, bins in target/release/:
    // check the sibling directory and its parent.
    let mut candidates = Vec::new();
    if let Some(dir) = exe.parent() {
        candidates.push(dir.join(format!("tynra-v2{}", std::env::consts::EXE_SUFFIX)));
        if let Some(parent) = dir.parent() {
            candidates.push(parent.join(format!("tynra-v2{}", std::env::consts::EXE_SUFFIX)));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let with_alloc = std::env::args().any(|arg| arg == "--alloc");
    #[cfg(not(feature = "testing-hooks"))]
    if with_alloc {
        return Err("--alloc requires a build with --features testing-hooks".into());
    }
    #[cfg(feature = "testing-hooks")]
    {
        ralloc::testing::reset_instrumentation();
    }
    let backends: Vec<(&str, ExecutionBackend)> = vec![
        ("interpreter", ExecutionBackend::Interpreter),
        ("asm", ExecutionBackend::NativeAsm),
    ];
    #[cfg(feature = "cranelift")]
    let backends = {
        let mut with_cranelift = backends;
        with_cranelift.push(("cranelift", ExecutionBackend::Cranelift));
        with_cranelift
    };
    let mut out = std::io::stdout().lock();
    writeln!(out, "# tynra-v2 perf baseline: samples={SAMPLES} batch={BATCH} fuel={FUEL}")?;
    writeln!(out, "# columns: metric,workload,p50_ns,p95_ns,p99_ns,ops_per_s")?;
    writeln!(
        out,
        "# alloc columns: metric,workload,batch,allocations,deallocations,reallocations,requested_bytes,peak_payload_bytes,growth_events,bytes_copied"
    )?;

    for workload in &WORKLOADS {
        // Correctness precondition: every measured path produces the pinned value.
        let program = compile_source_verified(workload.source)?;
        // The single-function scalar path rejects `Call`; workloads with
        // recursion skip it and measure the program path and VM only.
        let scalar = if workload.has_calls {
            None
        } else {
            Some(ScalarFunction::lower(&program, 0)?)
        };
        let scalar_program = ScalarProgram::lower(&program, 0)?;
        let mut vm = VM::from_verified_with_limits(&program, vec![], VmExecutionLimits::default())?;
        let vm_result = vm.invoke_function_with_fuel(0, vec![RuntimeValue::I64(workload.input)], &mut Vec::new(), FUEL)?;
        if vm_result != RuntimeValue::I64(workload.expected) {
            return Err(format!("VM result for {} is {vm_result:?}, expected {}", workload.name, workload.expected).into());
        }

        // Compile rows: one set per workload.
        let samples = measure(&mut || {
            black_box(compile_source_verified(black_box(workload.source))?);
            Ok(())
        })?;
        print_row("compile", workload.name, &samples);

        // Backend rows: single-function path and whole-program path.
        // The single-function scalar path rejects `Call`, so workloads with
        // recursion measure the program path and VM only.
        for (name, backend) in &backends {
            if !workload.has_calls {
                let scalar = scalar.as_ref().expect("function scalar present for call-free workloads");
                let samples = measure(&mut || {
                    black_box(backend.prepare(black_box(scalar))?);
                    Ok(())
                })?;
                print_row(&format!("{name}_prepare"), workload.name, &samples);
                let executable = backend.prepare(scalar)?;
                let samples = measure(&mut || {
                    black_box(executable.invoke(black_box(&[workload.input]), FUEL)?);
                    Ok(())
                })?;
                print_row(&format!("{name}_invoke"), workload.name, &samples);
            }

            let samples = measure(&mut || {
                black_box(backend.prepare_program(black_box(&scalar_program))?);
                Ok(())
            })?;
            print_row(&format!("program_{name}_prepare"), workload.name, &samples);
            let program_executable = backend.prepare_program(&scalar_program)?;
            let samples = measure(&mut || {
                black_box(program_executable.invoke(black_box(&[workload.input]), FUEL)?);
                Ok(())
            })?;
            print_row(&format!("program_{name}_invoke"), workload.name, &samples);

            #[cfg(feature = "testing-hooks")]
            if with_alloc {
                // Allocator activity for one invoke batch over the program
                // executable (the path every workload shares).
                ralloc::testing::reset_instrumentation();
                for _ in 0..BATCH {
                    black_box(program_executable.invoke(black_box(&[workload.input]), FUEL)?);
                }
                print_alloc_row(&format!("alloc_program_{name}_invoke"), workload.name, BATCH as u64);
            }
        }

        #[cfg(feature = "testing-hooks")]
        if with_alloc {
            // Allocator activity for a single compile, once per workload.
            ralloc::testing::reset_instrumentation();
            black_box(compile_source_verified(black_box(workload.source))?);
            print_alloc_row("alloc_compile", workload.name, 1);
        }

        // VM rows reuse one VM across samples (warm-state methodology as in engine_bench).
        let samples = measure(&mut || {
            black_box(vm.invoke_function_with_fuel(0, vec![RuntimeValue::I64(workload.input)], &mut Vec::new(), FUEL)?);
            Ok(())
        })?;
        print_row("vm_invoke", workload.name, &samples);
    }

    if let Some(cli) = cli_path() {
        cold_start_rows(&cli)?;
    } else {
        writeln!(out, "# cold_start rows skipped: tynra-v2 binary not found next to this example")?;
    }

    if let Some(bytes) = peak_rss_bytes() {
        writeln!(out, "peak_rss,process,,{bytes},,")?;
    }
    out.flush()?;
    Ok(())
}
