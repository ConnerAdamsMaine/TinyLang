use std::process::Command;

#[test]
fn cli_rejects_unknown_backend_and_bad_arguments_without_success_output() {
    for args in [vec!["--backend", "pretend"], vec!["--fuel", "zero"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn cli_executes_source_via_explicit_backend_and_emits_llvm_ir() {
    let source = std::env::temp_dir().join(format!("tinylang-v2-cli-{}.tinylang", std::process::id(),));
    std::fs::write(&source, "fn add(a,b) { return a+b }").unwrap();
    for backend in ["interpreter", "asm"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
            .args(["--backend", backend])
            .arg(&source)
            .args(["add", "19", "23"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, b"42\n");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
        .args(["--backend", "llvm"])
        .arg(&source)
        .arg("add")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(String::from_utf8(output.stdout).unwrap().contains("define"));
    std::fs::remove_file(source).unwrap();
}

/// Source whose entry contains a call: leaf lowering rejects `CALL`, the CLI
/// must escalate to the program profile on the interpreter and ASM backends,
/// and the backends without a program implementation must fail explicitly
/// rather than fall back.
#[test]
fn cli_program_fallback_executes_call_closures_and_reports_missing_backends() {
    let source = std::env::temp_dir().join(format!("tinylang-v2-cli-prog-{}.tinylang", std::process::id(),));
    std::fs::write(&source, "fn fact(n) { if n < 2 { return 1 } return n * fact(n - 1) }").unwrap();
    for backend in ["interpreter", "asm"] {
        for (argument, expected) in [("5", "120\n"), ("10", "3628800\n")] {
            let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
                .args(["--backend", backend])
                .arg(&source)
                .args(["fact", argument])
                .output()
                .unwrap();
            assert!(output.status.success(), "{backend} {argument}: {}", String::from_utf8_lossy(&output.stderr));
            assert_eq!(output.stdout, expected.as_bytes(), "{backend} {argument}");
        }
    }
    // `cranelift` executes when compiled in and reports unavailability
    // otherwise; `llvm` emits the program-profile IR for the same closure
    // instead of reporting a gap.
    let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
        .args(["--backend", "cranelift"])
        .arg(&source)
        .args(["fact", "5"])
        .output()
        .unwrap();
    if cfg!(feature = "cranelift") {
        assert!(output.status.success(), "cranelift: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, b"120\n", "cranelift");
    } else {
        assert!(!output.status.success(), "cranelift must not report success");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("unavailable"), "cranelift: {stderr}");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
        .args(["--backend", "llvm"])
        .arg(&source)
        .arg("fact")
        .output()
        .unwrap();
    assert!(output.status.success(), "llvm: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("; entry: tinylang_program_"), "llvm program header: {stdout}");
    assert!(stdout.contains("call i64 @tinylang_program_"), "llvm program call: {stdout}");
    std::fs::remove_file(source).unwrap();
}
