//! Process-level hardening the sandbox worker applies to itself.
//!
//! The worker binary is a public C-ABI surface: an embedding host may run it
//! directly, so the playground safety contract (`Web/src/playground.rs`) is
//! reproduced here instead of trusting the spawn site. Programs executed by
//! the worker never see host environment variables: every run path passes an
//! empty `sys_env` map, and the worker's fixed deny-all policy refuses the
//! `sys_env_*` builtins. That is the v1 environment-forwarding decision; the
//! worker process itself keeps the environment it was spawned with because
//! only trusted worker code reads it.
//!
//! Resource caps mirror the playground values: a 128 MiB address-space
//! ceiling, a 6-second CPU backstop (the parent's 5-second wall-clock
//! watchdog stays the user-visible stop), no file writes (`RLIMIT_FSIZE 0`),
//! a 16-file-descriptor budget, `PR_SET_NO_NEW_PRIVS` on Linux, and a Windows
//! Job Object with the same memory ceiling and a single-process limit.

use std::path::PathBuf;

use crate::{Result, TynraError};

/// Holds every platform resource that must outlive the response write. In
/// particular the Windows job object terminates the worker when its handle
/// closes, so dropping it early is a correctness violation, not a cleanup
/// choice; `sandbox_worker_main` keeps the guard alive until process exit.
pub(crate) struct WorkerLimits {
    #[cfg(windows)]
    _job: JobObject,
}

impl WorkerLimits {
    /// Applies the platform caps before the request is read. Failure is
    /// fail-closed: the worker answers with an error response instead of
    /// serving a request with host resources unbounded.
    pub(crate) fn acquire() -> Result<Self> {
        #[cfg(unix)]
        apply_unix_limits().map_err(|error| {
            TynraError::runtime(format!("sandbox worker could not apply resource limits: {error}"))
        })?;
        #[cfg(windows)]
        let _job = JobObject::attach_current_process()?;
        Ok(Self {
            #[cfg(windows)]
            _job,
        })
    }
}

/// The `getrlimit`/`setrlimit` resource type, which differs between libc's
/// glibc and musl/Apple bindings.
#[cfg(all(unix, target_os = "linux", not(target_env = "musl")))]
type RlimitResource = libc::__rlimit_resource_t;
#[cfg(all(unix, any(target_env = "musl", not(target_os = "linux"))))]
type RlimitResource = libc::c_int;

/// `PR_SET_NO_NEW_PRIVS` from the kernel's stable `prctl(2)` ABI. `libc`
/// binds `prctl` only on some Linux targets, so the single-flag form used
/// here is declared locally for glibc and musl alike.
#[cfg(target_os = "linux")]
const PR_SET_NO_NEW_PRIVS: libc::c_int = 38;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    /// Matches glibc/musl `int prctl(int option, ...)`.
    fn prctl(option: libc::c_int, ...) -> libc::c_int;
}

#[cfg(unix)]
fn apply_unix_limits() -> std::io::Result<()> {
    // Address-space ceiling, shared with the playground contract.
    const MEMORY_LIMIT_BYTES: u64 = 128 * 1024 * 1024;
    // Kernel CPU backstop. The parent's wall-clock watchdog remains the
    // user-visible 5-second hard stop; this fires later so the parent can
    // return a clean timeout response instead of racing the kill.
    const CPU_LIMIT_SECONDS: u64 = 6;
    // File-descriptor budget for one bounded request/response exchange.
    const FILE_LIMIT: u64 = 16;
    lower_limit(libc::RLIMIT_AS, MEMORY_LIMIT_BYTES, MEMORY_LIMIT_BYTES)?;
    lower_limit(libc::RLIMIT_CPU, CPU_LIMIT_SECONDS, CPU_LIMIT_SECONDS)?;
    // No file may be created or grown by worker code: the per-run working
    // directory is removed on exit and the compile cache treats denied cache
    // writes as an optimization.
    lower_limit(libc::RLIMIT_FSIZE, 0, 0)?;
    lower_limit(libc::RLIMIT_NOFILE, FILE_LIMIT, FILE_LIMIT)?;
    #[cfg(target_os = "linux")]
    {
        // SAFETY: the variadic arguments are `unsigned long` values, exactly
        // what prctl(2) reads with va_arg, and no argument is a pointer.
        if unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1 as libc::c_ulong, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Lowers one rlimit, mirroring `Web/src/playground.rs`. A parent that
/// already confined the worker more strictly must keep working: raising a
/// hard limit is unprivileged-impossible, so each resource is left alone when
/// its current limits are already at least as strict as the request.
#[cfg(unix)]
fn lower_limit(resource: RlimitResource, soft: u64, hard: u64) -> std::io::Result<()> {
    // SAFETY: `current` and `desired` are initialized, stack-resident rlimit
    // buffers for the requested resource; getrlimit/setrlimit read or write
    // exactly that buffer.
    unsafe {
        let mut current: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(resource, &mut current) != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let already_strict = u64::from(current.rlim_cur) <= soft && u64::from(current.rlim_max) <= hard;
        if already_strict {
            return Ok(());
        }
        let desired = libc::rlimit {
            rlim_cur: soft as libc::rlim_t,
            rlim_max: hard as libc::rlim_t,
        };
        if libc::setrlimit(resource, &desired) != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(windows)]
struct JobObject(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl JobObject {
    /// Creates the job, applies the playground caps, and attaches the worker
    /// process itself as early as possible in its lifetime.
    fn attach_current_process() -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // Address-space/commit ceiling, shared with the playground contract.
        const MEMORY_LIMIT_BYTES: u64 = 128 * 1024 * 1024;
        // SAFETY: every handle is checked before use and closed exactly once
        // on each failure/success path. The information buffer is
        // zero-initialized and its byte size matches the type. The current
        // process pseudo-handle carries full access to the worker itself and
        // is documented to need no CloseHandle call.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(job_object_error("create"));
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
                | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            limits.BasicLimitInformation.ActiveProcessLimit = 1;
            limits.ProcessMemoryLimit = usize::try_from(MEMORY_LIMIT_BYTES)
                .expect("the worker memory limit fits a platform usize");
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0;
            let assigned = configured && AssignProcessToJobObject(job, GetCurrentProcess()) != 0;
            if assigned {
                return Ok(Self(job));
            }
            let error = job_object_error("apply");
            windows_sys::Win32::Foundation::CloseHandle(job);
            Err(error)
        }
    }
}

#[cfg(windows)]
fn job_object_error(stage: &str) -> TynraError {
    TynraError::runtime(format!(
        "sandbox worker could not {stage} Windows Job Object limits: {}",
        std::io::Error::last_os_error()
    ))
}

#[cfg(windows)]
impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: JobObject owns this non-null handle, and Drop runs exactly once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// Confines the worker's working directory to a per-run temporary directory
/// that is removed on exit. One worker process serves exactly one request, so
/// the guard covers the whole run.
pub(crate) struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    /// Creates the run directory and moves into it. Failure is fail-closed:
    /// without a confined working directory the worker refuses the request.
    pub(crate) fn acquire() -> Result<Self> {
        let path = run_directory_path();
        std::fs::create_dir(&path).map_err(|error| {
            TynraError::runtime(format!(
                "sandbox worker could not create its run directory {}: {error}",
                path.display()
            ))
        })?;
        if let Err(error) = std::env::set_current_dir(&path) {
            let _ = std::fs::remove_dir(&path);
            return Err(TynraError::runtime(format!(
                "sandbox worker could not confine its working directory: {error}"
            )));
        }
        Ok(Self { path })
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        // Windows refuses to delete the process's current directory, so leave
        // it first. Cleanup is deliberately best-effort: a leaked run
        // directory is a hygiene issue, never a safety issue, and removal
        // must not mask the worker's response.
        let _ = std::env::set_current_dir(std::env::temp_dir());
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// One run directory per worker process. The process id already serializes
/// concurrent workers; the timestamp distinguishes reused process ids.
fn run_directory_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    std::env::temp_dir().join(format!("tynra-sandbox-worker-{}-{nanos}", std::process::id()))
}
