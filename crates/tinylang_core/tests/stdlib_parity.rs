use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

use tinylang::{compile_source, run_program};

fn run_modes(source: &str) -> (String, String) {
    let mut vm = Vec::new();
    let mut jit = Vec::new();
    let program = compile_source(source).expect("compile");
    run_program(Arc::clone(&program), "vm", &mut vm, Vec::new()).expect("vm run");
    run_program(Arc::clone(&program), "jit", &mut jit, Vec::new()).expect("jit run");
    (String::from_utf8(vm).unwrap(), String::from_utf8(jit).unwrap())
}

fn assert_parity(source: &str, expected: &str) {
    let (vm, jit) = run_modes(source);
    assert_eq!(vm, expected, "vm output mismatch");
    assert_eq!(jit, expected, "jit output mismatch");
}

#[test]
fn vec_push_pop_round_trip_matches_backends() {
    let source = r"
    let v = vec_new()
    let ignored = push(v, 1)
    let ignored2 = push(v, 2)
    let ignored3 = push(v, 3)
    print map_len(map_new())
    print len(v)
    print v[0]
    print v[1]
    print v[2]
    print pop(v)
    print len(v)
    print vec_clear(v)
    print len(v)
    ";
    assert_parity(source, "0\n3\n1\n2\n3\n3\n2\n0\n0\n");
}

#[test]
fn map_basic_operations_match_backends() {
    let source = r"
    let m = map_new()
    print map_set(m, 1, 100)
    print map_set(m, 2, 200)
    print map_set(m, 1, 111)
    print map_get(m, 1)
    print map_get(m, 2)
    print map_has(m, 1)
    print map_has(m, 99)
    print map_len(m)
    print map_del(m, 1)
    print map_len(m)
    print map_has(m, 1)
    ";
    assert_parity(source, "100\n200\n111\n111\n200\n1\n0\n2\n1\n1\n0\n");
}

#[test]
fn map_numeric_key_boundaries_preserve_identity_and_original_key_types() {
    let source = r"
    let minimum = -9223372036854775807 - 1
    let maximum = 9223372036854775807
    let keys = [u8(255), u16(65535), u32(4294967295), -1, minimum, maximum]
    let m = map_new()
    let i = 0
    while i < len(keys) {
      let ignored = map_set(m, keys[i], i + 10)
      i = i + 1
    }
    print map_len(m)
    print map_get(m, 255)
    print map_get(m, 65535)
    print map_get(m, 4294967295)
    print map_get(m, -1)
    print map_get(m, minimum)
    print map_get(m, maximum)
    let updated = map_set(m, 255, 99)
    print map_get(m, u16(255))
    let ordered = map_keys(m)
    print type_of(ordered[0])
    print type_of(ordered[1])
    print type_of(ordered[2])
    print map_len(m)
    print map_del(m, u32(255))
    print map_has(m, u8(255))
    print map_get(m, u32(65535))
    print map_len(m)
    let reinserted = map_set(m, u8(255), 77)
    print map_get(m, 255)
    ";
    let expected = b"6\n10\n11\n12\n13\n14\n15\n99\nu8\nu16\nu32\n6\n1\n0\n11\n5\n77\n";
    let verified = tinylang::compile_source_verified(source).expect("numeric map key fixture");
    let mut reference = Vec::new();
    tinylang::run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM execution");
    assert_eq!(reference, expected);
    for threshold in [0, 1, 8] {
        let mut compiled = tinylang::JitProgram::compile_verified_with_options(
            &verified,
            tinylang::JitOptions::new().with_hot_back_edge_threshold(threshold),
        )
        .expect("JIT lowering");
        for _ in 0..2 {
            let mut output = Vec::new();
            compiled.run(&mut output, Vec::new()).expect("JIT execution");
            assert_eq!(output, reference, "threshold={threshold}");
        }
    }
}

#[test]
fn map_keys_and_values_preserve_insertion_order() {
    let source = r"
    let m = map_new()
    let ignored = map_set(m, 30, 1)
    let ignored2 = map_set(m, 10, 2)
    let ignored3 = map_set(m, 20, 3)
    let keys = map_keys(m)
    let values = map_values(m)
    print keys[0]
    print keys[1]
    print keys[2]
    print values[0]
    print values[1]
    print values[2]
    ";
    assert_parity(source, "30\n10\n20\n1\n2\n3\n");
}

#[test]
fn map_index_handles_content_identity_and_index_shifts() {
    let source = r#"
    let m = map_new()
    let first = [1]
    let equal_but_distinct = [1]
    let pointer = ptr(first, 0)
    let ignored1 = map_set(m, first, 10)
    let ignored2 = map_set(m, 7, 70)
    let ignored3 = map_set(m, "name", 20)
    let ignored4 = map_set(m, pointer, 30)
    print map_has(m, first)
    print map_has(m, equal_but_distinct)
    print map_get(m, "name")
    print map_del(m, 7)
    print map_get(m, "name")
    print map_get(m, pointer)
    "#;
    assert_parity(source, "1\n0\n20\n1\n20\n30\n");
}

fn assert_map_queries_at_thresholds(source: &str, expected: &[u8]) {
    let verified = tinylang::compile_source_verified(source).expect("map ownership fixture");
    let mut reference = Vec::new();
    tinylang::run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("VM map execution");
    assert_eq!(reference, expected);
    for threshold in [0, 1, 8] {
        let mut compiled = tinylang::JitProgram::compile_verified_with_options(
            &verified,
            tinylang::JitOptions::new().with_hot_back_edge_threshold(threshold),
        )
        .expect("map lowering");
        for run in 0..2 {
            let mut output = Vec::new();
            compiled.run(&mut output, Vec::new()).expect("JIT map execution");
            assert_eq!(output, reference, "threshold={threshold}, run={run}");
        }
    }
}

#[test]
fn map_string_queries_preserve_owned_index_and_original_key_identity() {
    let source = r#"
    let m = map_new()
    let original = str_concat("na", "me")
    let equal = str_concat("nam", "e")
    let inserted = map_set(m, original, 10)
    print map_get(m, equal)
    print map_set(m, equal, 20)
    print map_len(m)
    let before = map_keys(m)
    print ptr_addr(before[0]) == ptr_addr(original)
    let freed = unsafe free(original)
    let i = 0
    let total = 0
    while i < 24 {
      let updated = map_set(m, equal, i)
      total = total + map_get(m, equal)
      i = i + 1
    }
    print total
    print map_get(m, equal)
    print map_del(m, equal)
    print map_has(m, equal)
    let reinserted = map_set(m, equal, 77)
    let after = map_keys(m)
    print ptr_addr(after[0]) == ptr_addr(equal)
    let empty = ""
    let other_empty = str_concat("", "")
    let empty_inserted = map_set(m, empty, 99)
    print map_get(m, other_empty)
    let unicode_inserted = map_set(m, "héllø", 100)
    print map_get(m, str_concat("hé", "llø"))
    print map_len(m)
    "#;
    assert_map_queries_at_thresholds(source, b"10\n20\n1\ntrue\n276\n23\n1\n0\ntrue\n99\n100\n3\n");
}

#[test]
fn map_field_queries_preserve_cast_insensitive_identity_and_insertion_order() {
    let source = r#"
    struct Pair { left, right }
    let pair = Pair(4, 5)
    let right = fieldptr(pair, "right")
    let cast = cast_ptr(right, "u8")
    let left = fieldptr(pair, "left")
    let m = map_new()
    let inserted = map_set(m, right, 10)
    let other_inserted = map_set(m, left, 20)
    print map_set(m, cast, 30)
    print map_len(m)
    print map_get(m, right)
    print map_get(m, cast)
    print map_get(m, left)
    let before = map_keys(m)
    print ptr_type(before[0])
    let i = 0
    let total = 0
    while i < 24 {
      let updated = map_set(m, cast, i)
      total = total + map_get(m, right)
      i = i + 1
    }
    print total
    print map_del(m, cast)
    print map_get(m, left)
    let reinserted = map_set(m, cast, 40)
    let after = map_keys(m)
    print ptr_field(after[0])
    print ptr_type(after[1])
    print map_get(m, right)
    "#;
    assert_map_queries_at_thresholds(source, b"30\n2\n30\n30\n20\nfield\n276\n1\n20\nleft\nu8\n40\n");
}

#[test]
fn map_queries_preserve_stored_pointer_validation_and_error_precedence() {
    for operation in [
        "map_get(m, \"live\")",
        "map_has(m, \"live\")",
        "map_del(m, \"live\")",
        "map_set(m, \"live\", 21)",
        "map_set(m, \"new\", 22)",
        "map_get(m, 7)",
        "map_set(m, 7, 23)",
        "map_get(1, field)",
        "map_set(1, field, 9)",
    ] {
        let source = format!(
            "struct Pair {{ left, right }}\n\
             let pair = Pair(1, 2)\n\
             let field = fieldptr(pair, \"right\")\n\
             let m = map_new()\n\
             let inserted = map_set(m, field, 10)\n\
             let other_inserted = map_set(m, \"live\", 20)\n\
             let freed = unsafe free(pair)\n\
             let replacement = Pair(3, 4)\n\
             print 42\n\
             print {operation}\n"
        );
        let verified =
            tinylang::compile_source_verified_with_filename(&source, "map-queries.to").expect("map error fixture");
        let mut reference_output = Vec::new();
        let reference = tinylang::run_verified_program(&verified, "vm", &mut reference_output, Vec::new())
            .expect_err("stored/query pointer must be stale");
        assert!(matches!(reference, tinylang::TinyLangError::Runtime(_)));
        let message = reference.to_string();
        assert!(message.starts_with("map-queries.to:10:7:"), "{operation}: {message}");
        assert!(message.contains("Stale heap pointer"), "{operation}: {message}");
        assert_eq!(reference_output, b"42\n");
        for threshold in [0, 1, 8] {
            let mut compiled = tinylang::JitProgram::compile_verified_with_options(
                &verified,
                tinylang::JitOptions::new().with_hot_back_edge_threshold(threshold),
            )
            .expect("map error lowering");
            for run in 0..2 {
                let mut output = Vec::new();
                let error = compiled
                    .run(&mut output, Vec::new())
                    .expect_err("JIT must reject stale pointer");
                assert!(matches!(error, tinylang::TinyLangError::Runtime(_)));
                assert_eq!(error.to_string(), message, "{operation}: threshold={threshold}, run={run}");
                assert_eq!(output, reference_output);
            }
        }
    }
}

#[test]
fn io_capture_round_trips_writeln() {
    let source = r#"
    let captured = io_writeln(io_stdout(), "hi")
    let s = io_capture_stdout()
    print captured
    print str_byte_len(s)
    print str_char_len(s)
    "#;
    assert_parity(source, "3\n3\n3\n");
}

#[test]
fn io_and_string_helpers_match_backends() {
    let source = r#"
    let ignored = io_write(io_stderr(), "err")
    let ignored2 = io_flush(io_stdout())
    let captured = io_capture_stderr()
    print captured
    print io_stdin()
    print str_concat("left", "right")
    print i64(7)
    print fp64(1)
    "#;
    assert_parity(source, "err\n0\nleftright\n7\n1.0\n");
}

#[test]
fn typed_arithmetic_and_assert_helpers_match_backends() {
    let source = r#"
    print check_int_range(127, "i8")
    print typed_sub(9, 4, "i64")
    print typed_mul(6, 7, "i64")
    print typed_div(20, 5, "i64")
    print typed_neg(9, "i64")
    print assert(1, "must pass")
    "#;
    assert_parity(source, "127\n5\n42\n4\n-9\n1\n");
}

#[test]
fn io_read_line_consumes_the_same_input_in_both_backends() {
    let source = r"
    print io_read_line()
    ";
    let program = compile_source(source).expect("compile");
    for mode in ["vm", "jit"] {
        let mut output = Vec::new();
        run_program(Arc::clone(&program), mode, &mut output, vec!["from-input".to_string()]).expect("run");
        assert_eq!(String::from_utf8(output).unwrap(), "from-input\n");
    }
}

#[test]
fn string_byte_vs_char_indexing() {
    let source = r#"
    let text = "héllo"
    print str_byte_len(text)
    print str_char_len(text)
    print str_byte_at(text, 0)
    print str_char_at(text, 1)
    print str_slice(text, 1, 4)
    print str_is_utf8(text)
    "#;
    // 'h' is byte 104. é is multibyte. char_len = 5, byte_len = 6.
    assert_parity(source, "6\n5\n104\né\néll\n1\n");
}

#[test]
fn invalid_utf8_buffer_is_detected() {
    let source = r"
    let mem = buffer(2)
    let p = ptr(mem, 0)
    let ignored = unsafe write8(p, 255)
    let ignored2 = unsafe write8(unsafe ptr_add(p, 1), 254)
    print str_is_utf8(mem)
    ";
    assert_parity(source, "0\n");
}

#[test]
fn mutex_double_lock_errors() {
    let source = r#"
    let m = mutex_new()
    print mutex_lock(m)
    print mutex_unlock(m)
    print mutex_lock(m)
    print mutex_lock(m)
    "#;
    let mut vm = Vec::new();
    let err = run_program(compile_source(source).expect("compile"), "vm", &mut vm, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("deadlock"), "got: {err}");
}

#[test]
fn atomic_add_overflows_at_i64_max() {
    let source = r#"
    let a = atomic_new(9223372036854775807)
    print atomic_load(a)
    print atomic_add(a, 1)
    "#;
    let mut vm = Vec::new();
    let err = run_program(compile_source(source).expect("compile"), "vm", &mut vm, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("Memory_Overflow"), "got: {err}");
}

#[test]
fn result_round_trip_matches_backends() {
    let source = r#"
    let r = result_ok(42)
    print result_is_ok(r)
    print result_is_err(r)
    print result_unwrap(r)
    let e = result_err(99)
    print result_is_ok(e)
    print result_unwrap_err(e)
    "#;
    assert_parity(source, "1\n0\n42\n0\n99\n");
}

#[test]
fn option_unwrap_on_none_errors() {
    let source = r#"
    let s = option_some(7)
    print option_is_some(s)
    print option_unwrap(s)
    let n = option_none()
    print option_is_none(n)
    print option_unwrap(n)
    "#;
    let mut vm = Vec::new();
    let err = run_program(compile_source(source).expect("compile"), "vm", &mut vm, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("None"), "got: {err}");
}

#[test]
fn typed_add_overflows_outside_range() {
    let source = r#"
    print typed_add(100, 28, "u8")
    print typed_add(200, 100, "u8")
    "#;
    let mut vm = Vec::new();
    let err = run_program(compile_source(source).expect("compile"), "vm", &mut vm, Vec::new())
        .unwrap_err()
        .to_string();
    assert!(err.contains("Memory_Overflow"), "got: {err}");
}

#[test]
fn smallest_fit_and_promote_match_spec() {
    let source = r#"
    print smallest_fit(16)
    print smallest_fit(255)
    print smallest_fit(256)
    print smallest_fit(-16)
    print promote("i8", "u8")
    print promote("u8", "u16")
    print promote("i32", "i64")
    "#;
    assert_parity(source, "u8\nu8\nu16\ni8\ni16\nu32\ni64\n");
}

#[test]
fn type_of_recognizes_runtime_shapes() {
    let source = r#"
    print type_of(0)
    print type_of("abc")
    print type_of([1, 2])
    print type_of(alloc(1))
    print type_of(buffer(2))
    print type_of(map_new())
    print type_of(result_ok(1))
    print type_of(option_none())
    print type_of(mutex_new())
    print type_of(atomic_new(0))
    print type_of(null)
    "#;
    assert_parity(source, "i64\nString\nArray\nCell\nBuffer\nMap\nResult\nOption\nMutex\nAtomic\nNull\n");
}

#[test]
fn scaffold_types_are_constructible_and_round_trip() {
    let source = r#"
    fn add(a, b) { return a + b }
    let closure = closure_new("add", [10, 20])
    print type_of(closure)
    print closure_function(closure)
    print len(closure_captures(closure))

    let bare = sum_new(3)
    print type_of(bare)
    print sum_tag(bare)
    print sum_has_payload(bare)
    let carried = sum_new(4, 99)
    print sum_unwrap(carried)

    let union = tagged_union_new(7, "payload")
    print tagged_union_tag(union)
    print tagged_union_unwrap(union)

    let dyn_value = dyn_new(12, 34, 55)
    print dyn_type_id(dyn_value)
    print dyn_vtable_id(dyn_value)
    print dyn_unwrap(dyn_value)

    let boxed = box_new(8)
    print box_get(boxed)
    print box_set(boxed, 9)
    print box_get(boxed)
    print type_of(char_new(65))
    print type_of(unsafe fd_new(3))
    print type_of(char_buffer_new([65, 66]))
    struct Pair { left, right }
    let pair = Pair(1, 2)
    print type_of(record_new(pair))
    let map = map_new()
    let ignored = map_set(map, "key", 4)
    print type_of(dictionary_new(map))
    let raw = unsafe alloc_new("i32", buffer(4))
    print type_of(raw)
    "#;
    assert_parity(
        source,
        "Closure\n0\n2\nSum\n3\n0\n99\n7\npayload\n12\n34\n55\n8\n9\n9\nChar\nFileDescriptor\nCharBuffer\nRecord\nDictionary\nAlloc\n",
    );
}

#[test]
fn math_constants_have_documented_values() {
    let source = r#"
    print math_const("PI_THOUSANDTHS")
    print math_const("E_THOUSANDTHS")
    print math_const("TAU_THOUSANDTHS")
    print math_const("MAX_I64")
    print math_const("MIN_I64")
    print math_abs(-42)
    print math_min(1, 2)
    print math_max(1, 2)
    print logic_and(1, 1)
    print logic_and(1, 0)
    print logic_or(0, 1)
    print logic_not(0)
    print logic_xor(1, 0)
    "#;
    assert_parity(source, "3142\n2718\n6283\n9223372036854775807\n-9223372036854775808\n42\n1\n2\n1\n0\n1\n1\n1\n");
}

#[test]
fn sys_args_and_env_are_deterministic() {
    let source = r#"
    print sys_argc()
    print sys_argv(0)
    print sys_argv(1)
    print sys_env_has("FOO")
    print sys_env_get("FOO")
    print sys_env_has("MISSING")
    "#;
    let mut vm = Vec::new();
    let mut env = HashMap::new();
    env.insert("FOO".to_string(), "bar".to_string());
    tinylang::run_program_with_env(
        compile_source(source).expect("compile"),
        "vm",
        &mut vm,
        Vec::new(),
        vec!["program".to_string(), "alpha".to_string()],
        env,
    )
    .expect("run");
    let out = String::from_utf8(vm).unwrap();
    assert_eq!(out, "2\nprogram\nalpha\n1\nbar\n0\n");
}

#[test]
fn fs_read_write_round_trip_through_tempdir() {
    let dir = std::env::temp_dir().join(format!("tinylang-stdlib-parity-{}-{}", std::process::id(), rand_suffix()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let target = dir.join("hello.txt");
    let target_str = target.to_string_lossy().to_string();
    let source = format!(
        r#"
        let mem = buffer(4)
        let p = ptr(mem, 0)
        let ignored = unsafe write8(p, 104)
        let ignored2 = unsafe write8(unsafe ptr_add(p, 1), 105)
        let ignored3 = unsafe write8(unsafe ptr_add(p, 2), 33)
        let ignored4 = unsafe write8(unsafe ptr_add(p, 3), 10)
        let ignored5 = unsafe fs_write({path:?}, mem)
        let body = unsafe fs_read({path:?})
        print str_from_buffer(body)
        print fs_exists({path:?})
        let names = unsafe fs_list_dir({dir:?})
        print len(names)
        print path_basename({path:?})
        print path_dirname({path:?})
        print path_join("/tmp", "x")
        "#,
        path = target_str,
        dir = dir.to_string_lossy().to_string(),
    );
    let (vm, jit) = run_modes(&source);
    let expected = format!("hi!\n\n1\n1\nhello.txt\n{}\n/tmp/x\n", dir.to_string_lossy());
    assert_eq!(vm, expected, "vm output mismatch");
    assert_eq!(vm, jit);
    let _ = fs::remove_dir_all(&dir);
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}", d.as_nanos()))
        .unwrap_or_else(|_| "x".to_string())
}
