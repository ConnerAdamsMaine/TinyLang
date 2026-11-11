//! Capacity transitions in the existing lowering path, not allocator-call timing.
use super::*;
use crate::execution::jit::origins::FailureStage;
use crate::language::bytecode::control_flow::BytecodeOffset;

#[path = "../../../../tests/preparation/sources.rs"]
mod sources;

const OWNERS: [&str; 8] = [
    "expressions",
    "actions",
    "effects",
    "expression_origins",
    "action_origins",
    "roots",
    "slot_facts",
    "encoder_work",
];

fn retained_builder_bytes(lowerer: &SymbolicLowerer) -> [usize; 8] {
    let origins = lowerer.block.reserved_origin_extents();
    [
        lowerer.block.expressions.capacity() * size_of::<Expression>(),
        lowerer.block.reserved_action_bytes(),
        lowerer.block.effects.capacity() * size_of::<JitOp>(),
        origins[0],
        origins[1],
        lowerer.roots.capacity() * size_of::<TempId>(),
        lowerer.optimizer.reserved_bytes(),
        lowerer.encoder.reserved_bytes(),
    ]
}

#[derive(Default)]
struct CapacityTrace {
    bytes: [usize; 8],
}

impl CapacityTrace {
    fn observe(&mut self, lowerer: &SymbolicLowerer, fixture: &str, stage: &str, pc: usize) {
        let current = retained_builder_bytes(lowerer);
        for ((owner, previous), next) in OWNERS.iter().zip(self.bytes).zip(current) {
            assert!(next >= previous, "builder capacity is reused across blocks");
            if next != previous {
                eprintln!(
                    "jit-builder fixture={fixture} stage={stage} pc={pc} owner={owner} before_bytes={previous} after_bytes={next}"
                );
            }
        }
        self.bytes = current;
    }
}

#[test]
#[ignore = "builder-capacity diagnostic; run explicitly with --ignored --nocapture"]
fn report_source_builder_capacity_changes() {
    for fixture in sources::SOURCE_CASES {
        let verified = crate::compile_source_verified(fixture.source).expect("shared source fixture");
        let code = &verified.program().code;
        // All three collection idioms require a Builtin. This replay deliberately
        // covers only the ordinary symbolic route, not the collection matcher.
        assert!(code.iter().all(|instruction| instruction.op != Op::Builtin));
        let certificate = verified.chunk_certificate(0).expect("main certificate");
        let mut lowerer = SymbolicLowerer::default();
        let mut trace = CapacityTrace::default();
        let mut lowered = LoweredChunk::with_capacity(code.len());
        let mut targets = vec![0; code.len() + 1];
        for block in &certificate.blocks {
            let range = block.range();
            targets[range.clone()].fill(lowered.len());
            lowerer.begin_block(block.entry_depth.expect("source fixture blocks are reachable"));
            trace.observe(&lowerer, fixture.name, "block_start", range.start);
            for pc in range.clone() {
                lowerer.set_origin(OpOrigin::instruction(BytecodeOffset::parse(pc).expect("source offset")));
                lowerer.instruction(code[pc]).expect("symbolic instruction");
                trace.observe(&lowerer, fixture.name, "instruction", pc);
            }
            lowerer.finish(&mut lowered).expect("optimization and encoding");
            trace.observe(&lowerer, fixture.name, "block_finish", range.end);
        }
        targets[code.len()] = lowered.len();
        for op in lowered.ops_mut() {
            op.remap_targets(&targets);
        }
        let (ops, origins) = lowered.finish();
        let mut normal = crate::JitProgram::compile_verified(&verified).expect("ordinary lowering");
        let chunk = normal.chunks[0].as_ref().expect("main chunk");
        assert_eq!(&ops, &*chunk.ops, "replay must preserve ordinary encoded operations");
        for index in 0..ops.len() {
            for stage in [FailureStage::Operation, FailureStage::Secondary, FailureStage::Store] {
                assert_eq!(origins.instruction(index, stage), chunk.origins.instruction(index, stage));
            }
        }
        let mut reference = Vec::new();
        crate::run_verified_program(&verified, "vm", &mut reference, Vec::new()).expect("reference execution");
        let mut output = Vec::new();
        normal.run(&mut output, Vec::new()).expect("JIT execution");
        assert_eq!(reference, fixture.output.as_bytes());
        assert_eq!(output, reference);
        eprintln!(
            "jit-builder-summary fixture={} bytecode_ops={} compiled_ops={} retained_builder_bytes={:?}",
            fixture.name,
            code.len(),
            ops.len(),
            trace.bytes
        );
    }
}
