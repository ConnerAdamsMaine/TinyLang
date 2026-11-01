use std::sync::Arc;
use tinyone::{Function, Instr, Op, Program, RuntimeValue, TinyMemory, VM};

fn session() -> VM {
    let program = Program::new(
        vec![
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ],
        1,
    )
    .with_functions(vec![Function::new(
        "read_global",
        0,
        vec![Instr::new(Op::LoadGlobal, 0, 0), Instr::new(Op::Return, 0, 0)],
        0,
    )]);
    let mut memory = TinyMemory::try_new(1).unwrap();
    memory.store(0, RuntimeValue::I64(0)).unwrap();
    VM::new(Arc::new(program), memory, vec![]).unwrap()
}

#[test]
fn successive_calls_retain_globals_and_receive_fresh_fuel() {
    let mut vm = session();
    for count in 1..=3 {
        vm.execute_main_with_fuel(&mut Vec::new(), 5).unwrap();
        assert_eq!(vm.memory().load(0).unwrap(), RuntimeValue::I64(count));
        assert_eq!(vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 2).unwrap(), RuntimeValue::I64(count));
    }
}

#[test]
fn trap_keeps_completed_writes_and_later_execution_works() {
    let mut vm = session();
    assert!(vm.execute_main_with_fuel(&mut Vec::new(), 4).is_err());
    assert_eq!(vm.memory().load(0).unwrap(), RuntimeValue::I64(1));
    assert!(vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 1).is_err());
    assert!(vm.invoke_function_with_fuel(99, vec![], &mut Vec::new(), 2).is_err());
    vm.execute_main_with_fuel(&mut Vec::new(), 5).unwrap();
    assert_eq!(vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 2).unwrap(), RuntimeValue::I64(2));
}

#[test]
fn heap_results_remain_live_for_later_calls_in_the_same_vm() {
    let create = Function::new(
        "create",
        0,
        vec![
            Instr::new(Op::PushInt, 42, 0),
            Instr::new(Op::MakeArray, 1, 0),
            Instr::new(Op::Return, 0, 0),
        ],
        0,
    );
    let read = Function::new(
        "read",
        1,
        vec![
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Index, 0, 0),
            Instr::new(Op::Return, 0, 0),
        ],
        1,
    );
    let program = Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![create, read]);
    let mut vm = VM::new(Arc::new(program), TinyMemory::new(0), vec![]).unwrap();
    let value = vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 3).unwrap();
    vm.execute_main_with_fuel(&mut Vec::new(), 1).unwrap();
    assert_eq!(
        vm.invoke_function_with_fuel(1, vec![value], &mut Vec::new(), 4)
            .unwrap(),
        RuntimeValue::I64(42)
    );
}

#[test]
fn per_session_heap_limits_apply_to_repeated_allocations() {
    let create = Function::new(
        "create",
        0,
        vec![
            Instr::new(Op::PushInt, 42, 0),
            Instr::new(Op::MakeArray, 1, 0),
            Instr::new(Op::Return, 0, 0),
        ],
        0,
    );
    let program = Arc::new(Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![create]));
    let limits = tinyone::VmExecutionLimits::new(1024, 1, 4).unwrap();
    let mut vm = VM::new_with_limits(Arc::clone(&program), TinyMemory::new(0), vec![], limits).unwrap();
    vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 3).unwrap();
    let error = vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 3).unwrap_err();
    assert!(error.to_string().contains("Heap object limit 1"));
    let limits = tinyone::VmExecutionLimits::new(1, 10, 4).unwrap();
    let mut vm = VM::new_with_limits(program, TinyMemory::new(0), vec![], limits).unwrap();
    assert!(
        vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 3)
            .unwrap_err()
            .to_string()
            .contains("Heap byte limit 1")
    );
}

#[test]
fn per_session_call_depth_restores_after_trap() {
    let outer = Function::new("outer", 0, vec![Instr::new(Op::Call, 1, 0), Instr::new(Op::Return, 0, 0)], 0);
    let inner = Function::new("inner", 0, vec![Instr::new(Op::PushInt, 42, 0), Instr::new(Op::Return, 0, 0)], 0);
    let program = Arc::new(Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0).with_functions(vec![outer, inner]));
    let limits = tinyone::VmExecutionLimits::new(1024, 10, 1).unwrap();
    let mut vm = VM::new_with_limits(program, TinyMemory::new(0), vec![], limits).unwrap();
    assert!(
        vm.invoke_function_with_fuel(0, vec![], &mut Vec::new(), 10)
            .unwrap_err()
            .to_string()
            .contains("Call stack overflow after 1")
    );
    assert_eq!(vm.invoke_function_with_fuel(1, vec![], &mut Vec::new(), 2).unwrap(), RuntimeValue::I64(42));
    assert!(tinyone::VmExecutionLimits::new(0, 1, 1).is_err());
    assert!(tinyone::VmExecutionLimits::new(1, 0, 1).is_err());
    assert!(tinyone::VmExecutionLimits::new(1, 1, 17).is_err());
}

#[test]
fn heap_growth_checks_the_session_budget_before_mutating() {
    let program = tinyone::compile_source(
        "fn make() { return [42] } fn grow(a) { push(a, 1) return a } fn count(a) { return len(a) }",
    )
    .unwrap();
    let function = |name: &str| {
        program
            .functions()
            .iter()
            .position(|entry| entry.name() == name)
            .unwrap()
    };
    let (make, grow, count) = (function("make"), function("grow"), function("count"));
    let limits = tinyone::VmExecutionLimits::new(std::mem::size_of::<RuntimeValue>(), 10, 4).unwrap();
    let memory = TinyMemory::try_new(program.slot_count()).unwrap();
    let mut vm = VM::new_with_limits(program, memory, vec![], limits).unwrap();
    let value = vm
        .invoke_function_with_fuel(make, vec![], &mut Vec::new(), 100)
        .unwrap();
    let error = vm
        .invoke_function_with_fuel(grow, vec![value.clone()], &mut Vec::new(), 100)
        .unwrap_err();
    assert!(error.to_string().contains("Heap byte limit"));
    assert_eq!(
        vm.invoke_function_with_fuel(count, vec![value], &mut Vec::new(), 100)
            .unwrap(),
        RuntimeValue::I64(1)
    );
}
