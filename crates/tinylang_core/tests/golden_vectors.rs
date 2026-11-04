use tinylang::v2::*;

struct Case {
    name:            &'static str,
    bytes:           &'static [u8],
    call_status:     TinyStatus,
    response_status: Option<TinyStatus>,
}

const CASES: &[Case] = &[
    Case {
        name:            "valid_nop",
        bytes:           include_bytes!("vectors/valid_nop.bin"),
        call_status:     TINY_STATUS_OK,
        response_status: Some(TINY_STATUS_OK),
    },
    Case {
        name:            "valid_abi_info",
        bytes:           include_bytes!("vectors/valid_abi_info.bin"),
        call_status:     TINY_STATUS_OK,
        response_status: Some(TINY_STATUS_OK),
    },
    Case {
        name:            "valid_echo_values",
        bytes:           include_bytes!("vectors/valid_echo_values.bin"),
        call_status:     TINY_STATUS_OK,
        response_status: Some(TINY_STATUS_OK),
    },
    Case {
        name:            "valid_unknown_opcode",
        bytes:           include_bytes!("vectors/valid_unknown_opcode.bin"),
        call_status:     TINY_STATUS_OK,
        response_status: Some(TINY_STATUS_UNKNOWN_OPCODE),
    },
    Case {
        name:            "valid_handle_release_unknown",
        bytes:           include_bytes!("vectors/valid_handle_release_unknown.bin"),
        call_status:     TINY_STATUS_OK,
        response_status: Some(TINY_STATUS_INVALID_HANDLE),
    },
    Case {
        name:            "valid_vm_create_unknown_program",
        bytes:           include_bytes!("vectors/valid_vm_create_unknown_program.bin"),
        call_status:     TINY_STATUS_OK,
        response_status: Some(TINY_STATUS_INVALID_HANDLE),
    },
    Case {
        name:            "invalid_bad_magic",
        bytes:           include_bytes!("vectors/invalid_bad_magic.bin"),
        call_status:     TINY_STATUS_INVALID_FRAME,
        response_status: None,
    },
    Case {
        name:            "invalid_unsupported_version",
        bytes:           include_bytes!("vectors/invalid_unsupported_version.bin"),
        call_status:     TINY_STATUS_UNSUPPORTED_VERSION,
        response_status: None,
    },
    Case {
        name:            "invalid_nonzero_padding",
        bytes:           include_bytes!("vectors/invalid_nonzero_padding.bin"),
        call_status:     TINY_STATUS_INVALID_FRAME,
        response_status: None,
    },
    Case {
        name:            "invalid_truncated_header",
        bytes:           include_bytes!("vectors/invalid_truncated_header.bin"),
        call_status:     TINY_STATUS_INVALID_FRAME,
        response_status: None,
    },
    Case {
        name:            "invalid_impossible_payload_length",
        bytes:           include_bytes!("vectors/invalid_impossible_payload_length.bin"),
        call_status:     TINY_STATUS_INVALID_FRAME,
        response_status: None,
    },
];

#[test]
fn committed_vectors_match_the_raw_abi_contract() {
    let runtime = create_runtime();
    for case in CASES {
        let mut output = [0xa5u8; 256];
        let mut written = u64::MAX;
        // SAFETY: every descriptor references live, disjoint test storage.
        let status = unsafe {
            tinylang_v2_call(
                runtime,
                TinyBuffer {
                    data:   case.bytes.as_ptr(),
                    length: case.bytes.len() as u64,
                },
                TinyMutableBuffer {
                    data:     output.as_mut_ptr(),
                    capacity: output.len() as u64,
                },
                &mut written,
            )
        };
        assert_eq!(status, case.call_status, "{} call status", case.name);
        match case.response_status {
            Some(expected) => {
                assert!(written >= HEADER_BYTES as u64, "{} length", case.name);
                assert_eq!(&output[0..4], &MAGIC, "{} response magic", case.name);
                assert_eq!(read_u32(&output, 12), FLAG_RESPONSE, "{} response flags", case.name);
                assert_eq!(read_u32(&output, 16), expected, "{} response status", case.name);
                assert_eq!(read_u64(&output, 24), read_u64(case.bytes, 24), "{} correlation id", case.name);
            }
            None => assert_eq!(written, 0, "{} must not write a response", case.name),
        }
    }
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

#[test]
fn echo_vector_response_preserves_payload_bytes_exactly() {
    let runtime = create_runtime();
    let input = include_bytes!("vectors/valid_echo_values.bin");
    let mut output = [0u8; 256];
    let mut written = 0;
    // SAFETY: every descriptor references live, disjoint test storage.
    assert_eq!(
        unsafe {
            tinylang_v2_call(
                runtime,
                TinyBuffer {
                    data:   input.as_ptr(),
                    length: input.len() as u64,
                },
                TinyMutableBuffer {
                    data:     output.as_mut_ptr(),
                    capacity: output.len() as u64,
                },
                &mut written,
            )
        },
        TINY_STATUS_OK
    );
    assert_eq!(&output[HEADER_BYTES..written as usize], &input[HEADER_BYTES..]);
    assert_eq!(tinylang_v2_runtime_destroy(runtime), TINY_STATUS_OK);
}

fn create_runtime() -> TinyRuntimeHandle {
    let mut runtime = 0;
    // SAFETY: `runtime` is valid output storage.
    assert_eq!(unsafe { tinylang_v2_runtime_create(&mut runtime) }, TINY_STATUS_OK);
    runtime
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
