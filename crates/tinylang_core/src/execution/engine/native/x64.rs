//! Small purpose-built assembler for the verified scalar profile.
//!
//! Volatile register assignment, valid under both supported ABIs (Windows x64
//! and SysV AMD64 on Linux) because every register listed is caller-saved on
//! both:
//! R10 = frame, R8 = locals, R9 = operand stack, R11 = remaining fuel;
//! RAX/RCX/RDX = arithmetic scratch. No calls, pushes or RSP adjustments.
//! Static stack offsets rely on the CFG entry heights proven during lowering.
//! The single pointer argument arrives in RCX on Windows and in RDI on Linux
//! (see [`ENTRY_ARG_TO_R10`]); on Windows RDI is nonvolatile but never used
//! past the prologue read, so the image preserves the nonvolatile set of both
//! ABIs.

use super::{CallFrame, DIVISION_BY_ZERO, EngineError, FUEL_EXHAUSTED, OVERFLOW, ScalarFunction};
use crate::execution::engine::{BinaryOp, Instruction};

/// Loads the callee's one pointer argument into R10. This is the only
/// instruction the ABIs disagree on: Windows x64 passes the first argument in
/// RCX, SysV AMD64 (Linux) in RDI.
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
const ENTRY_ARG_TO_R10: [u8; 3] = [0x49, 0x89, 0xca]; // mov r10, rcx
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
const ENTRY_ARG_TO_R10: [u8; 3] = [0x49, 0x89, 0xfa]; // mov r10, rdi

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
        self.relocations.push(Relocation { displacement, target });
    }

    fn frame_immediate(&mut self, offset: usize, value: u32) -> Result<(), EngineError> {
        let offset =
            u8::try_from(offset).map_err(|_| EngineError::Codegen("native frame field exceeds disp8".to_owned()))?;
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

    code.emit(&[0xf3, 0x0f, 0x1e, 0xfa]); // endbr64: valid indirect entry on CET/IBT hosts
    code.emit(&ENTRY_ARG_TO_R10); // r10 = frame pointer (ABI-selected argument register)
    code.emit(&[
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
            Instruction::Call(..) => {
                return Err(EngineError::Codegen(
                    "single function emission cannot execute calls; use program compilation".to_owned(),
                ));
            }
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

/// Deferred cross-function call relocation inside the program image.
struct CrossCall {
    displacement: usize,
    callee: usize,
}

/// One finished body and its pending cross-function relocations.
type AssembledBody = (Vec<u8>, Vec<CrossCall>);

/// Machine-code ABI for the program profile (all functions in one image).
///
/// Volatile register assignment (volatile under both the Windows x64 and
/// SysV AMD64 ABIs):
/// R10 = per-invocation context (volatile on both ABIs, so no callee ever
/// has to preserve it), R8 = current
/// frame slots base, R9 = current frame operand-stack base, R11 = remaining
/// fuel (threaded through calls: every executed instruction deducts from the
/// one shared budget), RAX/RCX/RDX = arithmetic scratch.
///
/// Cross-function `call rel32` edges stay inside the image and the bodies
/// never read the hardware stack beyond popping their return address, so
/// SysV's 16-byte RSP alignment rule is not observable inside the image.
///
/// Context layout (u64 offsets): 0x00 call depth, 0x08 trap code (u32),
/// 0x10 entry fuel, 0x18 frame bank base, 0x20 failing instruction. The
/// host owns one invocation context and one frame bank; the machine code
/// never spills registers, so the hardware stack only carries return
/// addresses. Depth runs 0..=limit with the same call-depth accounting as
/// the reference interpreter and the authoritative VM.
///
/// Frame bank: depth `d` owns one segment of `stride_values` values. Slots
/// occupy the first `slots_cap` values, and every function's operand stack
/// pointer is `segment + slots_cap`, whatever the executing function is.
pub(super) fn emit_program(
    functions: &[Option<&ScalarFunction>],
    entry: usize,
    slots_cap: usize,
    stack_cap: usize,
    depth_limit: usize,
) -> Result<Vec<u8>, EngineError> {
    const CALL_DEPTH: u32 = 4;
    // Host-side entry cue: the image starts with it, immediately followed by
    // the entry function body. It exists only for the host's indirect call;
    // cross-function calls patch straight to the bodies. Bytes 4..7 are the
    // ABI-selected argument load (see `ENTRY_ARG_TO_R10`).
    let mut host_cue = [
        0xf3, 0x0f, 0x1e, 0xfa, // endbr64: valid indirect entry on CET/IBT hosts
        0, 0, 0, // patched below: mov r10, <abi argument register>
        0x4d, 0x8b, 0x5a, 0x10, // mov r11, [r10+0x10] (initial fuel)
    ];
    host_cue[4..7].copy_from_slice(&ENTRY_ARG_TO_R10);
    let stride_bytes = (slots_cap + stack_cap) * size_of::<i64>();
    // Immediates use the 32-bit forms selected throughout the emitter.
    let bound = |value: i64, name: &str| -> Result<i32, EngineError> {
        i32::try_from(value).map_err(|_| EngineError::Codegen(format!("{name} exceeds immediate range")))
    };
    let slot_bytes = bound((slots_cap * size_of::<i64>()) as i64, "slot capacity")?;
    let stride = bound(stride_bytes as i64, "frame stride")?;
    let limit = bound(depth_limit as i64, "call depth limit")?;

    // Stage one: assemble every lowered function with internal links patched
    // and cross-function calls deferred. Image slots follow program function
    // indexes, so `Call` targets translate directly to image offsets.
    let mut images: Vec<Option<(Assembler, Vec<CrossCall>)>> = Vec::with_capacity(functions.len());
    for slot in functions {
        match slot {
            Some(function) => images.push(Some((Assembler::new(function.instructions().len())?, Vec::new()))),
            None => images.push(None),
        }
    }
    for (_index, (function, (assembler, cross))) in
        functions
            .iter()
            .zip(images.iter_mut())
            .enumerate()
            .filter_map(|(index, (slot, image))| {
                let (assembler, cross) = image.as_mut()?;
                Some((index, (slot.as_ref()?, (assembler, cross))))
            })
    {
        let (overflow, division_zero, exhausted, depth, propagate) =
            (assembler.label(), assembler.label(), assembler.label(), assembler.label(), assembler.label());
        // Common prologue: read the frame index, reserve the next bank
        // segment, then point the two frame registers into the new segment.
        // The depth budget is enforced at every call site (see the `Call`
        // sequence) and the host enters the entry body at depth 0, so this
        // prologue cannot create a frame past the limit. The host-side entry
        // cue is not emitted here: it precedes the entry body in the
        // stage-two image, and internal calls target the body directly so a
        // nested entry skips the cue (a nested entry would otherwise load
        // leftover caller registers into r10).
        assembler.emit(&[0x49, 0x8b, 0x02]); // mov rax, [r10] (this frame's depth)
        assembler.emit(&[0x49, 0xff, 0x02]); // inc qword [r10]
        assembler.emit(&[0x4d, 0x8b, 0x42, 0x18]); // mov r8, [r10+0x18] (bank)
        assembler.emit(&[0x48, 0x69, 0xc0]); // imul rax, rax, stride (pre-increment index)
        assembler.emit(&stride.to_le_bytes());
        assembler.emit(&[0x49, 0x01, 0xc0]); // add r8, rax (this frame's slots)
        assembler.emit(&[0x4d, 0x8d, 0x88]); // lea r9, [r8+slot_bytes] (stack)
        assembler.emit(&slot_bytes.to_le_bytes());

        for (pc, instruction) in function.instructions().iter().enumerate() {
            let Some(height) = function.stack_heights()[pc] else {
                continue;
            };
            assembler.bind(Label(pc))?;
            assembler.emit(&[0x49, 0xc7, 0x42, 0x20]); // mov qword [r10+0x20], pc
            assembler.emit(&(pc as u32).to_le_bytes());
            assembler.emit(&[0x4d, 0x85, 0xdb]); // test r11, r11
            assembler.edge(&[0x0f, 0x84], exhausted); // jz before executing IR
            assembler.emit(&[0x49, 0xff, 0xcb]); // dec r11

            match *instruction {
                Instruction::Constant(value) => {
                    assembler.emit(&[0x48, 0xb8]); // mov rax, imm64
                    assembler.emit(&value.to_le_bytes());
                    assembler.store_stack_rax(height)?;
                }
                Instruction::Load(slot) => {
                    assembler.memory(&[0x49, 0x8b, 0x80], slot)?; // mov rax, [r8+disp32]
                    assembler.store_stack_rax(height)?;
                }
                Instruction::Store(slot) => {
                    assembler.load_stack_rax(height - 1)?;
                    assembler.memory(&[0x49, 0x89, 0x80], slot)?; // mov [r8+disp32], rax
                }
                Instruction::Binary(binary) => {
                    assembler.load_stack_rax(height - 2)?;
                    assembler.load_stack_rcx(height - 1)?;
                    emit_binary(assembler, binary, overflow, division_zero)?;
                    assembler.store_stack_rax(height - 2)?;
                }
                Instruction::Negate => {
                    assembler.load_stack_rax(height - 1)?;
                    assembler.emit(&[0x48, 0xf7, 0xd8]); // neg rax
                    assembler.edge(&[0x0f, 0x80], overflow); // jo
                    assembler.store_stack_rax(height - 1)?;
                }
                Instruction::Jump(target) => assembler.edge(&[0xe9], Label(target)),
                Instruction::JumpIfZero(target) => {
                    assembler.load_stack_rax(height - 1)?;
                    assembler.emit(&[0x48, 0x85, 0xc0]); // test rax, rax
                    assembler.edge(&[0x0f, 0x84], Label(target));
                }
                Instruction::Call(callee, arguments) => {
                    if arguments * size_of::<i64>() > slots_cap * size_of::<i64>() {
                        return Err(EngineError::Codegen("call arguments exceed the callee slot capacity".to_owned()));
                    }
                    // Interpreter parity: a call whose callee would run at
                    // depth limit+1 traps here, before any bank write. The
                    // counter then equals the callee's own depth: segment `d`
                    // starts at bank + d * stride, the same arithmetic the
                    // callee prologue performs when it re-reads the counter.
                    assembler.emit(&[0x49, 0x8b, 0x02]); // mov rax, [r10] (callee depth)
                    assembler.emit(&[0x48, 0x3d]); // cmp rax, limit
                    assembler.emit(&limit.to_le_bytes());
                    assembler.edge(&[0x0f, 0x87], depth); // ja: depth budget exhausted
                    assembler.emit(&[0x48, 0x69, 0xc0]); // imul rax, rax, stride
                    assembler.emit(&stride.to_le_bytes());
                    assembler.emit(&[0x49, 0x03, 0x42, 0x18]); // add rax, [r10+0x18]
                    for (argument, offset) in (0..arguments).zip((height - arguments)..height) {
                        assembler.load_stack_rcx(offset)?; // mov rcx, [r9+disp32]
                        let displacement = bound((argument * size_of::<i64>()) as i64, "argument displacement")?;
                        assembler.emit(&[0x48, 0x89, 0x88]); // mov [rax+disp32], rcx
                        assembler.emit(&displacement.to_le_bytes());
                    }
                    assembler.emit(&[0xe8]); // call rel32 (cross-function)
                    let displacement = assembler.bytes.len();
                    assembler.emit(&[0; 4]);
                    cross.push(CrossCall { displacement, callee });
                    // The callee left its return value in rax; keep it across
                    // the restore arithmetic below, which uses rax as scratch.
                    assembler.emit(&[0x48, 0x89, 0xc2]); // mov rdx, rax (save result)
                    // Restore this frame's pointers: the callee's Return
                    // decremented the counter back to this depth + 1, so the
                    // segment index is the value minus one.
                    assembler.emit(&[0x49, 0x8b, 0x02]); // mov rax, [r10]
                    assembler.emit(&[0x48, 0xff, 0xc8]); // dec rax (back to this frame's depth)
                    assembler.emit(&[0x48, 0x69, 0xc0]); // imul rax, rax, stride
                    assembler.emit(&stride.to_le_bytes());
                    assembler.emit(&[0x49, 0x03, 0x42, 0x18]); // add rax, [r10+0x18]
                    assembler.emit(&[0x49, 0x89, 0xc0]); // mov r8, rax
                    assembler.emit(&[0x4d, 0x8d, 0x88]); // lea r9, [r8+slot_bytes]
                    assembler.emit(&slot_bytes.to_le_bytes());
                    assembler.emit(&[0x41, 0x83, 0x7a, 0x08, 0x00]); // cmp dword [r10+8], 0
                    assembler.edge(&[0x0f, 0x85], propagate); // jne: propagate trap
                    assembler.emit(&[0x48, 0x89, 0xd0]); // mov rax, rdx (callee result)
                    assembler.store_stack_rax(height - arguments)?; // result
                }
                Instruction::Pop => {} // Its statically known successor height pops.
                Instruction::Return => {
                    assembler.load_stack_rax(height - 1)?;
                    assembler.emit(&[0x49, 0xff, 0x0a, 0xc3]); // dec qword [r10]; ret
                }
            }
        }
        for (label, trap) in [
            (overflow, OVERFLOW),
            (division_zero, DIVISION_BY_ZERO),
            (exhausted, FUEL_EXHAUSTED),
            (depth, CALL_DEPTH),
        ] {
            assembler.bind(label)?;
            assembler.emit(&[0x41, 0xc7, 0x42, 0x08]); // mov dword [r10+8], trap
            assembler.emit(&trap.to_le_bytes());
            assembler.emit(&[0xc3]);
        }
        // A nested callee already recorded the trap and failing instruction.
        assembler.bind(propagate)?;
        assembler.emit(&[0xc3]);
    }

    // Stage two: finish internal relocations, concatenate the images and
    // resolve cross-function calls. Unsupported slots occupy no image space
    // and get no entry, so a call into one is a codegen error rather than a
    // jump into empty space.
    let mut assembled: Vec<Option<AssembledBody>> = Vec::with_capacity(images.len());
    for slot in images {
        match slot {
            Some((assembler, pending)) => assembled.push(Some((assembler.finish()?, pending))),
            None => assembled.push(None),
        }
    }
    let mut bytes_total = 0usize;
    let mut cross_total = 0usize;
    for (bytes, pending) in assembled.iter().flatten() {
        bytes_total += bytes.len();
        cross_total += pending.len();
    }
    let mut image = Vec::new();
    image
        .try_reserve_exact(bytes_total)
        .map_err(|error| EngineError::Allocation(error.to_string()))?;
    let mut cross: Vec<(usize, usize)> = Vec::new();
    cross
        .try_reserve_exact(cross_total)
        .map_err(|error| EngineError::Allocation(error.to_string()))?;
    let mut entries: Vec<Option<usize>> = vec![None; assembled.len()];
    // Emission order: the entry body goes first, directly under the host cue,
    // so the cue falls through into the entry frame. Every other lowered body
    // keeps its program index order below it. Unsupported slots keep no image
    // space and no entry, so a call into one is a codegen error rather than a
    // jump into empty space.
    let mut ordered: Vec<(usize, AssembledBody)> = Vec::with_capacity(assembled.len());
    let mut deferred: Vec<(usize, AssembledBody)> = Vec::new();
    for (index, slot) in assembled.into_iter().enumerate() {
        if index == entry {
            if let Some(completed) = slot {
                ordered.push((index, completed));
            }
        } else if let Some(completed) = slot {
            deferred.push((index, completed));
        }
    }
    ordered.extend(deferred);
    image.extend_from_slice(&host_cue);
    for (slot_index, (bytes, pending)) in ordered {
        let body = image.len();
        entries[slot_index] = Some(body);
        for CrossCall { displacement, callee } in pending {
            cross.push((body + displacement, callee));
        }
        image.extend_from_slice(&bytes);
    }
    for (displacement, callee) in cross {
        let target = entries
            .get(callee)
            .copied()
            .flatten()
            .ok_or_else(|| EngineError::Codegen("cross call targets an unsupported function".to_owned()))?;
        let relative = i64::try_from(target).ok().and_then(|target| {
            i64::try_from(displacement + 4)
                .ok()
                .and_then(|after| i32::try_from(target - after).ok())
        });
        let relative: i32 =
            relative.ok_or_else(|| EngineError::Codegen("cross call exceeds rel32 range".to_owned()))?;
        let patch = image
            .get_mut(displacement..displacement + 4)
            .ok_or_else(|| EngineError::Codegen("invalid cross call relocation".to_owned()))?;
        patch.copy_from_slice(&relative.to_le_bytes());
    }
    Ok(image)
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
