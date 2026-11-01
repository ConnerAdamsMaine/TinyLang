use tinyone::{Instr, Op, Program, VerifiedProgram};

use super::schema::{ArtifactFormat, MAX_ARTIFACT_BYTES, ProgramLoad};

pub const ARTIFACT_MAGIC: [u8; 4] = *b"TLBC";
pub const ARTIFACT_MAJOR: u16 = 2;
pub const ARTIFACT_MINOR: u16 = 0;
pub const ARTIFACT_HEADER_BYTES: usize = 32;
pub const SECTION_ENTRY_BYTES: usize = 24;
pub const INSTRUCTION_BYTES: usize = 24;
pub const SECTION_METADATA: u16 = 1;
pub const SECTION_CODE: u16 = 2;
pub const PROFILE_MAIN_ONLY: u32 = 0;

const PROFILE_SECTION_COUNT: usize = 2;
const METADATA_BYTES: usize = 16;
const MAX_INSTRUCTIONS: usize = 262_144;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactError {
    Malformed,
    UnsupportedVersion,
    UnsupportedProfile,
    VerificationFailed,
}

#[derive(Clone, Copy)]
struct Section {
    kind: u16,
    offset: usize,
    length: usize,
    item_count: usize,
}

pub(super) fn decode_request(request: ProgramLoad<'_>) -> Result<VerifiedProgram, ArtifactError> {
    match request.artifact_format {
        ArtifactFormat::Tlbc => decode(request.artifact),
        // Foreign bytes never authenticate serialized host authority. Keep
        // verification and compiler format/version limits in the owning decoder.
        ArtifactFormat::CompilerBinary => VerifiedProgram::from_binary_artifact(request.artifact)
            .map_err(|_| ArtifactError::VerificationFailed),
    }
}

pub fn decode(bytes: &[u8]) -> Result<VerifiedProgram, ArtifactError> {
    if bytes.len() < ARTIFACT_HEADER_BYTES || bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(ArtifactError::Malformed);
    }
    if bytes[0..4] != ARTIFACT_MAGIC {
        return Err(ArtifactError::Malformed);
    }
    if read_u16(bytes, 4) != ARTIFACT_MAJOR || read_u16(bytes, 6) > ARTIFACT_MINOR {
        return Err(ArtifactError::UnsupportedVersion);
    }
    if usize::from(read_u16(bytes, 8)) != ARTIFACT_HEADER_BYTES
        || usize::from(read_u16(bytes, 10)) != SECTION_ENTRY_BYTES
        || read_u16(bytes, 18) != 0
        || read_u32(bytes, 20) != 0
        || read_u64(bytes, 24) != 0
    {
        return Err(ArtifactError::Malformed);
    }
    let total_length = read_u32(bytes, 12) as usize;
    let section_count = usize::from(read_u16(bytes, 16));
    if total_length != bytes.len() || section_count != PROFILE_SECTION_COUNT {
        return Err(ArtifactError::UnsupportedProfile);
    }
    let directory_end = ARTIFACT_HEADER_BYTES
        .checked_add(
            section_count
                .checked_mul(SECTION_ENTRY_BYTES)
                .ok_or(ArtifactError::Malformed)?,
        )
        .ok_or(ArtifactError::Malformed)?;
    if directory_end > bytes.len() {
        return Err(ArtifactError::Malformed);
    }
    let metadata = parse_section(bytes, ARTIFACT_HEADER_BYTES)?;
    let code = parse_section(bytes, ARTIFACT_HEADER_BYTES + SECTION_ENTRY_BYTES)?;
    if metadata.kind != SECTION_METADATA
        || code.kind != SECTION_CODE
        || metadata.offset != directory_end
        || metadata.length != METADATA_BYTES
        || metadata.item_count != 1
        || code.offset != metadata.offset + metadata.length
        || code.offset % 8 != 0
        || code.length % INSTRUCTION_BYTES != 0
        || code.item_count != code.length / INSTRUCTION_BYTES
        || code.item_count == 0
        || code.item_count > MAX_INSTRUCTIONS
        || code.offset.checked_add(code.length) != Some(bytes.len())
    {
        return Err(ArtifactError::Malformed);
    }

    let metadata_bytes = &bytes[metadata.offset..metadata.offset + metadata.length];
    let profile = read_u32(metadata_bytes, 0);
    if profile != PROFILE_MAIN_ONLY {
        return Err(ArtifactError::UnsupportedProfile);
    }
    if read_u64(metadata_bytes, 8) != 0 {
        return Err(ArtifactError::Malformed);
    }
    let slot_count = read_u32(metadata_bytes, 4) as usize;

    let mut instructions = Vec::with_capacity(code.item_count);
    for index in 0..code.item_count {
        let offset = code.offset + index * INSTRUCTION_BYTES;
        let record = &bytes[offset..offset + INSTRUCTION_BYTES];
        if read_u16(record, 2) != 0 || read_u32(record, 4) != 0 {
            return Err(ArtifactError::Malformed);
        }
        let op = decode_op(read_u16(record, 0)).ok_or(ArtifactError::Malformed)?;
        let arg = read_i64(record, 8);
        let arg2 = read_i64(record, 16);
        instructions.push(Instr::new(op, arg, arg2));
    }

    VerifiedProgram::verify(Program::new(instructions, slot_count))
        .map_err(|_| ArtifactError::VerificationFailed)
}

fn parse_section(bytes: &[u8], offset: usize) -> Result<Section, ArtifactError> {
    let end = offset
        .checked_add(SECTION_ENTRY_BYTES)
        .ok_or(ArtifactError::Malformed)?;
    if end > bytes.len() {
        return Err(ArtifactError::Malformed);
    }
    let entry = &bytes[offset..end];
    if read_u16(entry, 2) != 0 || read_u64(entry, 16) != 0 {
        return Err(ArtifactError::Malformed);
    }
    Ok(Section {
        kind: read_u16(entry, 0),
        offset: read_u32(entry, 4) as usize,
        length: read_u32(entry, 8) as usize,
        item_count: read_u32(entry, 12) as usize,
    })
}

fn decode_op(opcode: u16) -> Option<Op> {
    Some(match opcode {
        1 => Op::PushInt,
        2 => Op::Load,
        3 => Op::Store,
        4 => Op::Add,
        5 => Op::Sub,
        6 => Op::Mul,
        7 => Op::Div,
        8 => Op::Neg,
        9 => Op::Print,
        10 => Op::Lt,
        11 => Op::Lte,
        12 => Op::Gt,
        13 => Op::Gte,
        14 => Op::Eq,
        15 => Op::Ne,
        16 => Op::Jump,
        17 => Op::JumpIfZero,
        18 => Op::Call,
        19 => Op::Return,
        20 => Op::Halt,
        21 => Op::PushString,
        22 => Op::MakeArray,
        23 => Op::Index,
        24 => Op::SetIndex,
        25 => Op::MakeStruct,
        26 => Op::GetField,
        27 => Op::SetField,
        28 => Op::Builtin,
        29 => Op::PushNull,
        30 => Op::Pop,
        31 => Op::LoadGlobal,
        32 => Op::MakeEnum,
        33 => Op::PushBool,
        34 => Op::PushFloat,
        35 => Op::PushFunction,
        36 => Op::CallValue,
        _ => return None,
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("bounded read"))
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("bounded read"))
}

fn read_i64(bytes: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("bounded read"))
}

#[cfg(test)]
mod compiler_tests {
    use super::*;
    use tinyone::{RuntimeValue, VM, VmExecutionLimits};

    #[test]
    fn compiler_adapter_preserves_functions_and_pools_without_granting_host_authority() {
        let source = tinyone::compile_source(
            "fn answer() { return 42 } fn text_len() { return len(\"hello\") } \
             fn count() { return len([1, 2]) } fn probe() { return fs_exists(\".\") }",
        )
        .unwrap();
        let bytes = source.to_binary_artifact().unwrap();
        let program = decode_request(ProgramLoad {
            artifact_format: ArtifactFormat::CompilerBinary,
            artifact: &bytes,
        })
        .unwrap();
        assert_eq!(
            source.functions().len(),
            program.program().functions().len()
        );
        let mut vm =
            VM::from_verified_with_limits(&program, vec![], VmExecutionLimits::default()).unwrap();
        for (name, expected) in [("answer", 42), ("text_len", 5), ("count", 2)] {
            let index = program
                .program()
                .functions()
                .iter()
                .position(|f| f.name() == name)
                .unwrap();
            assert_eq!(
                vm.invoke_function_with_fuel(index, vec![], &mut Vec::new(), 100)
                    .unwrap(),
                RuntimeValue::I64(expected)
            );
        }
        let probe = program
            .program()
            .functions()
            .iter()
            .position(|f| f.name() == "probe")
            .unwrap();
        let error = vm
            .invoke_function_with_fuel(probe, vec![], &mut Vec::new(), 100)
            .unwrap_err();
        assert!(
            error.to_string().contains("requires \"filesystem\""),
            "{error}"
        );
    }
}
