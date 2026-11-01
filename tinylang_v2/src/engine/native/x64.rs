//! Small purpose-built assembler for the verified scalar profile.
//!
//! Windows x64 volatile register assignment:
//! R10 = frame, R8 = locals, R9 = operand stack, R11 = remaining fuel;
//! RAX/RCX/RDX = arithmetic scratch. No calls, pushes or RSP adjustments.
//! Static stack offsets rely on the CFG entry heights proven during lowering.

use super::{CallFrame, DIVISION_BY_ZERO, EngineError, FUEL_EXHAUSTED, OVERFLOW, ScalarFunction};
use crate::engine::{BinaryOp, Instruction};

#[derive(Clone, Copy)]
struct Label(usize);

struct Relocation {
    displacement: usize,
    target: Label,
}

struct Assembler {
    bytes: Vec<u8>,
    labels: Vec<Option<usize>>,
    relocations: Vec<Relocation>,
}

impl Assembler {
    fn new(instructions: usize) -> Result<Self, EngineError> {
        // Scalar lowering bounds instructions to 4096. Each instruction emits
        // fewer than 128 bytes, six rel32 edges and at most two internal labels.
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(instructions * 128 + 64)
            .map_err(|error| EngineError::Allocation(error.to_string()))?;
        let mut labels = Vec::new();
        labels
            .try_reserve_exact(instructions * 3 + 3)
            .map_err(|error| EngineError::Allocation(error.to_string()))?;
        labels.resize(instructions, None);
        let mut relocations = Vec::new();
        relocations
            .try_reserve_exact(instructions * 6)
            .map_err(|error| EngineError::Allocation(error.to_string()))?;
        Ok(Self {
            bytes,
            labels,
            relocations,
        })
    }

    fn label(&mut self) -> Label {
        let label = Label(self.labels.len());
        self.labels.push(None);
        label
    }

    fn bind(&mut self, label: Label) -> Result<(), EngineError> {
        let position = self
            .labels
            .get_mut(label.0)
            .ok_or_else(|| EngineError::Codegen("invalid native label".to_owned()))?;
        if position.replace(self.bytes.len()).is_some() {
            return Err(EngineError::Codegen("native label bound twice".to_owned()));
        }
        Ok(())
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn edge(&mut self, opcode: &[u8], target: Label) {
        self.emit(opcode);
        let displacement = self.bytes.len();
        self.emit(&[0; 4]);
        self.relocations.push(Relocation {
            displacement,
            target,
        });
    }

    fn frame_immediate(&mut self, offset: usize, value: u32) -> Result<(), EngineError> {
        let offset = u8::try_from(offset)
            .map_err(|_| EngineError::Codegen("native frame field exceeds disp8".to_owned()))?;
        // mov qword [r10 + disp8], sign-extended imm32 (all tags/PCs <= 4096).
        self.emit(&[0x49, 0xc7, 0x42, offset]);
        self.emit(&value.to_le_bytes());
        Ok(())
    }

    fn memory(&mut self, opcode: &[u8], index: usize) -> Result<(), EngineError> {
        let displacement = index
            .checked_mul(8)
            .and_then(|bytes| i32::try_from(bytes).ok())
            .ok_or_else(|| EngineError::Codegen("native frame offset exceeds disp32".to_owned()))?;
        self.emit(opcode);
        self.emit(&displacement.to_le_bytes());
        Ok(())
    }

    fn load_stack_rax(&mut self, index: usize) -> Result<(), EngineError> {
        self.memory(&[0x49, 0x8b, 0x81], index) // mov rax, [r9 + disp32]
    }

    fn load_stack_rcx(&mut self, index: usize) -> Result<(), EngineError> {
        self.memory(&[0x49, 0x8b, 0x89], index) // mov rcx, [r9 + disp32]
    }

    fn store_stack_rax(&mut self, index: usize) -> Result<(), EngineError> {
        self.memory(&[0x49, 0x89, 0x81], index) // mov [r9 + disp32], rax
    }

    fn finish(mut self) -> Result<Vec<u8>, EngineError> {
        for relocation in self.relocations {
            let target = self
                .labels
                .get(relocation.target.0)
                .copied()
                .flatten()
                .ok_or_else(|| EngineError::Codegen("unbound native branch target".to_owned()))?;
            let relative = i64::try_from(target)
                .ok()
                .zip(i64::try_from(relocation.displacement + 4).ok())
                .and_then(|(target, next)| i32::try_from(target - next).ok())
                .ok_or_else(|| EngineError::Codegen("native branch exceeds rel32".to_owned()))?;
            let patch = self
                .bytes
                .get_mut(relocation.displacement..relocation.displacement + 4)
                .ok_or_else(|| EngineError::Codegen("invalid native relocation".to_owned()))?;
            patch.copy_from_slice(&relative.to_le_bytes());
        }
        Ok(self.bytes)
    }
}

pub(super) fn emit(function: &ScalarFunction) -> Result<Vec<u8>, EngineError> {
    let mut code = Assembler::new(function.instructions().len())?;
    let overflow = code.label();
    let division_zero = code.label();
    let exhausted = code.label();

    code.emit(&[
        0xf3, 0x0f, 0x1e, 0xfa, // endbr64: valid indirect entry on CET/IBT hosts
        0x49, 0x89, 0xca, // mov r10, rcx (Windows x64 first argument)
        0x4d, 0x8b, 0x02, // mov r8, [r10] (slots)
        0x4d, 0x8b, 0x4a, 0x08, // mov r9, [r10+8] (operand stack)
        0x4d, 0x8b, 0x5a, 0x10, // mov r11, [r10+16] (fuel)
    ]);

    for (pc, instruction) in function.instructions().iter().enumerate() {
        let Some(height) = function.stack_heights()[pc] else {
            continue;
        };
        code.bind(Label(pc))?;
        code.frame_immediate(std::mem::offset_of!(CallFrame, instruction), pc as u32)?;
        code.emit(&[0x4d, 0x85, 0xdb]); // test r11, r11
        code.edge(&[0x0f, 0x84], exhausted); // jz before executing original IR
        code.emit(&[0x49, 0xff, 0xcb]); // dec r11

        match *instruction {
            Instruction::Constant(value) => {
                code.emit(&[0x48, 0xb8]); // mov rax, imm64
                code.emit(&value.to_le_bytes());
                code.store_stack_rax(height)?;
            }
            Instruction::Load(slot) => {
                code.memory(&[0x49, 0x8b, 0x80], slot)?; // mov rax, [r8+disp32]
                code.store_stack_rax(height)?;
            }
            Instruction::Store(slot) => {
                code.load_stack_rax(height - 1)?;
                code.memory(&[0x49, 0x89, 0x80], slot)?; // mov [r8+disp32], rax
            }
            Instruction::Binary(binary) => {
                code.load_stack_rax(height - 2)?;
                code.load_stack_rcx(height - 1)?;
                emit_binary(&mut code, binary, overflow, division_zero)?;
                code.store_stack_rax(height - 2)?;
            }
            Instruction::Negate => {
                code.load_stack_rax(height - 1)?;
                code.emit(&[0x48, 0xf7, 0xd8]); // neg rax
                code.edge(&[0x0f, 0x80], overflow); // jo
                code.store_stack_rax(height - 1)?;
            }
            Instruction::Jump(target) => code.edge(&[0xe9], Label(target)),
            Instruction::JumpIfZero(target) => {
                code.load_stack_rax(height - 1)?;
                code.emit(&[0x48, 0x85, 0xc0]); // test rax, rax
                code.edge(&[0x0f, 0x84], Label(target));
            }
            Instruction::Pop => {} // Its statically known successor height pops.
            Instruction::Return => {
                code.load_stack_rax(height - 1)?;
                code.emit(&[0x49, 0x89, 0x42, 0x18, 0xc3]); // frame.result=rax; ret
            }
        }
    }
    for (label, trap) in [
        (overflow, OVERFLOW),
        (division_zero, DIVISION_BY_ZERO),
        (exhausted, FUEL_EXHAUSTED),
    ] {
        code.bind(label)?;
        code.frame_immediate(std::mem::offset_of!(CallFrame, trap), trap)?;
        code.emit(&[0xc3]);
    }
    code.finish()
}

fn emit_binary(
    code: &mut Assembler,
    operation: BinaryOp,
    overflow: Label,
    division_zero: Label,
) -> Result<(), EngineError> {
    match operation {
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply => {
            code.emit(match operation {
                BinaryOp::Add => &[0x48, 0x01, 0xc8],      // add rax, rcx
                BinaryOp::Subtract => &[0x48, 0x29, 0xc8], // sub rax, rcx
                _ => &[0x48, 0x0f, 0xaf, 0xc1],            // imul rax, rcx (signed OF)
            });
            code.edge(&[0x0f, 0x80], overflow);
        }
        BinaryOp::Divide => {
            let divide = code.label();
            let done = code.label();
            code.emit(&[0x48, 0x85, 0xc9]); // test rcx, rcx
            code.edge(&[0x0f, 0x84], division_zero);
            code.emit(&[0x48, 0x83, 0xf9, 0xff]); // cmp rcx, -1
            code.edge(&[0x0f, 0x85], divide);
            code.emit(&[0x48, 0xba]); // mov rdx, i64::MIN
            code.emit(&i64::MIN.to_le_bytes());
            code.emit(&[0x48, 0x39, 0xd0]); // cmp rax, rdx
            code.edge(&[0x0f, 0x84], overflow);
            code.bind(divide)?;
            code.emit(&[0x48, 0x99, 0x48, 0xf7, 0xf9]); // cqo; idiv rcx
            // TinyLang's authoritative VM uses floor division. idiv truncates
            // toward zero: decrement iff nonzero remainder and opposite signs.
            // The remainder has the dividend's sign, so xor with divisor tests
            // sign disagreement without retaining another scratch register.
            code.emit(&[0x48, 0x85, 0xd2]); // test rdx, rdx (remainder)
            code.edge(&[0x0f, 0x84], done); // jz: exact quotient
            code.emit(&[0x48, 0x31, 0xca]); // xor rdx, rcx
            code.edge(&[0x0f, 0x89], done); // jns: signs agree
            // A nonexact quotient cannot be MIN, so this cannot overflow.
            code.emit(&[0x48, 0xff, 0xc8]); // dec rax
            code.bind(done)?;
        }
        comparison => {
            let condition = match comparison {
                BinaryOp::Less => 0x9c,
                BinaryOp::LessEqual => 0x9e,
                BinaryOp::Greater => 0x9f,
                BinaryOp::GreaterEqual => 0x9d,
                BinaryOp::Equal => 0x94,
                BinaryOp::NotEqual => 0x95,
                _ => return Err(EngineError::Codegen("invalid native comparison".to_owned())),
            };
            code.emit(&[0x48, 0x39, 0xc8, 0x0f, condition, 0xc0]); // cmp; setcc al
            code.emit(&[0x48, 0x0f, 0xb6, 0xc0]); // movzx rax, al (canonical i64)
        }
    }
    Ok(())
}
