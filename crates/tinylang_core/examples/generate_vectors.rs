use std::fs;
use std::path::PathBuf;

const HEADER_BYTES: usize = 32;
const VALUE_HEADER_BYTES: usize = 16;

fn main() -> std::io::Result<()> {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("vectors");
    fs::create_dir_all(&output)?;

    let artifact_output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("artifacts");
    fs::create_dir_all(&artifact_output)?;

    let nop = request(0x0000, 0x0102_0304_0506_0708, &[]);
    write(&output, "valid_nop.bin", &nop)?;

    let abi_info = request(0x0001, 0x1112_1314_1516_1718, &[]);
    write(&output, "valid_abi_info.bin", &abi_info)?;

    let mut echo_payload = scalar_u32(0x1234_5678);
    echo_payload.extend_from_slice(&utf8_value("TinyLang v2"));
    let echo = request(0x0002, 0x2122_2324_2526_2728, &echo_payload);
    write(&output, "valid_echo_values.bin", &echo)?;

    let unknown = request(0x0fff, 0x3132_3334_3536_3738, &[]);
    write(&output, "valid_unknown_opcode.bin", &unknown)?;

    let release = request(0x1003, 0x4142_4344_4546_4748, &handle_value(0x0000_0001_0000_0001));
    write(&output, "valid_handle_release_unknown.bin", &release)?;

    let mut vm_create_payload = handle_value(0x0000_0001_0000_0001);
    vm_create_payload.extend_from_slice(&scalar_u64(1024));
    vm_create_payload.extend_from_slice(&scalar_u32(32));
    vm_create_payload.extend_from_slice(&scalar_u32(8));
    vm_create_payload.extend_from_slice(&scalar_u32(0));
    let vm_create = request(0x1001, 0x5152_5354_5556_5758, &vm_create_payload);
    write(&output, "valid_vm_create_unknown_program.bin", &vm_create)?;

    let mut bad_magic = nop.clone();
    bad_magic[0] = b'X';
    write(&output, "invalid_bad_magic.bin", &bad_magic)?;

    let mut unsupported_version = nop.clone();
    unsupported_version[4..6].copy_from_slice(&3u16.to_le_bytes());
    write(&output, "invalid_unsupported_version.bin", &unsupported_version)?;

    let mut nonzero_padding = echo.clone();
    nonzero_padding[HEADER_BYTES + VALUE_HEADER_BYTES + 4] = 1;
    write(&output, "invalid_nonzero_padding.bin", &nonzero_padding)?;

    let truncated = nop[..nop.len() - 1].to_vec();
    write(&output, "invalid_truncated_header.bin", &truncated)?;

    let mut impossible_length = nop;
    impossible_length[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    write(&output, "invalid_impossible_payload_length.bin", &impossible_length)?;

    write(&artifact_output, "minimal_halt.tlbc", &binary_artifact(0, &[(20, 0, 0)]))?;
    write(&artifact_output, "push_print_halt.tlbc", &binary_artifact(0, &[(1, 42, 0), (9, 0, 0), (20, 0, 0)]))?;
    write(
        &artifact_output,
        "arithmetic_print.tlbc",
        &binary_artifact(0, &[(1, 20, 0), (1, 22, 0), (4, 0, 0), (9, 0, 0), (20, 0, 0)]),
    )?;
    write(&artifact_output, "invalid_bad_opcode.tlbc", &binary_artifact(0, &[(u16::MAX, 0, 0)]))?;

    Ok(())
}

fn request(opcode: u16, correlation_id: u64, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; HEADER_BYTES + payload.len()];
    bytes[0..4].copy_from_slice(b"TLV2");
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    bytes[6..8].copy_from_slice(&0u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER_BYTES as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&opcode.to_le_bytes());
    bytes[20..24].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes[24..32].copy_from_slice(&correlation_id.to_le_bytes());
    bytes[HEADER_BYTES..].copy_from_slice(payload);
    bytes
}

fn scalar_u32(value: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 24];
    bytes[0..2].copy_from_slice(&4u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&4u32.to_le_bytes());
    bytes[VALUE_HEADER_BYTES..VALUE_HEADER_BYTES + 4].copy_from_slice(&value.to_le_bytes());
    bytes
}

fn scalar_u64(value: u64) -> Vec<u8> {
    let mut bytes = vec![0u8; 24];
    bytes[0..2].copy_from_slice(&5u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&8u32.to_le_bytes());
    bytes[VALUE_HEADER_BYTES..VALUE_HEADER_BYTES + 8].copy_from_slice(&value.to_le_bytes());
    bytes
}

fn utf8_value(value: &str) -> Vec<u8> {
    let value_bytes = value.as_bytes();
    let unpadded = VALUE_HEADER_BYTES + value_bytes.len();
    let padded = (unpadded + 7) & !7;
    let mut bytes = vec![0u8; padded];
    bytes[0..2].copy_from_slice(&13u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&(value_bytes.len() as u32).to_le_bytes());
    bytes[VALUE_HEADER_BYTES..unpadded].copy_from_slice(value_bytes);
    bytes
}

fn handle_value(handle: u64) -> Vec<u8> {
    let mut bytes = vec![0u8; VALUE_HEADER_BYTES];
    bytes[0..2].copy_from_slice(&14u16.to_le_bytes());
    bytes[8..16].copy_from_slice(&handle.to_le_bytes());
    bytes
}

fn binary_artifact(slot_count: u32, instructions: &[(u16, i64, i64)]) -> Vec<u8> {
    const HEADER: usize = 32;
    const DIRECTORY_ENTRY: usize = 24;
    const METADATA_OFFSET: usize = HEADER + 2 * DIRECTORY_ENTRY;
    const CODE_OFFSET: usize = METADATA_OFFSET + 16;
    const INSTRUCTION: usize = 24;

    let code_length = instructions.len() * INSTRUCTION;
    let total_length = CODE_OFFSET + code_length;
    let mut bytes = vec![0u8; total_length];
    bytes[0..4].copy_from_slice(b"TLBC");
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    bytes[8..10].copy_from_slice(&(HEADER as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&(DIRECTORY_ENTRY as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&(total_length as u32).to_le_bytes());
    bytes[16..18].copy_from_slice(&2u16.to_le_bytes());
    write_section(&mut bytes, HEADER, 1, METADATA_OFFSET, 16, 1);
    write_section(&mut bytes, HEADER + DIRECTORY_ENTRY, 2, CODE_OFFSET, code_length, instructions.len());
    bytes[METADATA_OFFSET + 4..METADATA_OFFSET + 8].copy_from_slice(&slot_count.to_le_bytes());
    for (index, (opcode, arg, arg2)) in instructions.iter().copied().enumerate() {
        let offset = CODE_OFFSET + index * INSTRUCTION;
        bytes[offset..offset + 2].copy_from_slice(&opcode.to_le_bytes());
        bytes[offset + 8..offset + 16].copy_from_slice(&arg.to_le_bytes());
        bytes[offset + 16..offset + 24].copy_from_slice(&arg2.to_le_bytes());
    }
    bytes
}

fn write_section(bytes: &mut [u8], entry: usize, kind: u16, offset: usize, length: usize, count: usize) {
    bytes[entry..entry + 2].copy_from_slice(&kind.to_le_bytes());
    bytes[entry + 4..entry + 8].copy_from_slice(&(offset as u32).to_le_bytes());
    bytes[entry + 8..entry + 12].copy_from_slice(&(length as u32).to_le_bytes());
    bytes[entry + 12..entry + 16].copy_from_slice(&(count as u32).to_le_bytes());
}

fn write(directory: &std::path::Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    fs::write(directory.join(name), bytes)
}
