//! LLVM IR emission for whole checked-scalar call closures (program profile).
//!
//! Mirrors the emitted x86-64 image (`native::x64`) and the Cranelift module
//! (`cranelift::program`) plus the authoritative accounting in
//! `program.rs::execute`: one shared fuel budget and one trap channel live in
//! the invocation context, every frame owns a bank segment addressed by its
//! call depth, and each call checks the depth limit before touching the
//! callee's segment. Results, shared fuel minima and trap pairs (category and
//! failing instruction) therefore match the reference interpreter exactly.
//!
//! Module ABI: every closure member is
//! `uint64_t tinylang_program_N(uint8_t *context)` where `N` is the scalar
//! program function index; the entry member is named with that entry index.
//! The caller supplies an aligned, writable 40-byte invocation context laid out
//! as `uint64_t depth` at offset 0, `uint32_t trap` at offset 8, `uint64_t
//! fuel` at offset 16, `uint64_t *bank` at offset 24, and `uint64_t pc` at
//! offset 32. It preallocates `(depth_limit + 1)` segments of `stride_values`
//! zero-initialized i64 values whose stride is documented in the header
//! comment, and copies arguments into segment 0's first slots in operand-stack
//! order. Trap tags: 1 overflow, 2 division by zero, 3 fuel exhausted, 4
//! call-stack overflow. A trapping callee stores the tag and the failing
//! instruction into the context and returns 0; callers propagate that state
//! untouched. Emission alone does not claim executable-backend availability
//! or validation.

use super::emission::{Text, TrapSink, binary, checked_result, load_stack, stack_pointer};
use crate::execution::engine::{EngineError, Instruction, ScalarFunction, ScalarProgram};

// Overflow (1) and division-by-zero (2) tags are emitted by the shared leaf
// `binary` helper in `super::emission`, which uses the same numbers as the
// leaf profile's status codes.
const FUEL_EXHAUSTED: u32 = 3;
const CALL_DEPTH: u32 = 4;

/// Program-profile trap sink: the failing pc is already recorded by the
/// per-instruction preamble, so the trap block only stores the tag in the
/// shared context and returns zero; callers propagate that state untouched.
struct ProgramTrap;

impl TrapSink for ProgramTrap {
    fn trap(&mut self, out: &mut Text, label: &str, tag: u32, _pc: usize) -> Result<(), EngineError> {
        out.emit(format_args!("{label}:\n  store i32 {tag}, ptr %trap_ptr, align 4\n  ret i64 0\n"))
    }
}

pub fn emit_program_llvm_ir(program: &ScalarProgram) -> Result<String, EngineError> {
    let mut out = Text::default();
    let mut stride_values = 0usize;
    for index in 0..program.function_count() {
        if let Some(function) = program.function(index) {
            stride_values = stride_values.max(function.slot_count() + function.max_stack());
        }
    }
    let depth_limit = program.call_depth_limit();
    let entry = program.entry();
    out.emit(format_args!(
        "; TinyLang checked-i64 program ABI v1\n\
         ; entry: tinylang_program_{entry}\n\
         ; stride: {stride_values} i64 values per call-depth segment\n\
         ; depth_limit: {depth_limit}\n\
         ; context: uint64 depth@0, uint32 trap@8, uint64 fuel@16, uint64* bank@24, uint64 pc@32\n\
         ; traps: 1 overflow, 2 division by zero, 3 fuel exhausted, 4 call stack overflow\n\
         declare {{ i64, i1 }} @llvm.sadd.with.overflow.i64(i64, i64)\n\
         declare {{ i64, i1 }} @llvm.ssub.with.overflow.i64(i64, i64)\n\
         declare {{ i64, i1 }} @llvm.smul.with.overflow.i64(i64, i64)\n\n"
    ))?;
    for index in 0..program.function_count() {
        let Some(function) = program.function(index) else {
            continue;
        };
        emit_function(&mut out, index, function, stride_values, depth_limit)?;
    }
    out.finish()
}

/// Lowers one closure member into `function`. The prologue derives the frame
/// pointers from the context depth, records the failing instruction before
/// anything can trap, and shares one fuel budget through the context.
fn emit_function(
    out: &mut Text,
    index: usize,
    function: &ScalarFunction,
    stride_values: usize,
    depth_limit: usize,
) -> Result<(), EngineError> {
    let mut traps = ProgramTrap;
    let slot_count = function.slot_count();
    out.emit(format_args!(
        "define i64 @tinylang_program_{index}(ptr %context) {{\n\
         \x20 %depth = load i64, ptr %context, align 8\n\
         \x20 %depth_ptr = getelementptr i8, ptr %context, i64 0\n\
         \x20 %trap_ptr = getelementptr i8, ptr %context, i64 8\n\
         \x20 %fuel_ptr = getelementptr i8, ptr %context, i64 16\n\
         \x20 %bank_ptr = getelementptr i8, ptr %context, i64 24\n\
         \x20 %pc_ptr = getelementptr i8, ptr %context, i64 32\n\
         \x20 %bank = load ptr, ptr %bank_ptr, align 8\n\
         \x20 %segment = mul i64 %depth, {stride_values}\n\
         \x20 %slots = getelementptr i64, ptr %bank, i64 %segment\n\
         \x20 %stack = getelementptr i64, ptr %slots, i64 {slot_count}\n\
         \x20 %nested = add i64 %depth, 1\n\
         \x20 store i64 %nested, ptr %depth_ptr, align 8\n\
         \x20 br label %i0\n"
    ))?;
    for (pc, &operation) in function.instructions().iter().enumerate() {
        let Some(height) = function.stack_heights()[pc] else {
            continue;
        };
        preamble(out, pc)?;
        match operation {
            Instruction::Constant(value) => {
                stack_pointer(out, pc, height, "destination")?;
                out.emit(format_args!("  store i64 {value}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Load(slot) => {
                out.emit(format_args!("  %local{pc} = getelementptr i64, ptr %slots, i64 {slot}\n"))?;
                out.emit(format_args!("  %value{pc} = load i64, ptr %local{pc}, align 8\n"))?;
                stack_pointer(out, pc, height, "destination")?;
                out.emit(format_args!("  store i64 %value{pc}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Store(slot) => {
                load_stack(out, pc, height - 1, "value")?;
                out.emit(format_args!("  %local{pc} = getelementptr i64, ptr %slots, i64 {slot}\n"))?;
                out.emit(format_args!("  store i64 %value{pc}, ptr %local{pc}, align 8\n"))?;
            }
            Instruction::Binary(op) => {
                load_stack(out, pc, height - 2, "left")?;
                load_stack(out, pc, height - 1, "right")?;
                binary(out, &mut traps, op, pc)?;
                stack_pointer(out, pc, height - 2, "destination")?;
                out.emit(format_args!("  store i64 %result{pc}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Negate => {
                load_stack(out, pc, height - 1, "value")?;
                out.emit(format_args!(
                    "  %pair{pc} = call {{ i64, i1 }} @llvm.ssub.with.overflow.i64(i64 0, i64 %value{pc})\n"
                ))?;
                checked_result(out, &mut traps, pc)?;
                stack_pointer(out, pc, height - 1, "destination")?;
                out.emit(format_args!("  store i64 %result{pc}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Jump(destination) => {
                out.emit(format_args!("  br label %i{destination}\n"))?;
                continue;
            }
            Instruction::JumpIfZero(destination) => {
                load_stack(out, pc, height - 1, "condition")?;
                out.emit(format_args!(
                    "  %zero{pc} = icmp eq i64 %condition{pc}, 0\n\
                     \x20 br i1 %zero{pc}, label %i{destination}, label %i{}\n",
                    pc + 1
                ))?;
                continue;
            }
            Instruction::Pop => {}
            Instruction::Call(callee, argument_count) => {
                // The reference interpreter traps when `depth + 1` exceeds the
                // program's declared limit; here the callee depth is already in
                // the context, mirroring the Cranelift program profile.
                out.emit(format_args!(
                    "  %depth_overflow{pc} = icmp sgt i64 %nested, {depth_limit}\n\
                     \x20 br i1 %depth_overflow{pc}, label %call_depth_trap{pc}, label %copy_args{pc}\n"
                ))?;
                out.emit(format_args!(
                    "call_depth_trap{pc}:\n\
                     \x20 store i32 {CALL_DEPTH}, ptr %trap_ptr, align 4\n\
                     \x20 ret i64 0\n"
                ))?;
                out.emit(format_args!("copy_args{pc}:\n"))?;
                // The callee segment is bank + nested * stride. Arguments leave
                // in operand-stack order: the deepest value is argument zero
                // and lands in the callee's first slot.
                out.emit(format_args!(
                    "  %callee_offset{pc} = mul i64 %nested, {stride_values}\n\
                     \x20 %callee_segment{pc} = getelementptr i64, ptr %bank, i64 %callee_offset{pc}\n"
                ))?;
                for argument in 0..argument_count {
                    let slot = height - argument_count + argument;
                    out.emit(format_args!(
                        "  %arg{pc}_{argument} = getelementptr i64, ptr %stack, i64 {slot}\n\
                         \x20 %arg{pc}_value{argument} = load i64, ptr %arg{pc}_{argument}, align 8\n\
                         \x20 %callee_slot{pc}_{argument} = getelementptr i64, ptr %callee_segment{pc}, i64 {argument}\n\
                         \x20 store i64 %arg{pc}_value{argument}, ptr %callee_slot{pc}_{argument}, align 8\n"
                    ))?;
                }
                out.emit(format_args!("  %callee_result{pc} = call i64 @tinylang_program_{callee}(ptr %context)\n"))?;
                // A callee trap propagates untouched: this frame returns
                // without touching the context's trap tag or location.
                out.emit(format_args!(
                    "  %trapped_tag{pc} = load i32, ptr %trap_ptr, align 4\n\
                     \x20 %trapped{pc} = icmp ne i32 %trapped_tag{pc}, 0\n\
                     \x20 br i1 %trapped{pc}, label %propagate{pc}, label %call_continue{pc}\n\
                     propagate{pc}:\n\
                     \x20 ret i64 %callee_result{pc}\n\
                     call_continue{pc}:\n"
                ))?;
                // The result replaces the deepest popped argument.
                let result_slot = height - argument_count;
                out.emit(format_args!(
                    "  %call_dest{pc} = getelementptr i64, ptr %stack, i64 {result_slot}\n\
                     \x20 store i64 %callee_result{pc}, ptr %call_dest{pc}, align 8\n"
                ))?;
            }
            Instruction::Return => {
                let value = load_stack_value(out, pc, height - 1)?;
                // Hand the context depth back to the caller.
                out.emit(format_args!("  store i64 %depth, ptr %depth_ptr, align 8\n"))?;
                out.emit(format_args!("  ret i64 {value}\n"))?;
                continue;
            }
        }
        out.emit(format_args!("  br label %i{}\n", pc + 1))?;
    }
    out.emit(format_args!("}}\n\n"))
}

/// Block preamble for one executed instruction: records the failing pc
/// before anything can trap, then checks and decrements the shared fuel.
fn preamble(out: &mut Text, pc: usize) -> Result<(), EngineError> {
    out.emit(format_args!(
        "i{pc}:\n\
         \x20 store i64 {pc}, ptr %pc_ptr, align 8\n\
         \x20 %f{pc} = load i64, ptr %fuel_ptr, align 8\n\
         \x20 %empty{pc} = icmp eq i64 %f{pc}, 0\n\
         \x20 br i1 %empty{pc}, label %fuel_trap{pc}, label %work{pc}\n\
         fuel_trap{pc}:\n\
         \x20 store i32 {FUEL_EXHAUSTED}, ptr %trap_ptr, align 4\n\
         \x20 ret i64 0\n\
         work{pc}:\n\
         \x20 %remaining{pc} = sub i64 %f{pc}, 1\n\
         \x20 store i64 %remaining{pc}, ptr %fuel_ptr, align 8\n"
    ))
}

/// Loads the value at `stack[slot]` into `%{name}{pc}`.
fn load_stack_value(out: &mut Text, pc: usize, slot: usize) -> Result<String, EngineError> {
    out.emit(format_args!(
        "  %value_ptr{pc} = getelementptr i64, ptr %stack, i64 {slot}\n\
         \x20 %value{pc} = load i64, ptr %value_ptr{pc}, align 8\n"
    ))?;
    Ok(format!("%value{pc}"))
}
