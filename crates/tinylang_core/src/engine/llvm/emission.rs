use std::fmt::{self, Write};

use crate::engine::{BinaryOp, EngineError, Instruction, ScalarFunction};

pub fn emit_llvm_ir(function: &ScalarFunction) -> Result<String, EngineError> {
    let mut out = Text::default();
    out.emit(format_args!(
        "; TinyLang checked-i64 scalar frame ABI v1\n\
         declare {{ i64, i1 }} @llvm.sadd.with.overflow.i64(i64, i64)\n\
         declare {{ i64, i1 }} @llvm.ssub.with.overflow.i64(i64, i64)\n\
         declare {{ i64, i1 }} @llvm.smul.with.overflow.i64(i64, i64)\n\n\
         define i32 @tinylang_scalar(ptr %slots, ptr %stack, ptr %output, ptr %fuel, ptr %error_instruction) {{\n\
         entry:\n  br label %i0\n"
    ))?;
    for (pc, &operation) in function.instructions().iter().enumerate() {
        let Some(height) = function.stack_heights()[pc] else {
            continue;
        };
        out.emit(format_args!(
            "i{pc}:\n  %f{pc} = load i64, ptr %fuel, align 8\n\
             \x20 %empty{pc} = icmp eq i64 %f{pc}, 0\n\
             \x20 br i1 %empty{pc}, label %fuel_trap{pc}, label %work{pc}\n"
        ))?;
        trap(&mut out, &format!("fuel_trap{pc}"), 3, pc)?;
        out.emit(format_args!(
            "work{pc}:\n  %remaining{pc} = sub i64 %f{pc}, 1\n\
             \x20 store i64 %remaining{pc}, ptr %fuel, align 8\n"
        ))?;
        match operation {
            Instruction::Constant(value) => {
                stack_pointer(&mut out, pc, height, "destination")?;
                out.emit(format_args!("  store i64 {value}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Load(slot) => {
                out.emit(format_args!("  %local{pc} = getelementptr i64, ptr %slots, i64 {slot}\n"))?;
                out.emit(format_args!("  %value{pc} = load i64, ptr %local{pc}, align 8\n"))?;
                stack_pointer(&mut out, pc, height, "destination")?;
                out.emit(format_args!("  store i64 %value{pc}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Store(slot) => {
                load_stack(&mut out, pc, height - 1, "value")?;
                out.emit(format_args!("  %local{pc} = getelementptr i64, ptr %slots, i64 {slot}\n"))?;
                out.emit(format_args!("  store i64 %value{pc}, ptr %local{pc}, align 8\n"))?;
            }
            Instruction::Binary(op) => {
                load_stack(&mut out, pc, height - 2, "left")?;
                load_stack(&mut out, pc, height - 1, "right")?;
                binary(&mut out, op, pc)?;
                stack_pointer(&mut out, pc, height - 2, "destination")?;
                out.emit(format_args!("  store i64 %result{pc}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Negate => {
                load_stack(&mut out, pc, height - 1, "value")?;
                out.emit(format_args!(
                    "  %pair{pc} = call {{ i64, i1 }} @llvm.ssub.with.overflow.i64(i64 0, i64 %value{pc})\n"
                ))?;
                checked_result(&mut out, pc)?;
                stack_pointer(&mut out, pc, height - 1, "destination")?;
                out.emit(format_args!("  store i64 %result{pc}, ptr %destination{pc}, align 8\n"))?;
            }
            Instruction::Jump(target) => {
                out.emit(format_args!("  br label %i{target}\n"))?;
                continue;
            }
            Instruction::JumpIfZero(target) => {
                load_stack(&mut out, pc, height - 1, "condition")?;
                out.emit(format_args!(
                    "  %zero{pc} = icmp eq i64 %condition{pc}, 0\n\
                     \x20 br i1 %zero{pc}, label %i{target}, label %i{}\n",
                    pc + 1,
                ))?;
                continue;
            }
            Instruction::Pop => {}
            Instruction::Return => {
                load_stack(&mut out, pc, 0, "value")?;
                out.emit(format_args!("  store i64 %value{pc}, ptr %output, align 8\n  ret i32 0\n"))?;
                continue;
            }
        }
        out.emit(format_args!("  br label %i{}\n", pc + 1))?;
    }
    out.emit(format_args!("}}\n"))?;
    Ok(out.text)
}

#[derive(Default)]
struct Text {
    text:             String,
    allocation_error: Option<std::collections::TryReserveError>,
}

impl Write for Text {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.text.try_reserve(text.len()).map_err(|error| {
            self.allocation_error = Some(error);
            fmt::Error
        })?;
        self.text.push_str(text);
        Ok(())
    }
}

impl Text {
    fn emit(&mut self, args: fmt::Arguments<'_>) -> Result<(), EngineError> {
        fmt::write(self, args).map_err(|error| {
            match self.allocation_error.take() {
                Some(allocation) => EngineError::Allocation(allocation.to_string()),
                None => EngineError::Codegen(error.to_string()),
            }
        })
    }
}

fn stack_pointer(out: &mut Text, pc: usize, slot: usize, name: &str) -> Result<(), EngineError> {
    out.emit(format_args!("  %{name}{pc} = getelementptr i64, ptr %stack, i64 {slot}\n"))
}

fn load_stack(out: &mut Text, pc: usize, slot: usize, name: &str) -> Result<(), EngineError> {
    out.emit(format_args!("  %{name}_ptr{pc} = getelementptr i64, ptr %stack, i64 {slot}\n"))?;
    out.emit(format_args!("  %{name}{pc} = load i64, ptr %{name}_ptr{pc}, align 8\n"))
}

fn trap(out: &mut Text, label: &str, status: u32, pc: usize) -> Result<(), EngineError> {
    out.emit(format_args!("{label}:\n  store i64 {pc}, ptr %error_instruction, align 8\n  ret i32 {status}\n"))
}

fn checked_result(out: &mut Text, pc: usize) -> Result<(), EngineError> {
    out.emit(format_args!(
        "  %result{pc} = extractvalue {{ i64, i1 }} %pair{pc}, 0\n\
         \x20 %overflow{pc} = extractvalue {{ i64, i1 }} %pair{pc}, 1\n\
         \x20 br i1 %overflow{pc}, label %overflow_trap{pc}, label %arithmetic_ok{pc}\n"
    ))?;
    trap(out, &format!("overflow_trap{pc}"), 1, pc)?;
    out.emit(format_args!("arithmetic_ok{pc}:\n"))
}

fn binary(out: &mut Text, op: BinaryOp, pc: usize) -> Result<(), EngineError> {
    match op {
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => {
            let intrinsic = match op {
                BinaryOp::Add => "sadd",
                BinaryOp::Subtract => "ssub",
                _ => "smul",
            };
            out.emit(format_args!(
                "  %pair{pc} = call {{ i64, i1 }} @llvm.{intrinsic}.with.overflow.i64(i64 %left{pc}, i64 %right{pc})\n"
            ))?;
            checked_result(out, pc)
        }
        BinaryOp::Divide => {
            out.emit(format_args!(
                "  %divzero{pc} = icmp eq i64 %right{pc}, 0\n\
                 \x20 br i1 %divzero{pc}, label %division_trap{pc}, label %division_check{pc}\n"
            ))?;
            trap(out, &format!("division_trap{pc}"), 2, pc)?;
            out.emit(format_args!(
                "division_check{pc}:\n  %minimum{pc} = icmp eq i64 %left{pc}, -9223372036854775808\n\
                 \x20 %negative_one{pc} = icmp eq i64 %right{pc}, -1\n\
                 \x20 %overflow{pc} = and i1 %minimum{pc}, %negative_one{pc}\n\
                 \x20 br i1 %overflow{pc}, label %overflow_trap{pc}, label %division_ok{pc}\n"
            ))?;
            trap(out, &format!("overflow_trap{pc}"), 1, pc)?;
            // A nonzero remainder excludes the sole i64::MIN quotient (MIN/1).
            // Thus the 0/1 floor correction cannot overflow; nsw is justified.
            out.emit(format_args!(
                "division_ok{pc}:\n  %quotient{pc} = sdiv i64 %left{pc}, %right{pc}\n\
                 \x20 %remainder{pc} = srem i64 %left{pc}, %right{pc}\n\
                 \x20 %inexact{pc} = icmp ne i64 %remainder{pc}, 0\n\
                 \x20 %sign_bits{pc} = xor i64 %left{pc}, %right{pc}\n\
                 \x20 %opposite{pc} = icmp slt i64 %sign_bits{pc}, 0\n\
                 \x20 %adjust{pc} = and i1 %inexact{pc}, %opposite{pc}\n\
                 \x20 %correction{pc} = zext i1 %adjust{pc} to i64\n\
                 \x20 %result{pc} = sub nsw i64 %quotient{pc}, %correction{pc}\n"
            ))
        }
        comparison => {
            let predicate = match comparison {
                BinaryOp::Less => "slt",
                BinaryOp::LessEqual => "sle",
                BinaryOp::Greater => "sgt",
                BinaryOp::GreaterEqual => "sge",
                BinaryOp::Equal => "eq",
                BinaryOp::NotEqual => "ne",
                _ => return Err(EngineError::Codegen("non-comparison opcode".to_owned())),
            };
            out.emit(format_args!(
                "  %comparison{pc} = icmp {predicate} i64 %left{pc}, %right{pc}\n\
                 \x20 %result{pc} = zext i1 %comparison{pc} to i64\n"
            ))
        }
    }
}
