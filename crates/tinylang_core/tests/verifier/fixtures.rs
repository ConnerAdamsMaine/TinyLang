//! Raw metadata scaling fixtures shared by verifier diagnostics and timings.
//! These do not represent general large-program execution throughput.

use tinylang::{Function, Instr, Op, Program, StructDef, VerifiedProgram, run_verified_program};

#[derive(Clone, Copy, Debug)]
pub(super) enum VerifierFixture {
    Functions(usize),
    Names(usize),
    StructFields(usize),
}

impl VerifierFixture {
    pub(super) fn prepare(self) -> VerifiedProgram {
        let program = Program::new(
            vec![
                Instr::new(Op::PushInt, 42, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
            0,
        );
        let program = match self {
            Self::Functions(count) => program.with_functions(
                (0..count)
                    .map(|index| {
                        Function::new(
                            format!("sample_{index}"),
                            0,
                            vec![Instr::new(Op::PushInt, 0, 0), Instr::new(Op::Return, 0, 0)],
                            0,
                        )
                    })
                    .collect(),
            ),
            Self::Names(count) => program
                .with_slot_count(count)
                .with_names((0..count).map(|index| format!("slot_{index}")).collect()),
            Self::StructFields(count) => program.with_structs(vec![StructDef::new(
                "Record",
                (0..count).map(|index| format!("field_{index}")).collect(),
            )]),
        };
        VerifiedProgram::verify(program).expect("valid verifier metadata fixture")
    }
}

pub(super) fn check_output(verified: &VerifiedProgram) {
    for mode in ["vm", "jit"] {
        let mut output = Vec::new();
        drop(run_verified_program(verified, mode, &mut output, Vec::new()).expect("verifier fixture execution"));
        assert_eq!(output, b"42\n", "{mode}");
    }
}
