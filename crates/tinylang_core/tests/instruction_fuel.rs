use std::sync::Arc;

use tinylang::{Function, Instr, Op, Program, TinyMemory, VM};

fn vm(program: Program) -> VM {
    VM::new(Arc::new(program), TinyMemory::new(0), vec![]).unwrap()
}

#[test]
fn zero_fuel_and_exact_halt_budget() {
    let program = || Program::new(vec![Instr::new(Op::Halt, 0, 0)], 0);
    let mut output = Vec::new();
    let error = vm(program()).run_report_with_fuel(&mut output, 0).unwrap_err();
    assert!(error.to_string().contains("Instruction fuel exhausted"));
    assert!(output.is_empty());
    vm(program()).run_report_with_fuel(&mut output, 1).unwrap();
    vm(program()).run_report_with_fuel(&mut output, u64::MAX).unwrap();
    vm(program()).run_report(&mut output).unwrap();
}

#[test]
fn infinite_jump_stops_at_budget() {
    let program = Program::new(vec![Instr::new(Op::Jump, 0, 0), Instr::new(Op::Halt, 0, 0)], 0);
    let error = vm(program).run_report_with_fuel(&mut Vec::new(), 100).unwrap_err();
    assert!(error.to_string().contains("Instruction fuel exhausted"));
}

#[test]
fn print_is_not_executed_until_its_instruction_is_funded() {
    let program = || {
        Program::new(
            vec![
                Instr::new(Op::PushInt, 42, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        )
    };
    let mut output = Vec::new();
    assert!(vm(program()).run_report_with_fuel(&mut output, 1).is_err());
    assert!(output.is_empty());
    assert!(vm(program()).run_report_with_fuel(&mut output, 2).is_err());
    assert_eq!(output, b"42\n");
    output.clear();
    vm(program()).run_report_with_fuel(&mut output, 3).unwrap();
    assert_eq!(output, b"42\n");
}

#[test]
fn direct_and_indirect_calls_share_the_callers_budget() {
    let function = Function::new("answer", 0, vec![Instr::new(Op::PushInt, 42, 0), Instr::new(Op::Return, 0, 0)], 0);
    for (call, exact_fuel) in [
        (vec![Instr::new(Op::Call, 0, 0)], 5),
        (vec![Instr::new(Op::PushFunction, 0, 0), Instr::new(Op::CallValue, 0, 0)], 6),
    ] {
        let mut code = call;
        code.extend([Instr::new(Op::Print, 0, 0), Instr::new(Op::Halt, 0, 0)]);
        let program = Program::new(code, 0).with_functions(vec![function.clone()]);
        let mut output = Vec::new();
        let error = vm(program.clone())
            .run_report_with_fuel(&mut output, exact_fuel - 2)
            .unwrap_err();
        assert!(error.to_string().contains("Instruction fuel exhausted"));
        assert!(output.is_empty());
        vm(program).run_report_with_fuel(&mut output, exact_fuel).unwrap();
        assert_eq!(output, b"42\n");
    }
}
