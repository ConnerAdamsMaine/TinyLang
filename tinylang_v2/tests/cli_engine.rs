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
    let source =
        std::env::temp_dir().join(format!("tinylang-v2-cli-{}.tinyone", std::process::id(),));
    std::fs::write(&source, "fn add(a,b) { return a+b }").unwrap();
    for backend in ["interpreter", "asm"] {
        let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
            .args(["--backend", backend])
            .arg(&source)
            .args(["add", "19", "23"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"42\n");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_tinylang-v2"))
        .args(["--backend", "llvm"])
        .arg(&source)
        .arg("add")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8(output.stdout).unwrap().contains("define"));
    std::fs::remove_file(source).unwrap();
}
