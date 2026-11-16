use std::collections::HashSet;
use std::sync::Arc;

use crate::source::bytecode::BytecodeSource;
use crate::{Function, Instr, Op, Program, floor_div};

pub struct PeepholeOptimizer;

impl PeepholeOptimizer {
    #[must_use]
    pub fn optimize(mut program: Program) -> Program {
        Program {
            code: Self::optimize_code(program.code, &mut program.source),
            functions: program
                .functions
                .into_iter()
                .map(|mut function| Function {
                    code: Self::optimize_code(function.code, &mut function.source),
                    ..function
                })
                .collect(),
            ..program
        }
    }

    fn optimize_code(mut code: Vec<Instr>, source: &mut Option<Arc<BytecodeSource>>) -> Vec<Instr> {
        while let Some(length) = optimize_pass(&mut code, source.as_mut()) {
            code.truncate(length);
        }
        if let Some(source) = source
            && source.instruction_count() != code.len()
        {
            Arc::make_mut(source).truncate(code.len());
        }
        // Keep the former copying pass's compact output, but shrink only once
        // after all folds. An already-compact unchanged chunk keeps its buffer.
        code.into_boxed_slice().into_vec()
    }
}

#[derive(Clone, Copy)]
struct ConstantFold {
    instruction: Instr,
    width: usize,
}

impl ConstantFold {
    fn at(code: &[Instr]) -> Option<Self> {
        let [first, second, rest @ ..] = code else {
            return None;
        };
        if first.op != Op::PushInt {
            return None;
        }
        match second.op {
            Op::Neg => Some(Self {
                instruction: Instr::new(Op::PushInt, first.arg.checked_neg()?, 0),
                width: 2,
            }),
            Op::PushInt => Some(Self {
                instruction: fold_binop(rest.first()?.op, first.arg, second.arg)?,
                width: 3,
            }),
            _ => None,
        }
    }

    fn preserves_targets(self, start: usize, targets: &HashSet<usize>) -> bool {
        (start + 1..start + self.width).all(|offset| !targets.contains(&offset))
    }
}

/// Optimize one pass while retaining a total mapping from old instruction
/// offsets to new offsets. A fold may begin at a branch target, but it may not
/// consume a target in its interior. This lets straight-line regions inside
/// functions containing loops and conditionals benefit from constant folding.
/// Returns the initialized output prefix only when a fold shortened the chunk.
fn optimize_pass(code: &mut [Instr], source: Option<&mut Arc<BytecodeSource>>) -> Option<usize> {
    // Ordinary chunks often contain no foldable constants. Find a candidate
    // before allocating even the branch-target set, let alone a remap table.
    let first_candidate = (0..code.len()).find(|&start| ConstantFold::at(&code[start..]).is_some())?;
    let branch_targets: HashSet<usize> = code
        .iter()
        .filter_map(|instr| match instr.op {
            Op::Jump | Op::JumpIfZero => usize::try_from(instr.arg).ok(),
            _ => None,
        })
        .collect();
    let (start, first) = (first_candidate..code.len()).find_map(|start| {
        ConstantFold::at(&code[start..])
            .filter(|fold| fold.preserves_targets(start, &branch_targets))
            .map(|fold| (start, fold))
    })?;
    // Prefix offsets remain identical. All suffix offsets, including the
    // original end, receive their compacted positions below.
    let mut old_to_new: Vec<usize> = (0..=code.len()).collect();
    old_to_new[start..start + first.width].fill(start);
    code[start] = first.instruction;
    let mut read = start + first.width;
    let mut write = start + 1;

    while read < code.len() {
        let fold = ConstantFold::at(&code[read..]).filter(|fold| fold.preserves_targets(read, &branch_targets));
        let (instruction, width) = fold.map_or((code[read], 1), |fold| (fold.instruction, fold.width));
        old_to_new[read..read + width].fill(write);
        // write < read after the first fold, so this cannot overwrite any
        // unread source instruction. Instr is Copy and owns no runtime values.
        code[write] = instruction;
        read += width;
        write += 1;
    }
    old_to_new[code.len()] = write;

    if let Some(source) = source {
        Arc::make_mut(source).remap(&old_to_new[..code.len()]);
    }

    for instr in &mut code[..write] {
        if matches!(instr.op, Op::Jump | Op::JumpIfZero)
            && let Ok(target) = usize::try_from(instr.arg)
            && let Some(mapped) = old_to_new.get(target)
        {
            instr.arg = *mapped as i64;
        }
    }
    Some(write)
}

/// Folds a constant binary op into a single push instruction. Comparisons
/// fold to `PUSH_BOOL`, not `PUSH_INT` — the unoptimized `Lt`/`Lte`/`Gt`/
/// `Gte`/`Eq`/`Ne` opcodes all produce `Value::Bool` at runtime
/// (`runtime_compare`), so a constant-folded comparison must match: `1 < 2`
/// and `let a = 1; let b = 2; a < b` need to print identically regardless of
/// whether this optimizer fires.
fn fold_binop(op: Op, a: i64, b: i64) -> Option<Instr> {
    Some(match op {
        Op::Add => Instr::new(Op::PushInt, a.checked_add(b)?, 0),
        Op::Sub => Instr::new(Op::PushInt, a.checked_sub(b)?, 0),
        Op::Mul => Instr::new(Op::PushInt, a.checked_mul(b)?, 0),
        Op::Div if b != 0 => Instr::new(Op::PushInt, floor_div(a, b)?, 0),
        Op::Lt => Instr::new(Op::PushBool, i64::from(a < b), 0),
        Op::Lte => Instr::new(Op::PushBool, i64::from(a <= b), 0),
        Op::Gt => Instr::new(Op::PushBool, i64::from(a > b), 0),
        Op::Gte => Instr::new(Op::PushBool, i64::from(a >= b), 0),
        Op::Eq => Instr::new(Op::PushBool, i64::from(a == b), 0),
        Op::Ne => Instr::new(Op::PushBool, i64::from(a != b), 0),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_chains_shrink_in_place_without_replacing_the_instruction_buffer() {
        for terms in [3, 32, 128] {
            let mut code = Vec::with_capacity(2 * terms + 1);
            code.push(Instr::new(Op::PushInt, 1, 0));
            for _ in 1..terms {
                code.extend([Instr::new(Op::PushInt, 1, 0), Instr::new(Op::Add, 0, 0)]);
            }
            code.extend([Instr::new(Op::Print, 0, 0), Instr::new(Op::Halt, 0, 0)]);
            let pointer = code.as_ptr();
            let capacity = code.capacity();
            while let Some(length) = optimize_pass(&mut code, None) {
                assert!(length < code.len(), "each successful pass shrinks the chunk");
                assert_eq!(code.as_ptr(), pointer);
                assert_eq!(code.capacity(), capacity);
                code.truncate(length);
            }
            assert_eq!(code.len(), 3);
            assert_eq!(code[0], Instr::new(Op::PushInt, i64::try_from(terms).expect("bounded terms"), 0));
            assert_eq!(optimize_pass(&mut code, None), None);
            crate::BytecodeVerifier::verify(&Program::new(code, 0)).expect("folded chain");
        }
    }

    #[test]
    fn constant_chain_folding_matches_the_reference_at_every_branch_boundary() {
        for op in [
            Op::Add,
            Op::Sub,
            Op::Mul,
            Op::Div,
            Op::Lt,
            Op::Lte,
            Op::Gt,
            Op::Gte,
            Op::Eq,
            Op::Ne,
        ] {
            for (lhs, rhs) in [(4, 3), (-7, 3), (i64::MAX, 1), (i64::MIN, -1), (i64::MIN, 0), (0, 0)] {
                let template = [
                    Instr::new(Op::PushInt, lhs, 71),
                    Instr::new(Op::PushInt, rhs, 72),
                    Instr::new(Op::Add, 73, 74),
                    Instr::new(Op::PushInt, 2, 75),
                    Instr::new(op, 76, 77),
                    Instr::new(Op::Neg, 78, 79),
                    Instr::new(Op::PushInt, 3, 80),
                    Instr::new(Op::Sub, 81, 82),
                    Instr::new(Op::Print, 83, 84),
                    Instr::new(Op::Halt, 85, 86),
                ];
                for branch in [Op::Jump, Op::JumpIfZero] {
                    for position in 0..=template.len() {
                        let last_target = i64::try_from(template.len() + 2).expect("small fixture");
                        for target in (-1..=last_target).chain([i64::MIN, i64::MAX]) {
                            let mut code = template.to_vec();
                            code.insert(position, Instr::new(branch, target, 87));
                            let expected = copying_reference(code.clone());
                            assert_eq!(
                                PeepholeOptimizer::optimize_code(code, &mut None),
                                expected,
                                "{op:?} after {lhs}+{rhs}, {branch:?} at {position} targeting {target}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn constant_chain_origins_keep_the_last_operator_and_shared_source_owners() {
        let text =
            String::from("fn untouched(value) { return value } let divisor = 0 print (1 + 2 + 3 + 4 + 5) / divisor");
        let original =
            crate::compile_source_unoptimized_verified_with_filename(&text, "chain-origin.to").expect("source fixture");
        let root = original.program().source.as_ref().expect("original origins");
        let before = root.offsets().to_vec();
        let expected_code = copying_reference(original.program().code.clone());
        let optimized = PeepholeOptimizer::optimize(original.program().clone());
        assert_eq!(optimized.code, expected_code);
        assert_eq!(root.offsets(), before);
        let origins = optimized.source.as_ref().expect("optimized origins");
        assert!(!Arc::ptr_eq(root, origins), "changed origins use copy-on-write");
        assert!(Arc::ptr_eq(root.source_owner(), origins.source_owner()));
        assert_eq!(origins.instruction_count(), optimized.code.len());
        let folded = optimized
            .code
            .iter()
            .position(|instruction| *instruction == Instr::new(Op::PushInt, 15, 0))
            .expect("folded chain");
        assert_eq!(origins.offsets()[folded].0, text.rfind('+').expect("last folded operator"));
        assert!(Arc::ptr_eq(
            original.program().functions[0]
                .source
                .as_ref()
                .expect("original function"),
            optimized.functions[0].source.as_ref().expect("unchanged function"),
        ));
        let optimized = crate::VerifiedProgram::verify(optimized).expect("optimized verification");
        drop(text);
        let mut expected_output = Vec::new();
        let expected = crate::run_verified_program(&original, "vm", &mut expected_output, Vec::new())
            .expect_err("division by zero")
            .to_string();
        assert!(expected.contains("chain-origin.to"));
        let mut output = Vec::new();
        let actual = crate::run_verified_program(&optimized, "vm", &mut output, Vec::new())
            .expect_err("optimized division by zero")
            .to_string();
        assert_eq!(actual, expected);
        assert_eq!(output, expected_output);
        for threshold in [0, 1, 8] {
            let mut compiled = crate::JitProgram::compile_verified_with_options(
                &optimized,
                crate::JitOptions::new().with_hot_back_edge_threshold(threshold),
            )
            .expect("lower chain");
            for _ in 0..2 {
                output.clear();
                let actual = compiled.run(&mut output, Vec::new()).expect_err("JIT division by zero");
                assert_eq!(actual.to_string(), expected);
                assert_eq!(output, expected_output);
            }
        }
    }
    #[test]
    fn unchanged_owned_chunks_keep_their_compact_allocations() {
        let code = vec![
            Instr::new(Op::Call, 0, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let function =
            Function::new("answer", 0, vec![Instr::new(Op::PushInt, 42, 0), Instr::new(Op::Return, 0, 0)], 0);
        let main_pointer = code.as_ptr();
        let function_pointer = function.code.as_ptr();
        let program = Program::new(code, 0).with_functions(vec![function]);
        let optimized = PeepholeOptimizer::optimize(program);
        assert_eq!(optimized.code.as_ptr(), main_pointer);
        assert_eq!(optimized.functions[0].code.as_ptr(), function_pointer);
        crate::BytecodeVerifier::verify(&optimized).expect("unchanged program");
    }

    #[test]
    fn optimization_does_not_retain_input_slack_or_folded_away_storage() {
        for instructions in [
            Vec::new(),
            vec![Instr::new(Op::Halt, 0, 0)],
            vec![
                Instr::new(Op::PushInt, 2, 0),
                Instr::new(Op::PushInt, 3, 0),
                Instr::new(Op::Add, 0, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Halt, 0, 0),
            ],
        ] {
            let mut code = Vec::with_capacity(4096);
            code.extend(instructions);
            let optimized = PeepholeOptimizer::optimize(Program::new(code, 0));
            assert_eq!(optimized.code.capacity(), optimized.code.len());
        }
    }

    #[test]
    fn in_place_pass_preserves_its_buffer_and_skips_an_unchanged_prefix() {
        let mut code = vec![
            Instr::new(Op::PushInt, 7, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::PushInt, 2, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Print, 0, 0),
            Instr::new(Op::Jump, 2, 0),
            Instr::new(Op::Halt, 0, 0),
        ];
        let pointer = code.as_ptr();
        let capacity = code.capacity();
        let length = optimize_pass(&mut code, None).expect("one fold");
        assert_eq!(length, 6);
        assert_eq!(code.as_ptr(), pointer);
        assert_eq!(code.capacity(), capacity);
        code.truncate(length);
        assert_eq!(
            code,
            [
                Instr::new(Op::PushInt, 7, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::PushInt, 5, 0),
                Instr::new(Op::Print, 0, 0),
                Instr::new(Op::Jump, 2, 0),
                Instr::new(Op::Halt, 0, 0),
            ]
        );
        assert_eq!(optimize_pass(&mut code, None), None);
    }

    #[test]
    fn in_place_folding_matches_copying_passes_at_every_branch_boundary() {
        for op in [
            Op::Add,
            Op::Sub,
            Op::Mul,
            Op::Div,
            Op::Lt,
            Op::Lte,
            Op::Gt,
            Op::Gte,
            Op::Eq,
            Op::Ne,
        ] {
            for (lhs, rhs) in [(2, 3), (-7, 3), (i64::MAX, 1), (i64::MIN, -1), (i64::MIN, 0), (0, 0)] {
                let template = [
                    Instr::new(Op::PushInt, lhs, 71),
                    Instr::new(Op::PushInt, rhs, 72),
                    Instr::new(op, 73, 74),
                    Instr::new(Op::PushInt, -3, 0),
                    Instr::new(Op::Neg, 0, 0),
                    Instr::new(Op::Mul, 0, 0),
                    Instr::new(Op::Print, 0, 0),
                    Instr::new(Op::PushInt, i64::MIN, 0),
                    Instr::new(Op::Neg, 0, 0),
                    Instr::new(Op::Halt, 0, 0),
                ];
                for branch in [Op::Jump, Op::JumpIfZero] {
                    for position in 0..=template.len() {
                        let last_target = i64::try_from(template.len() + 2).expect("small fixture");
                        for target in (-1..=last_target).chain([i64::MIN, i64::MAX]) {
                            let mut code = template.to_vec();
                            code.insert(position, Instr::new(branch, target, 75));
                            let expected = copying_reference(code.clone());
                            assert_eq!(
                                PeepholeOptimizer::optimize_code(code, &mut None),
                                expected,
                                "{op:?}({lhs}, {rhs}), {branch:?} at {position} targeting {target}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_jump_operands_are_not_repaired_by_folding() {
        for branch in [Op::Jump, Op::JumpIfZero] {
            for target in [i64::MIN, -1, 5, 6, i64::MAX] {
                let program = Program::new(
                    vec![
                        Instr::new(Op::PushInt, 2, 0),
                        Instr::new(Op::PushInt, 3, 0),
                        Instr::new(Op::Add, 0, 0),
                        Instr::new(branch, target, 0),
                        Instr::new(Op::Halt, 0, 0),
                    ],
                    0,
                );
                let optimized = PeepholeOptimizer::optimize(program);
                assert_eq!(optimized.code.len(), 3);
                assert_eq!(optimized.code[1].arg, if target == 5 { 3 } else { target });
                crate::BytecodeVerifier::verify(&optimized).expect_err("invalid target must remain invalid");
            }
        }
    }

    #[test]
    fn folding_preserves_vm_results_and_error_order_in_every_tier() {
        for source in [
            "print (2 + 3) * 4 print 1 < 2 print -7 / 3",
            "fn answer() { return (2 + 3) * -(4 - 10) } print answer()",
            "print 7 let failure = 9223372036854775807 + 1 print 8",
            "print 7 let failure = 9223372036854775807 * 2 print 8",
            "print 7 let failure = (-9223372036854775807 - 1) / -1 print 8",
            "print 7 let failure = 2 / 0 print 8",
            "print 7 let failure = -(-9223372036854775807 - 1) print 8",
            "print 7 let failure = 0.0 / 0.0 print 8",
            "print (1 + 2 + 3) / -2 print (1 + 2 + 3) < 7",
            "print (1 + 2 + 3) + 0.25",
            "print 7 let failure = (1 + 2 + 3) / 0 print 8",
            "print 7 let failure = 9223372036854775805 + 1 + 1 + 1 print 8",
            "print 7 let failure = -((1 + 2 + 3) < 7) print 8",
        ] {
            let raw = crate::compile_source_unoptimized_verified(source).expect("unoptimized fixture");
            let mut expected_output = Vec::new();
            let expected = crate::run_verified_program(&raw, "vm", &mut expected_output, Vec::new())
                .map(|_| ())
                .map_err(|error| error.to_string());
            let optimized = crate::VerifiedProgram::verify(PeepholeOptimizer::optimize(raw.program().clone()))
                .expect("folded fixture");
            let mut output = Vec::new();
            let result = crate::run_verified_program(&optimized, "vm", &mut output, Vec::new())
                .map(|_| ())
                .map_err(|error| error.to_string());
            assert_eq!(result, expected, "optimized VM: {source}");
            assert_eq!(output, expected_output, "optimized VM: {source}");
            for threshold in [0, 1, 8] {
                let mut compiled = crate::JitProgram::compile_verified_with_options(
                    &optimized,
                    crate::JitOptions::new().with_hot_back_edge_threshold(threshold),
                )
                .expect("lower folded fixture");
                output.clear();
                let result = compiled
                    .run(&mut output, Vec::new())
                    .map(|_| ())
                    .map_err(|error| error.to_string());
                assert_eq!(result, expected, "JIT threshold={threshold}: {source}");
                assert_eq!(output, expected_output, "JIT threshold={threshold}: {source}");
            }
        }
    }

    // Independent copying model of the former pass: no in-place writes,
    // first-fold shortcut or ConstantFold matcher. Include malformed bytecode
    // in comparisons because optimization does not require a verified token.
    fn copying_reference(mut code: Vec<Instr>) -> Vec<Instr> {
        loop {
            let targets: HashSet<usize> = code
                .iter()
                .filter(|instr| matches!(instr.op, Op::Jump | Op::JumpIfZero))
                .filter_map(|instr| usize::try_from(instr.arg).ok())
                .collect();
            let mut mapping = vec![0usize; code.len() + 1];
            let mut out = Vec::with_capacity(code.len());
            let mut read = 0;
            while read < code.len() {
                mapping[read] = out.len();
                if read + 1 < code.len()
                    && code[read].op == Op::PushInt
                    && code[read + 1].op == Op::Neg
                    && !targets.contains(&(read + 1))
                    && let Some(value) = code[read].arg.checked_neg()
                {
                    mapping[read + 1] = out.len();
                    out.push(Instr::new(Op::PushInt, value, 0));
                    read += 2;
                    continue;
                }
                if read + 2 < code.len()
                    && code[read].op == Op::PushInt
                    && code[read + 1].op == Op::PushInt
                    && !targets.contains(&(read + 1))
                    && !targets.contains(&(read + 2))
                    && let Some(folded) = fold_binop(code[read + 2].op, code[read].arg, code[read + 1].arg)
                {
                    mapping[read + 1] = out.len();
                    mapping[read + 2] = out.len();
                    out.push(folded);
                    read += 3;
                    continue;
                }
                out.push(code[read]);
                read += 1;
            }
            if out.len() == code.len() {
                return out;
            }
            mapping[code.len()] = out.len();
            for instr in &mut out {
                if matches!(instr.op, Op::Jump | Op::JumpIfZero)
                    && let Ok(target) = usize::try_from(instr.arg)
                    && let Some(mapped) = mapping.get(target)
                {
                    instr.arg = *mapped as i64;
                }
            }
            code = out;
        }
    }

    #[test]
    fn folds_constants_inside_control_flow_and_remaps_targets() {
        let code = vec![
            Instr::new(Op::PushInt, 2, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Store, 0, 0),
            Instr::new(Op::Load, 0, 0),
            Instr::new(Op::JumpIfZero, 8, 0),
            Instr::new(Op::PushInt, 1, 0),
            Instr::new(Op::Jump, 4, 0),
            Instr::new(Op::Halt, 0, 0),
        ];

        let optimized = PeepholeOptimizer::optimize_code(code, &mut None);
        assert_eq!(optimized.len(), 7);
        assert_eq!(optimized[0], Instr::new(Op::PushInt, 5, 0));
        assert_eq!(optimized[3], Instr::new(Op::JumpIfZero, 6, 0));
        assert_eq!(optimized[5], Instr::new(Op::Jump, 2, 0));
    }

    #[test]
    fn does_not_fold_across_a_branch_target() {
        let code = vec![
            Instr::new(Op::Jump, 1, 0),
            Instr::new(Op::PushInt, 2, 0),
            Instr::new(Op::PushInt, 3, 0),
            Instr::new(Op::Add, 0, 0),
            Instr::new(Op::Halt, 0, 0),
        ];

        let optimized = PeepholeOptimizer::optimize_code(code, &mut None);
        assert_eq!(optimized.len(), 3);
        assert_eq!(optimized[0], Instr::new(Op::Jump, 1, 0));
        assert_eq!(optimized[1], Instr::new(Op::PushInt, 5, 0));
    }

    #[test]
    fn minimum_integer_negation_is_left_for_runtime_overflow_handling() {
        let code = vec![Instr::new(Op::PushInt, i64::MIN, 0), Instr::new(Op::Neg, 0, 0)];
        assert_eq!(PeepholeOptimizer::optimize_code(code.clone(), &mut None), code);
    }
}
