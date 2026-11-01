use tinyone::{
    JitOptions,
    JitProgram,
    VerifiedProgram,
    compile_source_unoptimized_verified_with_filename,
    compile_source_verified_with_filename,
    run_verified_program_with_jit_options,
};

type SourceCompiler = fn(&str, &str) -> tinyone::Result<VerifiedProgram>;

fn assert_runtime_location(source: &str, expected_location: &str, expected_message: &str) {
    assert_runtime_location_with_op(source, expected_location, expected_message, None);
}

fn assert_runtime_location_with_op(
    source: &str,
    expected_location: &str,
    expected_message: &str,
    expected_op: Option<&str>,
) {
    let compilers: [SourceCompiler; 2] = [
        compile_source_unoptimized_verified_with_filename,
        compile_source_verified_with_filename,
    ];
    for (optimization, compile) in compilers.into_iter().enumerate() {
        let verified = compile(source, "locations.to").expect("location fixture compilation");
        let mut reference_output = Vec::new();
        let reference = run_verified_program_with_jit_options(
            &verified,
            "vm",
            &mut reference_output,
            Vec::new(),
            JitOptions::default(),
        )
        .expect_err("reference must fail during execution");
        assert!(matches!(reference, tinyone::TinyOneError::Runtime(_)), "preserve runtime error category");
        let message = reference.to_string();
        assert!(message.starts_with(expected_location), "optimization={optimization}: {message}");
        assert!(message.contains(expected_message), "{message}");
        for threshold in [0, 1, 8] {
            let options = JitOptions::new().with_hot_back_edge_threshold(threshold);
            let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("JIT lowering");
            if let Some(op) = expected_op {
                assert!(compiled.listing().contains(op), "fixture must select {op}: {}", compiled.listing());
            }
            for run in 0..2 {
                let mut output = Vec::new();
                let error = compiled.run(&mut output, Vec::new()).expect_err("JIT must fail");
                assert!(matches!(error, tinyone::TinyOneError::Runtime(_)), "preserve runtime error category");
                assert_eq!(error.to_string(), message, "optimization={optimization}, threshold={threshold}, run={run}");
                assert_eq!(output, reference_output, "preserve output before failure");
            }
        }
    }
}

#[test]
fn division_reports_the_operator_not_the_statement_end() {
    assert_runtime_location("let value = 10\nprint value / 0\n", "locations.to:2:13:", "Division by zero");
}

#[test]
fn fused_store_reports_the_failing_division() {
    assert_runtime_location(
        "let total = 1\nlet denominator = 0\ntotal = total / denominator\n",
        "locations.to:3:15:",
        "Division by zero",
    );
}

#[test]
fn function_failure_reports_the_callee_not_the_call_site() {
    assert_runtime_location(
        "fn divide(value) {\n    return 42 / value\n}\nprint divide(0)\n",
        "locations.to:2:15:",
        "Division by zero",
    );
}

#[test]
fn compound_expression_operation_sites_match_across_backends() {
    for (source, location) in [
        ("let values = [1]\nprint values[2]\n", "locations.to:2:13:"),
        ("let values = [1]\nset values[2] = 3\n", "locations.to:2:11:"),
        ("print len(1)\n", "locations.to:1:7:"),
        ("print -\"text\"\n", "locations.to:1:7:"),
        ("struct S { field }\nlet value = S(1)\nprint value.missing\n", "locations.to:3:13:"),
        ("\r\nlet value = 0\r\nprint (1 + 2 * 3) / value\r\n", "locations.to:3:19:"),
    ] {
        assert_runtime_location(source, location, "");
    }
}

#[test]
fn fused_map_get_distinguishes_lookup_and_arithmetic_failures() {
    for (source, location, message) in [
        (
            "let total = 0\nlet map = []\nlet key = 0\ntotal = total + map_get(map, key)\n",
            "locations.to:4:17:",
            "map_get expects a map",
        ),
        (
            "let total = 9223372036854775807\nlet map = map_new()\nlet key = 0\nlet ignored = map_set(map, key, 1)\ntotal = total + map_get(map, key)\n",
            "locations.to:5:15:",
            "Runtime.Memory_Overflow",
        ),
    ] {
        assert_runtime_location_with_op(source, location, message, Some("map.get.add.slots"));
    }
}

#[test]
fn fused_map_set_distinguishes_arithmetic_and_builtin_failures() {
    for (value, location, message) in [
        ("9223372036854775807", "locations.to:4:39:", "Runtime.Memory_Overflow"),
        ("2", "locations.to:4:15:", "map_set expects a map"),
    ] {
        let source =
            format!("let map = []\nlet key = 0\nlet value = {value}\nlet ignored = map_set(map, key, value * 2)\n");
        assert_runtime_location_with_op(&source, location, message, Some("map.set.mul.slots"));
    }
}

#[test]
fn fused_array_length_guard_reports_the_builtin() {
    assert_runtime_location_with_op(
        "let values = 1\nwhile len(values) > 0 { print 42 }\n",
        "locations.to:2:7:",
        "Expected heap pointer",
        Some("array.len.slot.jz"),
    );
}

#[test]
fn specialized_scalar_failure_keeps_its_source_across_repeated_runs() {
    let source = "let value = 7\nlet round = 0\nwhile round < 64 {\n    if round == 63 { value = true }\n    value = value * 1\n    round = round + 1\n}\n";
    assert_runtime_location(source, "locations.to:5:19:", "");
    let verified = compile_source_verified_with_filename(source, "locations.to").expect("feedback fixture");
    for threshold in [1, 8] {
        let options = JitOptions::new()
            .with_hot_back_edge_threshold(threshold)
            .with_execution_profile(true);
        let mut compiled = JitProgram::compile_verified_with_options(&verified, options).expect("feedback lowering");
        for _ in 0..2 {
            let error = compiled
                .run(&mut Vec::new(), Vec::new())
                .expect_err("Bool arithmetic must fail");
            assert!(error.to_string().starts_with("locations.to:5:19:"), "{error}");
            assert!(compiled.listing().contains("slot.mul.i.hot"));
            let profile = compiled.execution_profile().expect("enabled profile");
            assert!(profile.specialization_promotions > 0);
            assert!(profile.specialization_hits > 0);
            assert!(profile.specialization_misses > 0);
        }
    }
}
