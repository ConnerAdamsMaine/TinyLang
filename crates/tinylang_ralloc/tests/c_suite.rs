#[cfg(not(windows))]
use std::process::Command;

#[test]
#[cfg(not(windows))]
fn c_allocator_suite_passes_against_static_library() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let runner = format!("{manifest_dir}/tests/c/run_c_suite.sh");
    let rlib = std::env::current_exe()
        .expect("locate test executable")
        .with_file_name("libralloc.rlib");
    let original_rlib = std::fs::read(&rlib).expect("read allocator dependency before C suite");

    let output = Command::new("sh")
        .arg(&runner)
        .current_dir(manifest_dir)
        .output()
        .expect("C suite runner should execute");

    assert!(
        output.status.success(),
        "C suite failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        std::fs::read(&rlib).expect("read allocator dependency after C suite") == original_rlib,
        "C suite must not overwrite the parent build's allocator dependency at {}",
        rlib.display()
    );
}
