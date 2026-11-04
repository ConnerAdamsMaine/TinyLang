use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use tinylang::{Instr, Op, Program, RuntimeValue, TinyMemory, VM};

fn main() {
    const LIMIT: i64 = 100_000;
    let program = Arc::new(Program::new(
        vec![
            Instr::new(Op::PushInt, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::PushInt, LIMIT, 0),
            Instr::new(Op::Lt, 0, 0),
            Instr::new(Op::JumpIfZero, 11, 0),
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Halt, 0, 0),
        ],
        1,
    ));
    println!("mode,sample,instructions,elapsed_ns");
    for bounded in [false, true] {
        for sample in 0..8 {
            let vm = VM::new(Arc::clone(&program), TinyMemory::try_new(1).unwrap(), vec![]).unwrap();
            let mut output = Vec::new();
            let instructions = 9 * LIMIT as u64 + 2;
            let (report, elapsed) = if bounded {
                let started = Instant::now();
                let report = vm.run_report_with_fuel(&mut output, black_box(instructions)).unwrap();
                (report, started.elapsed())
            } else {
                let started = Instant::now();
                let report = vm.run_report(&mut output).unwrap();
                (report, started.elapsed())
            };
            assert_eq!(report.memory.load(0).unwrap(), RuntimeValue::I64(LIMIT));
            assert!(output.is_empty());
            if sample != 0 {
                println!("{bounded},{sample},{instructions},{}", elapsed.as_nanos());
            }
            black_box(report);
        }
    }
}
