//! Pins the exported C ABI symbol set of the `tynra` CDyLib to the
//! documented contract: 9 frozen v1 JSON entry points plus 6 v2 binary ABI
//! entry points, declared in `include/tynra.h` and `include/tynra_v2.h`.
//!
//! The test parses the actual `#[no_mangle] extern "C" fn` declarations out of
//! the FFI sources, so adding, removing, or renaming a symbol without a
//! deliberate contract update fails here.

const V1_JSON_FFI: &str = include_str!("../src/interoperability/ffi.rs");
const V2_BINARY_FFI: &str = include_str!("../src/interoperability/ffi/v2/mod.rs");

/// Sorted, frozen v1 JSON C ABI surface (versioned by `tynra_abi_version`).
const EXPECTED_V1_JSON_SYMBOLS: [&str; 9] = [
    "tynra_abi_version",
    "tynra_compile_file_json",
    "tynra_compile_source_json",
    "tynra_free_string",
    "tynra_jit_listing_json",
    "tynra_lex_source_json",
    "tynra_run_artifact_json",
    "tynra_run_file_json",
    "tynra_run_source_json",
];

/// Sorted, separately versioned v2 binary C ABI surface (versioned by
/// `tynra_v2_abi_version`).
const EXPECTED_V2_BINARY_SYMBOLS: [&str; 6] = [
    "tynra_v2_abi_version",
    "tynra_v2_buffer_acquire",
    "tynra_v2_buffer_release",
    "tynra_v2_call",
    "tynra_v2_runtime_create",
    "tynra_v2_runtime_destroy",
];

/// Collects the identifiers of every `#[no_mangle] pub ... extern "C" fn` in
/// one FFI source file.
fn no_mangle_extern_c_symbols(source: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut rest = source;
    while let Some(position) = rest.find("extern \"C\" fn ") {
        let tail = &rest[position + "extern \"C\" fn ".len() ..];
        let preceding = &source[.. source.len() - tail.len()];
        let window = &preceding[preceding.len().saturating_sub(120) ..];
        let name: String = tail
            .chars()
            .take_while(|character| *character == '_' || character.is_ascii_alphanumeric())
            .collect();
        if window.contains("no_mangle") && !name.is_empty() {
            symbols.push(name);
        }
        rest = tail;
    }
    symbols.sort();
    symbols.dedup();
    symbols
}

fn assert_symbol_set(source: &str, expected: &[&str]) {
    let found = no_mangle_extern_c_symbols(source);
    let unexpected: Vec<&String> = found
        .iter()
        .filter(|symbol| !expected.contains(&symbol.as_str()))
        .collect();
    let missing: Vec<&&str> = expected
        .iter()
        .filter(|symbol| !found.iter().any(|found| found == *symbol))
        .collect();
    assert!(
        unexpected.is_empty() && missing.is_empty(),
        "C ABI symbol contract drift; unexpected: {unexpected:?}, missing: {missing:?}"
    );
}

#[test]
fn v1_json_abi_exports_exactly_the_frozen_nine_symbols() {
    assert_symbol_set(V1_JSON_FFI, &EXPECTED_V1_JSON_SYMBOLS);
}

#[test]
fn v2_binary_abi_exports_exactly_the_six_documented_symbols() {
    assert_symbol_set(V2_BINARY_FFI, &EXPECTED_V2_BINARY_SYMBOLS);
}

#[test]
fn v1_and_v2_surfaces_do_not_overlap_and_total_fifteen_symbols() {
    let total = EXPECTED_V1_JSON_SYMBOLS.len() + EXPECTED_V2_BINARY_SYMBOLS.len();
    assert_eq!(total, 15);
    let shared: Vec<&&str> = EXPECTED_V1_JSON_SYMBOLS
        .iter()
        .filter(|symbol| EXPECTED_V2_BINARY_SYMBOLS.contains(symbol))
        .collect();
    assert!(shared.is_empty(), "v1 and v2 ABI surfaces overlap: {shared:?}");
}
