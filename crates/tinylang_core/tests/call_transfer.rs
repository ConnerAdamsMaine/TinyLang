use tinylang::{JitOptions, JitProgram, compile_source_verified, run_verified_program};

enum ExpectedRun {
    Output(&'static str),
    Error(&'static str),
}

fn assert_call_parity(source: &str, expected: ExpectedRun) {
    let verified = compile_source_verified(source).expect("call-transfer fixture");
    let mut reference_output = Vec::new();
    let reference = run_verified_program(&verified, "vm", &mut reference_output, Vec::new())
        .map(|memory| memory.snapshot())
        .map_err(|error| error.to_string());
    match expected {
        ExpectedRun::Output(output) => {
            assert!(reference.is_ok(), "{reference:?}");
            assert_eq!(reference_output, output.as_bytes());
        }
        ExpectedRun::Error(message) => {
            let error = reference.as_ref().expect_err("reference must fail");
            assert!(error.contains(message), "{error:?} must contain {message:?}");
        }
    }
    for threshold in [0, 1, 8] {
        let mut compiled = JitProgram::compile_verified_with_options(
            &verified,
            JitOptions::new().with_hot_back_edge_threshold(threshold),
        )
        .expect("call-transfer lowering");
        for _ in 0..2 {
            let mut output = Vec::new();
            let actual = compiled
                .run(&mut output, Vec::new())
                .map(|memory| memory.snapshot())
                .map_err(|error| error.to_string());
            assert_eq!(output, reference_output, "threshold {threshold}");
            assert_eq!(actual, reference, "threshold {threshold}");
        }
    }
}

#[test]
fn nested_calls_preserve_operand_prefixes_and_argument_side_effect_order() {
    assert_call_parity(
        r#"
        fn mark(cell, value) {
          print value
          let ignored = store(cell, load(cell) * 10 + value)
          return value
        }
        fn pack(a, b, c) { return a * 100 + b * 10 + c }
        let state = alloc(0)
        print 1000 + pack(mark(state, 1), mark(state, 2), mark(state, 3))
        let callback = pack
        print 2000 + callback(mark(state, 4), mark(state, 5), mark(state, 6))
        let captured = closure_new("pack", [7])
        print 3000 + captured(mark(state, 8), mark(state, 9))
        print load(state)
        "#,
        ExpectedRun::Output("1\n2\n3\n1123\n4\n5\n6\n2456\n8\n9\n3789\n12345689\n"),
    );
}

#[test]
fn zero_arguments_and_distinct_closure_instances_keep_fresh_captures() {
    assert_call_parity(
        r#"
        fn answer() { return 3 }
        fn bump(base) { base = base + 1 return base }
        let function = answer
        let empty = closure_new("answer", [])
        print answer() + function() + empty()
        let first = closure_new("bump", [41])
        let second = closure_new("bump", [7])
        let i = 0
        let total = 0
        while i < 32 {
          let selected = first
          if i < 16 { selected = second }
          total = total + selected()
          i = i + 1
        }
        print total
        print first()
        print second()
        "#,
        ExpectedRun::Output("9\n800\n42\n8\n"),
    );
}

#[test]
fn closure_borrow_ends_before_recursive_calls_and_callee_heap_mutation() {
    assert_call_parity(
        r#"
        fn recurse(self, depth) {
          if depth == 0 { return 1 }
          return 1 + self(self, depth - 1)
        }
        fn release(self) { let ignored = unsafe free(self) return 7 }
        let recursive = closure_new("recurse", [])
        print recursive(recursive, 10)
        let disposable = closure_new("release", [])
        print disposable(disposable)
        "#,
        ExpectedRun::Output("11\n7\n"),
    );
}

#[test]
fn dynamic_call_errors_keep_validation_order_and_repeat_cleanly() {
    for (source, message) in [
        ("let callback = 4 print callback(1)", "CallValue expects a function or Closure"),
        ("let callback = [4] print callback(1)", "CallValue expects a function or Closure"),
        (
            "fn pair(a, b) { return a + b } let callback = pair print callback(1)",
            "expects 2 argument(s), got 1",
        ),
        (
            r#"fn pair(a, b) { return a + b } let callback = closure_new("pair", [7]) print callback()"#,
            "expects 2 argument(s), got 1",
        ),
        (
            r#"fn pair(a, b) { return a + b } let callback = closure_new("pair", [7, 8, 9]) print callback()"#,
            "expects 2 argument(s), got 3",
        ),
        (
            r#"fn answer() { return 7 } let callback = closure_new("answer", [])
               let ignored = unsafe free(callback) let replacement = alloc(4) print callback()"#,
            "Stale heap pointer",
        ),
        (
            "fn recurse(self, depth) { if depth > 0 { return self(self, depth - 1) } return self() }
             let callback = recurse print callback(callback, 15)",
            "expects 2 argument(s), got 0",
        ),
        (
            "fn recurse(self) { return self(self) } let callback = recurse print callback(callback)",
            "Call stack overflow",
        ),
    ] {
        assert_call_parity(source, ExpectedRun::Error(message));
    }
}
