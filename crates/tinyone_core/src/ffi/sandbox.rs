//! Bounded request/response transport for the single sandbox worker process.

use std::io::{ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::{Result, TinyOneError};

#[cfg(test)]
mod tests;

pub(super) fn exchange(
    command: &mut Command,
    request: &[u8],
    timeout: Duration,
    response_limit: usize,
) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| TinyOneError::runtime(format!("sandbox worker could not start: {error}")))?;
    exchange_with_child(&mut child, request, timeout, response_limit)
}

/// Always terminate/reap before the scoped stdout reader is joined, including
/// early errors and unwinding. The worker protocol has one stdout writer, the
/// worker itself; killing it closes that pipe and releases a blocked read.
struct ReapedChild<'a>(&'a mut Child);

impl Drop for ReapedChild<'_> {
    fn drop(&mut self) {
        // kill may report that a successfully waited worker has already exited.
        // wait is still required on every error path to avoid a zombie.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn exchange_with_child(child: &mut Child, request: &[u8], timeout: Duration, response_limit: usize) -> Result<Vec<u8>> {
    thread::scope(|scope| {
        let child = ReapedChild(child);
        // Drop stdin after writing so the worker can finish reading its request.
        child
            .0
            .stdin
            .take()
            .ok_or_else(|| TinyOneError::runtime("sandbox worker stdin was unavailable"))?
            .write_all(request)
            .map_err(|error| TinyOneError::runtime(format!("sandbox request failed: {error}")))?;
        let deadline = Instant::now() + timeout;
        let stdout = child
            .0
            .stdout
            .take()
            .ok_or_else(|| TinyOneError::runtime("sandbox worker stdout was unavailable"))?;
        // Only one bounded result can be retained; there is no queued stream of
        // chunks or detached thread that can survive this call.
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = thread::Builder::new()
            .name("tinyone-sandbox-stdout".into())
            .spawn_scoped(scope, move || {
                let result = read_bounded_response(stdout, response_limit);
                // A dropped receiver means the supervisor has already failed.
                let _ = sender.send(result);
            })
            .map_err(|error| TinyOneError::runtime(format!("sandbox response reader could not start: {error}")))?;
        let result = wait_for_response(child.0, &receiver, deadline, timeout);
        drop(child);
        reader
            .join()
            .map_err(|_| TinyOneError::runtime("sandbox response reader panicked"))?;
        result
    })
}

fn wait_for_response(
    child: &mut Child,
    receiver: &Receiver<Result<Vec<u8>>>,
    deadline: Instant,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let mut output = None;
    loop {
        if output.is_none() {
            match receiver.try_recv() {
                Ok(result) => output = Some(result?),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    return Err(TinyOneError::runtime("sandbox response reader disconnected"));
                }
            }
        }
        if child
            .try_wait()
            .map_err(|error| TinyOneError::runtime(format!("sandbox wait failed: {error}")))?
            .is_some()
            && let Some(output) = output.take()
        {
            return Ok(output);
        }
        if Instant::now() >= deadline {
            return Err(TinyOneError::runtime(format!("sandbox execution exceeded {} seconds", timeout.as_secs())));
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn read_bounded_response(mut reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        // Read at most one byte past the remaining allowance to distinguish
        // exact-limit EOF from overflow, without retaining the excess byte.
        let remaining = limit - output.len();
        let read_len = remaining.saturating_add(1).min(chunk.len());
        let count = match reader.read(&mut chunk[..read_len]) {
            Ok(0) => return Ok(output),
            Ok(count) => count,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(TinyOneError::runtime(format!("sandbox response read failed: {error}"))),
        };
        if count > remaining {
            return Err(TinyOneError::runtime("sandbox response exceeded byte limit"));
        }
        let required = output.len() + count;
        if required > output.capacity() {
            let capacity = output
                .capacity()
                .saturating_mul(2)
                .max(chunk.len())
                .max(required)
                .min(limit);
            output
                .try_reserve_exact(capacity - output.len())
                .map_err(|error| TinyOneError::runtime(format!("sandbox response allocation failed: {error}")))?;
        }
        output.extend_from_slice(&chunk[..count]);
    }
}
