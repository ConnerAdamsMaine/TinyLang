use std::fs;

use tinylang::{
    CompileCacheStatus,
    DecoratorArgument,
    Op,
    Program,
    VerifiedProgram,
    compile_file_cached_verified_with_status,
    compile_file_verified,
    compile_source_unoptimized_verified,
    compile_source_verified,
    run_verified_program,
};

const MULTIPLY: &str =
    "import \"stdx\" as stdx\n@stdx.coprocessor(Arduino)\nfn multiply(a, b) { return a * b }\nprint multiply(25, 40)";

#[test]
fn canonical_targets_and_aliases_are_equivalent_metadata() {
    let original = compile_source_verified(MULTIPLY).expect("compile placement");
    let function = &original.program().functions()[0];
    assert_eq!(function.name(), "multiply");
    assert_eq!(function.coprocessor_target(), Some("Arduino"));
    assert_eq!(function.decorators()[0].path(), ["stdx", "coprocessor"]);
    assert_eq!(function.decorators()[0].arguments(), [DecoratorArgument::String("Arduino".into())]);
    assert_eq!(original.coprocessor_target(0).expect("root hardware grant"), Some("Arduino"));
    assert!(original.coprocessor_target(100).is_err());
    for alias in ["stdx", "ext", "extensions"] {
        for target in ["Arduino", "\"Arduino\""] {
            let source = format!(
                "import \"stdx\" as {alias}\n@{alias}.coprocessor({target})\nfn multiply(a, b) {{ return a * b }}\nprint multiply(25, 40)"
            );
            let compiled = compile_source_verified(&source).expect("alias and target syntax");
            assert_eq!(compiled.fingerprint(), original.fingerprint());
        }
    }
    for target in ["RP2350", "ESP32", "FPGA0"] {
        let source = MULTIPLY.replace("Arduino", target);
        assert_eq!(
            compile_source_verified(&source)
                .expect("backend-neutral target")
                .program()
                .functions()[0]
                .coprocessor_target(),
            Some(target)
        );
    }
}

#[test]
fn optimization_and_both_execution_modes_preserve_ordinary_calls() {
    let undecorated = "fn helper(x) { return x * 2 }\nfn work(x) { return helper(x) }\nprint work(21)";
    let decorated = "import \"stdx\" as ext\nfn helper(x) { return x * 2 }\n@ext.coprocessor(Arduino)\nfn work(x) { return helper(x) }\nprint work(21)";
    let baseline = compile_source_verified(undecorated).expect("ordinary bytecode");
    let placed = compile_source_verified(decorated).expect("placed bytecode");
    assert_eq!(baseline.program().code(), placed.program().code());
    for (before, after) in baseline.program().functions().iter().zip(placed.program().functions()) {
        assert_eq!(before.code(), after.code());
    }
    assert!(
        placed.program().functions()[1]
            .code()
            .iter()
            .any(|instruction| instruction.op == Op::Call)
    );
    let unoptimized = compile_source_unoptimized_verified(decorated).expect("unoptimized");
    assert_eq!(unoptimized.program().functions()[1].decorators(), placed.program().functions()[1].decorators());
    assert_ne!(baseline.fingerprint(), placed.fingerprint());
    for mode in ["vm", "jit"] {
        let mut output = Vec::new();
        run_verified_program(&placed, mode, &mut output, Vec::new()).expect("ordinary host execution");
        assert_eq!(output, b"42\n");
    }
}

#[test]
fn future_lowerer_can_enumerate_targets_after_untrusted_round_trips() {
    let compiled = compile_source_verified(MULTIPLY).expect("compile");
    let binary = compiled.program().to_binary_artifact().expect("binary");
    assert_eq!(u16::from_le_bytes([binary[8], binary[9]]), 5);
    let json = compiled.program().to_artifact();
    for decoded in [
        Program::from_binary_artifact(&binary).expect("decode binary"),
        Program::from_artifact(json).expect("decode JSON"),
    ] {
        let verified = VerifiedProgram::verify(decoded).expect("untrusted metadata verification");
        let targets = verified
            .program()
            .functions()
            .iter()
            .filter_map(|function| {
                function
                    .coprocessor_target()
                    .map(|target| format!("{} -> {target}", function.name()))
            })
            .collect::<Vec<_>>();
        assert_eq!(targets, ["multiply -> Arduino"]);
        assert_eq!(verified.fingerprint(), compiled.fingerprint());
        assert!(verified.coprocessor_target(0).is_err(), "serialized metadata cannot authenticate a grant");
    }
}

#[test]
fn invalid_source_returns_located_compile_errors() {
    let cases = [
        ("@stdx.coprocessor(Arduino) fn x() {}", "Unknown decorator namespace"),
        ("import \"stdx\" as ext @stdx.coprocessor(Arduino) fn x() {}", "Unknown decorator namespace"),
        ("import \"stdx\" as stdx @stdx.unknown fn x() {}", "is not registered"),
        ("@unknown fn x() {}", "is not registered"),
        ("import \"stdx\" as stdx @stdx.coprocessor() fn x() {}", "exactly 1 argument"),
        ("import \"stdx\" as stdx @stdx.coprocessor(Arduino, ESP32) fn x() {}", "exactly 1 argument"),
        ("import \"stdx\" as stdx @stdx.coprocessor(4) fn x() {}", "identifier or string"),
        ("import \"stdx\" as stdx @stdx.coprocessor(true) fn x() {}", "identifier or string"),
        ("import \"stdx\" as stdx @stdx.coprocessor(\"\") fn x() {}", "must not be empty"),
        ("import \"stdx\" as stdx @stdx.coprocessor(\"  \") fn x() {}", "must not be empty"),
        ("import \"stdx\" as stdx @stdx.coprocessor(1 + 2) fn x() {}", "static"),
        ("import \"stdx\" as stdx @stdx.coprocessor(call()) fn x() {}", "static"),
        (
            "import \"stdx\" as stdx @stdx.coprocessor(Arduino) @stdx.coprocessor(ESP32) fn x() {}",
            "more than once",
        ),
        ("import \"stdx\" as stdx @stdx.coprocessor(Arduino)", "Dangling"),
        ("import \"stdx\" as stdx @stdx.coprocessor(Arduino) let x = 10", "function declarations"),
        ("import \"stdx\" as stdx fn x() { @stdx.coprocessor(Arduino) let y = 1 }", "top level"),
        ("import \"stdx\" as ext fn ext() {}", "namespace"),
        ("import \"stdx\" as ext let ext = 1", "namespace"),
        ("import \"stdx\" as ext import \"stdx\" as ext", "already defined"),
    ];
    for (source, message) in cases {
        let error = compile_source_verified(source).expect_err("reject invalid source");
        assert!(error.to_string().contains(message), "expected {message:?}: {error}");
        assert!(error.to_string().contains("<source>"), "missing source location: {error}");
    }
}

#[test]
fn configured_hardware_denial_cannot_be_overridden_by_a_decorator() {
    let directory = tempfile::tempdir().expect("project");
    let root = directory.path().join("main.to");
    fs::write(&root, MULTIPLY).expect("source");
    fs::write(directory.path().join("Config.toml"), "[permissions]\ncodebase = []\n").expect("deny hardware");
    let error = compile_file_verified(&root).expect_err("placement is not a grant");
    assert!(error.to_string().contains("Hardware"), "{error}");
    assert!(error.to_string().contains("main.to"), "{error}");
}

#[test]
fn imported_functions_require_their_own_grants_and_extension_imports() {
    let directory = tempfile::tempdir().expect("project");
    let root = directory.path().join("main.to");
    let module = directory.path().join("device.to");
    fs::write(&root, "import \"device\" as device\nprint device.work(21)").expect("root");
    let source = "import \"stdx\" as ext\n@ext.coprocessor(Arduino)\nexport fn work(x) { return x * 2 }";
    fs::write(&module, source).expect("placed module");
    let config = directory.path().join("Config.toml");
    fs::write(&config, "[modules.device]\npath = \"device.to\"\npermissions = []\n").expect("unprivileged module");
    assert!(
        compile_file_verified(&root)
            .expect_err("root grant cannot be borrowed")
            .to_string()
            .contains("hardware capability grant")
    );
    fs::write(&config, "[modules.device]\npath = \"device.to\"\npermissions = [\"hardware\"]\n")
        .expect("external module grant");
    let program = compile_file_verified(&root).expect("approved placement");
    assert_eq!(program.program().modules().len(), 1, "stdx does not create a runtime dependency");
    assert_eq!(program.coprocessor_target(0).expect("owning grant"), Some("Arduino"));
    for mode in ["vm", "jit"] {
        let mut output = Vec::new();
        run_verified_program(&program, mode, &mut output, Vec::new()).expect("ordinary imported calls");
        assert_eq!(output, b"42\n");
    }
    fs::write(&module, source.replace("import \"stdx\" as ext\n", "")).expect("missing module import");
    fs::write(&root, "import \"stdx\" as ext\nimport \"device\" as device\nprint device.work(21)")
        .expect("root import only");
    assert!(
        compile_file_verified(&root)
            .expect_err("namespace imports stay module-local")
            .to_string()
            .contains("Unknown decorator namespace")
    );
}

#[test]
fn cache_rebuilds_changed_placement_and_preserves_trusted_source_grants() {
    let directory = tempfile::tempdir().expect("project");
    let root = directory.path().join("main.to");
    // Exercise the cache artifact route instead of its small-project bypass.
    let padding = "# cache source padding\n".repeat(300);
    fs::write(&root, format!("{MULTIPLY}\n{padding}")).expect("source");
    let (before, _) = compile_file_cached_verified_with_status(&root).expect("compile original");
    let (same, status) = compile_file_cached_verified_with_status(&root).expect("repeat source");
    assert_eq!(status, CompileCacheStatus::Miss, "placement requires trusted source compilation");
    assert_eq!(same.coprocessor_target(0).expect("external root grant"), Some("Arduino"));
    fs::write(&root, format!("{}\n{padding}", MULTIPLY.replace("Arduino", "ESP32"))).expect("change target");
    let (after, _) = compile_file_cached_verified_with_status(&root).expect("compile changed");
    assert_eq!(after.coprocessor_target(0).expect("external root grant"), Some("ESP32"));
    assert_ne!(before.fingerprint(), after.fingerprint());
}
