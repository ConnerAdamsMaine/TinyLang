//! Generate a real compiler binary for the Python/Node SDK artifact tests.

use std::path::PathBuf;

use tinyone::{VerifiedProgram, compile_source_unoptimized_verified};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let path = PathBuf::from(
        arguments
            .next()
            .ok_or("usage: sdk_artifact PATH_TO_OUTPUT_BINARY")?,
    );
    if arguments.next().is_some() {
        return Err("usage: sdk_artifact PATH_TO_OUTPUT_BINARY".into());
    }

    let verified = compile_source_unoptimized_verified("fn add(a, b) { return a + b }")?;
    let binary = verified.program().to_binary_artifact()?;
    let reloaded = VerifiedProgram::from_binary_artifact(&binary)?;
    match reloaded.program().functions() {
        [function] if function.name() == "add" && function.param_count() == 2 => {}
        _ => return Err("SDK fixture must export only add(a, b) at function-table index 0".into()),
    }
    if binary.get(..10) != Some(b"TINYONEB\x05\x00".as_slice()) {
        return Err("SDK fixture requires compiler binary version 5".into());
    }
    std::fs::write(&path, &binary)?;
    println!(
        "SDK compiler artifact: compile_source_unoptimized_verified -> to_binary_artifact -> \
         untrusted verification=OK version=5 bytes={} export_id=0 name=add path={}",
        binary.len(),
        path.display()
    );
    Ok(())
}
