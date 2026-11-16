use std::ffi::c_char;

unsafe extern "C" {
    fn tynra_abi_version() -> u32;
    fn tynra_free_string(value: *mut c_char);
    fn tynra_run_source_json(
        source: *const c_char,
        mode: *const c_char,
        inputs_json: *const c_char,
    ) -> *mut c_char;
}

fn main() {
    // This fixture is compile-checked in CI. Linking and execution are covered
    // by the C fixture because Rust's platform-specific cdylib link flags vary.
    let _ = (tynra_abi_version, tynra_free_string, tynra_run_source_json);
}
