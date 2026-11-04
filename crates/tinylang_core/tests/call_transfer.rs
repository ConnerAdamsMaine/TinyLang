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
fn source_closure_literals_capture_locals_and_accept_explicit_arguments() {
    assert_call_parity(
        r#"
        fn make_adder(base) {
          let add = fn(delta) { return base + delta }
          return add
        }
        let add = make_adder(40)
        print add(2)
        "#,
        ExpectedRun::Output("42\n"),
    );
}

#[test]
fn nested_source_closures_forward_captures_through_intermediate_functions() {
    assert_call_parity(
        r#"
        fn make_nested(base) {
          let outer = fn(offset) {
            return fn(extra) { return base + offset + extra }
          }
          return outer(1)
        }
        let nested = make_nested(40)
        print nested(1)
        "#,
        ExpectedRun::Output("42\n"),
    );
}

#[test]
fn source_closures_observe_enclosing_mutations_of_captured_bindings() {
    assert_call_parity(
        r#"
        fn make_reader(value) {
          let reader = fn() { return value }
          value = 99
          return reader
        }
        let reader = make_reader(7)
        print reader()
        "#,
        ExpectedRun::Output("99\n"),
    );
}

#[test]
fn immutable_captures_remain_plain_values() {
    assert_call_parity(
        r#"
        fn make_reader(value) {
          let reader = fn() { return value }
          return reader
        }
        let first = make_reader(7)
        let second = make_reader(8)
        print first() + second()
        "#,
        ExpectedRun::Output("15\n"),
    );
}

#[test]
fn sibling_closures_share_mutable_captured_state() {
    assert_call_parity(
        r#"
        fn make_counter() {
          let count = 0
          let increment = fn() {
            count = count + 1
            return count
          }
          let decrement = fn() {
            count = count - 1
            return count
          }
          return [increment, decrement]
        }
        let counters = make_counter()
        print counters[0]()
        print counters[0]()
        print counters[1]()
        print counters[0]()
        "#,
        ExpectedRun::Output("1\n2\n1\n2\n"),
    );
}

#[test]
fn separate_factory_invocations_have_independent_environments() {
    assert_call_parity(
        r#"
        fn make_counter(start) {
          let count = start
          return fn() { count = count + 1 return count }
        }
        let a = make_counter(0)
        let b = make_counter(100)
        print a()
        print a()
        print b()
        print a()
        print b()
        "#,
        ExpectedRun::Output("1\n2\n101\n3\n102\n"),
    );
}

#[test]
fn nested_closures_preserve_forwarded_binding_identity() {
    assert_call_parity(
        r#"
        fn make() {
          let total = 0
          let outer = fn() {
            let inner = fn(step) { total = total + step return total }
            return inner
          }
          let add = outer()
          let ignored = add(5)
          let again = outer()
          let also = again(7)
          return fn() { return total }
        }
        let read = make()
        print read()
        "#,
        ExpectedRun::Output("12\n"),
    );
}

#[test]
fn captured_environments_outlive_their_defining_invocation() {
    assert_call_parity(
        r#"
        fn make_accumulators() {
          let sum = 0
          let add = fn(x) { sum = sum + x return sum }
          let get = fn() { return sum }
          return [add, get]
        }
        let pair = make_accumulators()
        let filler = make_accumulators()
        let i = 0
        while i < 10 {
          let ignored = pair[0](i)
          let other = filler[0](1000)
          i = i + 1
        }
        print pair[1]()
        print filler[1]()
        "#,
        ExpectedRun::Output("45\n10000\n"),
    );
}

#[test]
fn enclosing_mutation_after_closure_call_is_visible_both_ways() {
    assert_call_parity(
        r#"
        fn run() {
          let value = 1
          let bump = fn() { value = value * 10 return value }
          let a = bump()
          value = value + 3
          let b = bump()
          return value
        }
        print run()
        "#,
        ExpectedRun::Output("130\n"),
    );
}

#[test]
fn captured_callbacks_can_be_called_directly() {
    assert_call_parity(
        r#"
        fn factory(callback) {
          return fn() { return callback() + 1 }
        }
        fn answer() { return 41 }
        let wrapped = factory(answer)
        print wrapped()
        "#,
        ExpectedRun::Output("42\n"),
    );
}

#[test]
fn arbitrary_expressions_can_be_called() {
    assert_call_parity(
        r#"
        let result = (fn(x) {
          return x * 2
        })(21)
        print result
        fn double(x) { return x * 2 }
        let callbacks = [double, fn(x) { return x + 1 }]
        print callbacks[0](21)
        print callbacks[1](41)
        "#,
        ExpectedRun::Output("42\n42\n42\n"),
    );
}

#[test]
fn closures_cannot_break_out_of_enclosing_loops() {
    for source in [
        "while true { let callback = fn() { break } }",
        "while true { let callback = fn() { continue } }",
        "fn outer() { while true { fn inner() { break } } }",
    ] {
        assert!(compile_source_verified(source).is_err(), "{source} must be rejected");
    }
}

#[test]
fn closures_do_not_inherit_enclosing_unsafe_blocks() {
    let source = "unsafe { let callback = fn(p) { return free(p) } }";
    assert!(compile_source_verified(source).is_err());
}

#[test]
fn generated_closure_names_are_reserved() {
    assert!(compile_source_verified("fn __tiny_closure_0() { return 1 }").is_err());
    let verified =
        compile_source_verified(r#"let f = fn() { return 1 } let forged = closure_new("__tiny_closure_0", [])"#)
            .expect("forged name compiles");
    let mut output = Vec::new();
    assert!(run_verified_program(&verified, "vm", &mut output, Vec::new()).is_err());
}

#[test]
fn closure_captures_requires_unsafe_and_snapshots_cells() {
    assert!(
        compile_source_verified("fn f(a) { return a } let c = closure_new(\"f\", [1]) print closure_captures(c)")
            .is_err()
    );
    assert_call_parity(
        r#"
        fn make() {
          let n = 1
          let f = fn() { n = n + 1 return n }
          return f
        }
        let f = make()
        let ignored = f()
        unsafe {
          print closure_captures(f)[0]
        }
        "#,
        ExpectedRun::Output("2\n"),
    );
}

#[test]
fn source_closures_resolve_top_level_bindings_as_live_globals() {
    assert_call_parity(
        r#"
        let value = 1
        let reader = fn() { return value }
        value = 2
        print reader()
        "#,
        ExpectedRun::Output("2\n"),
    );
}

#[test]
fn source_closure_assignments_persist_across_calls() {
    assert_call_parity(
        r#"
        fn make_writer(value) {
          let writer = fn() { value = value + 1 return value }
          value = 99
          return writer
        }
        let writer = make_writer(7)
        print writer()
        print writer()
        "#,
        ExpectedRun::Output("100\n101\n"),
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

#[test]
fn local_bindings_shadow_builtin_names_in_call_position() {
    assert_call_parity(
        r#"
        fn run(read) {
          let len = fn(x) { return x + 1 }
          return read() + len(1)
        }
        print run(fn() { return 40 })
        "#,
        ExpectedRun::Output("42\n"),
    );
}
