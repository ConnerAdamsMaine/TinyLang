use tinylang::{RuntimeValue, VM, VerifiedProgram, VmExecutionLimits, VmHeapValue};

fn session(limits: VmExecutionLimits) -> VM {
    let program = tinylang::compile_source("fn identity(value) { return value }").unwrap();
    let program = VerifiedProgram::from_binary_artifact(&program.to_binary_artifact().unwrap()).unwrap();
    VM::from_verified_with_limits(&program, vec![], limits).unwrap()
}

#[test]
fn heap_arguments_and_results_round_trip_through_compiled_function() {
    let mut vm = session(VmExecutionLimits::default());
    for input in [
        VmHeapValue::String("snowman ☃\0tail".into()),
        VmHeapValue::Bytes(vec![0, 0xff, 0x80, 42]),
        VmHeapValue::Array(vec![RuntimeValue::I64(i64::MIN), RuntimeValue::I64(42)]),
        VmHeapValue::String(String::new()),
        VmHeapValue::Bytes(vec![]),
        VmHeapValue::Array(vec![]),
    ] {
        let value = vm.import_heap_value(input.clone()).unwrap();
        let result = vm
            .invoke_function_with_fuel(0, vec![value.clone()], &mut Vec::new(), 10)
            .unwrap();
        assert_eq!(vm.copy_heap_value(&result).unwrap(), input);
        vm.release_heap_value(&value).unwrap();
        assert!(vm.copy_heap_value(&result).is_err());
        assert!(vm.release_heap_value(&value).is_err());
    }
}

#[test]
fn nested_references_are_shallow_and_stale_children_are_rejected() {
    let mut vm = session(VmExecutionLimits::default());
    let child = vm.import_heap_value(VmHeapValue::String("child".into())).unwrap();
    let array = vm.import_heap_value(VmHeapValue::Array(vec![child.clone()])).unwrap();
    assert_eq!(vm.copy_heap_value(&array).unwrap(), VmHeapValue::Array(vec![child.clone()]));
    vm.release_heap_value(&array).unwrap();
    assert_eq!(vm.copy_heap_value(&child).unwrap(), VmHeapValue::String("child".into()));
    vm.release_heap_value(&child).unwrap();
    assert!(vm.import_heap_value(VmHeapValue::Array(vec![child])).is_err());
    assert!(vm.copy_heap_value(&RuntimeValue::I64(42)).is_err());
}

#[test]
fn imports_enforce_heap_limits_and_failed_imports_leave_capacity_available() {
    let mut vm = session(VmExecutionLimits::new(3, 1, 4).unwrap());
    assert!(vm.import_heap_value(VmHeapValue::Bytes(vec![0; 4])).is_err());
    let first = vm.import_heap_value(VmHeapValue::String("abc".into())).unwrap();
    assert!(vm.import_heap_value(VmHeapValue::Bytes(vec![])).is_err());
    vm.release_heap_value(&first).unwrap();
    let next = vm.import_heap_value(VmHeapValue::Bytes(vec![1, 2, 3])).unwrap();
    assert!(vm.copy_heap_value(&first).is_err());
    assert_eq!(vm.copy_heap_value(&next).unwrap(), VmHeapValue::Bytes(vec![1, 2, 3]));
}
