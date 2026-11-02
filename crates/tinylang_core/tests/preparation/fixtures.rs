//! Raw verified lowering workloads shared by timing and allocation diagnostics.
//! They bypass frontend folding so large action streams actually reach the JIT.

use tinylang::{Instr, Op, Program, VerifiedProgram};

#[derive(Clone, Copy)]
pub(super) enum PreparationFixture {
    ExpressionChain(usize),
    StoredExpressionChain(usize),
    DeadStores(usize),
    CopyStores(usize),
}

pub(super) const LARGE_CASES: [(&str, PreparationFixture, u64); 5] = [
    ("dead_stores_1024", PreparationFixture::DeadStores(1_024), 500),
    ("dead_stores_65536", PreparationFixture::DeadStores(65_536), 10),
    ("copy_stores_32768", PreparationFixture::CopyStores(32_768), 10),
    ("expression_chain_65536", PreparationFixture::ExpressionChain(65_536), 10),
    ("stored_expression_chain_65536", PreparationFixture::StoredExpressionChain(65_536), 10),
];

impl PreparationFixture {
    pub(super) fn prepare(self) -> (VerifiedProgram, String) {
        let (code, slots, output) = match self {
            Self::ExpressionChain(depth) => (expression_chain(depth, &[Op::Print, Op::Halt]), 1, "0\n".to_owned()),
            Self::StoredExpressionChain(depth) => {
                (expression_chain(depth, &[Op::Store, Op::Load, Op::Print, Op::Halt]), 1, "0\n".to_owned())
            }
            Self::DeadStores(count) => {
                assert!(count > 0, "the dead-store fixture needs a surviving write");
                let mut code = Vec::with_capacity(2 * count + 3);
                for value in 0..count {
                    code.push(Instr::new(Op::PushInt, i64::try_from(value).expect("fixture literal"), 0));
                    code.push(Instr::new(Op::Store, 0, 0));
                }
                code.push(Instr::new(Op::Load, 0, 0));
                code.push(Instr::new(Op::Print, 0, 0));
                code.push(Instr::new(Op::Halt, 0, 0));
                (code, 1, format!("{}\n", count - 1))
            }
            Self::CopyStores(count) => {
                let mut code = Vec::with_capacity(4 * count + 3);
                for _ in 0..count {
                    code.extend([
                        Instr::new(Op::Load, 0, 0),
                        Instr::new(Op::Store, 1, 0),
                        Instr::new(Op::Load, 1, 0),
                        Instr::new(Op::Store, 0, 0),
                    ]);
                }
                code.push(Instr::new(Op::Load, 0, 0));
                code.push(Instr::new(Op::Print, 0, 0));
                code.push(Instr::new(Op::Halt, 0, 0));
                (code, 2, "0\n".to_owned())
            }
        };
        (VerifiedProgram::verify(Program::new(code, slots)).expect("bounded lowering fixture"), output)
    }
}

fn expression_chain(depth: usize, ending: &[Op]) -> Vec<Instr> {
    let mut code = Vec::with_capacity(2 * depth + 1 + ending.len());
    code.push(Instr::new(Op::PushInt, 0, 0));
    for _ in 0..depth {
        code.push(Instr::new(Op::Load, 0, 0));
        code.push(Instr::new(Op::Add, 0, 0));
    }
    code.extend(ending.iter().map(|&op| Instr::new(op, 0, 0)));
    code
}
