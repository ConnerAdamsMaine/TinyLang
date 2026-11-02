use std::io::{self, Cursor, Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

use super::{exchange_with_child, read_bounded_response};

const FIXTURE_ENV: &str = "TINYLANG_SANDBOX_PIPE_FIXTURE";

#[test]
fn sandbox_child_fixture() {
    let Ok(kind) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    match kind.as_str() {
        "normal" => {
            io::stdout()
                .write_all(b"bounded-fixture-success")
                .expect("fixture output");
            io::stdout().flush().expect("flush fixture output");
            std::process::exit(0);
        }
        "oversized" => {
            let _ = io::stdout().write_all(&[b'x'; 65_536]);
        }
        "wait" => {}
        _ => panic!("unknown pipe fixture"),
    }
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn fixture(kind: &str) -> Child {
    Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", "ffi::sandbox::tests::sandbox_child_fixture", "--nocapture"])
        .env(FIXTURE_ENV, kind)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn pipe fixture")
}

#[test]
fn exact_response_limit_is_accepted_without_excess_capacity() {
    let output =
        read_bounded_response(Cursor::new(vec![b'x'; 16 * 1024 * 1024]), 16 * 1024 * 1024).expect("exact limit");
    assert_eq!(output.len(), 16 * 1024 * 1024);
    assert!(output.capacity() <= 16 * 1024 * 1024);
    assert!(output.iter().all(|byte| *byte == b'x'));
}

#[test]
fn one_byte_over_response_limit_is_rejected() {
    let error = read_bounded_response(Cursor::new(vec![b'x'; 16 * 1024 * 1024 + 1]), 16 * 1024 * 1024)
        .expect_err("oversized response");
    assert!(error.to_string().contains("sandbox response exceeded byte limit"));
}

#[test]
fn zero_limit_accepts_only_empty_response() {
    assert!(
        read_bounded_response(io::empty(), 0)
            .expect("empty response")
            .is_empty()
    );
    assert!(read_bounded_response(Cursor::new(b"x"), 0).is_err());
}

struct BrokenReader;

impl Read for BrokenReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("synthetic read failure"))
    }
}

#[test]
fn response_read_error_is_reported() {
    let error = read_bounded_response(BrokenReader, 1024).expect_err("read failure");
    assert!(
        error
            .to_string()
            .contains("sandbox response read failed: synthetic read failure")
    );
}

#[test]
fn successful_worker_is_reaped() {
    let mut child = fixture("normal");
    let output = exchange_with_child(&mut child, b"", Duration::from_secs(5), 8192).expect("normal response");
    assert!(output.ends_with(b"bounded-fixture-success"));
    assert!(child.try_wait().expect("cached child exit").is_some());
}

#[test]
fn response_limit_error_kills_and_reaps_worker() {
    let mut child = fixture("oversized");
    let error = exchange_with_child(&mut child, b"", Duration::from_secs(5), 8192).expect_err("oversized response");
    assert!(error.to_string().contains("sandbox response exceeded byte limit"));
    assert!(child.try_wait().expect("cached child exit").is_some());
}

#[test]
fn timeout_kills_and_reaps_worker() {
    let mut child = fixture("wait");
    let error = exchange_with_child(&mut child, b"", Duration::from_millis(100), 8192).expect_err("timeout");
    assert!(error.to_string().contains("sandbox execution exceeded"));
    assert!(child.try_wait().expect("cached child exit").is_some());
}

#[test]
fn missing_request_pipe_kills_and_reaps_worker() {
    let mut child = fixture("wait");
    drop(child.stdin.take());
    let error = exchange_with_child(&mut child, b"", Duration::from_secs(5), 8192).expect_err("missing stdin");
    assert!(error.to_string().contains("sandbox worker stdin was unavailable"));
    assert!(child.try_wait().expect("cached child exit").is_some());
}
