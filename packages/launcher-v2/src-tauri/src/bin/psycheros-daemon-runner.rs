//! Psycheros daemon runner — Job-Object-managed wrapper around deno.
//!
//! Windows Task Scheduler can't reliably tear down a process tree:
//! when `schtasks /End` terminates the action's root process, child
//! processes survive as orphans. The launcher's previous .cmd wrapper
//! exhibited this — `cmd.exe` died on /End but the deno child kept
//! running and kept holding port 3000, so Stop / Uninstall left zombie
//! daemons behind.
//!
//! The runner solves this by:
//!
//! 1. Creating a Win32 Job Object with
//!    `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
//! 2. Assigning the runner itself to the job. Children spawned from
//!    a process inside a job automatically join that job (unless they
//!    explicitly break away, which we don't allow).
//! 3. Spawning deno with `CREATE_NO_WINDOW` so no console flashes up
//!    on the user's screen at daemon start.
//! 4. Waiting for deno to exit.
//!
//! When the task is /End-ed, the OS calls `TerminateProcess` on the
//! runner. The runner's handle on the job closes; if it was the only
//! holder (which it is — the supervisor doesn't OpenJobObject from
//! elsewhere), the kernel walks the remaining job members and
//! terminates them. Deno dies. Port frees.
//!
//! The runner is also where stdout/stderr redirection lives — Task
//! Scheduler's `<Exec>` action has no native redirection, and we want
//! parity with the macOS launchd `StandardOutPath` / `StandardErrorPath`
//! flat-file logging that the launcher's log_tailer already reads.
//!
//! ## Invocation
//!
//! ```text
//! psycheros-daemon-runner.exe <deno_path> <source_dir> <stdout_log> <stderr_log> [KEY=VALUE ...]
//! ```
//!
//! - `deno_path` — absolute path to the bundled deno binary.
//! - `source_dir` — psycheros source directory; becomes deno's CWD and
//!   the directory `src/main.ts` resolves from.
//! - `stdout_log`, `stderr_log` — absolute paths to log files. Both
//!   are opened with `create+append` so logs survive across daemon
//!   restarts the same way launchd's defaults do.
//! - Remaining argv are `KEY=VALUE` env pairs to set in deno's
//!   environment. Format chosen so the launcher's supervisor can
//!   append them positionally without a quoting layer beyond the
//!   normal argv quoting Task Scheduler already does.
//!
//! Exit code is deno's exit code, or a small positive integer
//! indicating which setup step failed (see the `exit_code` values
//! below). With `#![windows_subsystem = "windows"]` there's no
//! console attached, so callers diagnose by inspecting the daemon's
//! stderr log (which the runner DOES open and redirect) or the task's
//! `Last Result` field via `schtasks /Query`.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::env;
    use std::ffi::{c_void, OsString};
    use std::fs::OpenOptions;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, CREATE_NO_WINDOW};

    /// Exit codes for setup failures. Each step that can fail before
    /// deno spawns gets a distinct code so a post-mortem read of the
    /// task's `Last Result` field is diagnosable. Deno's own exit
    /// status overwrites this if we reach the spawn.
    const E_ARGV: i32 = 64;
    const E_LOG_OPEN: i32 = 65;
    const E_JOB_CREATE: i32 = 66;
    const E_JOB_LIMIT: i32 = 67;
    const E_JOB_ASSIGN: i32 = 68;
    const E_SPAWN: i32 = 69;

    pub fn run() -> ! {
        let argv: Vec<OsString> = env::args_os().collect();
        if argv.len() < 5 {
            std::process::exit(E_ARGV);
        }
        let deno_path = &argv[1];
        let source_dir = &argv[2];
        let stdout_log = &argv[3];
        let stderr_log = &argv[4];
        let env_pairs = &argv[5..];

        // Open log files in append mode. `create(true)` lets the very
        // first run land cleanly even when the launcher hasn't seeded
        // the files; `append(true)` makes restart-after-restart
        // accumulation work the same way launchd's defaults do on
        // macOS.
        let stdout_file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(stdout_log)
        {
            Ok(f) => f,
            Err(_) => std::process::exit(E_LOG_OPEN),
        };
        let stderr_file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(stderr_log)
        {
            Ok(f) => f,
            Err(_) => std::process::exit(E_LOG_OPEN),
        };

        // Job Object setup. The two key bits:
        // - KILL_ON_JOB_CLOSE: when the last handle on the job closes
        //   (which happens when the runner process is terminated and
        //   no one else opened the job), the kernel terminates all
        //   remaining job members.
        // - We deliberately do NOT set JOB_OBJECT_LIMIT_BREAKAWAY_OK
        //   so children can't escape via CREATE_BREAKAWAY_FROM_JOB.
        //   Without that flag the OS denies breakaway attempts.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            std::process::exit(E_JOB_CREATE);
        }
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let info_size = std::mem::size_of_val(&info) as u32;
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                info_size,
            )
        };
        if ok == 0 {
            unsafe { CloseHandle(job) };
            std::process::exit(E_JOB_LIMIT);
        }
        // AssignProcessToJobObject(self) — children we spawn after
        // this point inherit job membership. The kernel walks the
        // job's member list at close time; deno gets added by the
        // CreateProcess inheritance behavior.
        let assigned = unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) };
        if assigned == 0 {
            unsafe { CloseHandle(job) };
            std::process::exit(E_JOB_ASSIGN);
        }

        // Spawn deno. CREATE_NO_WINDOW suppresses the console window
        // that would otherwise flash up momentarily. Stdio::from on
        // a File transfers ownership of the file descriptor (HANDLE)
        // to the child — std handles the duplication / inheritance
        // bookkeeping.
        let mut cmd = Command::new(deno_path);
        cmd.arg("run")
            .arg("-A")
            .arg("src\\main.ts")
            .current_dir(source_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .creation_flags(CREATE_NO_WINDOW);

        for pair in env_pairs {
            let pair_str = pair.to_string_lossy();
            if let Some((key, value)) = pair_str.split_once('=') {
                cmd.env(key, value);
            }
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => {
                unsafe { CloseHandle(job) };
                std::process::exit(E_SPAWN);
            }
        };

        // Wait. If schtasks /End hits us during this wait, the runner
        // process is terminated by TerminateProcess; this stack frame
        // never returns. The kernel then walks the job and kills deno.
        // The `child.wait()` path is for the normal "deno exits on its
        // own" case (won't happen in steady-state — deno is the
        // persistent entity — but matters during crash + restart).
        let status = child.wait();

        // Close our explicit job handle. Doing this here doesn't
        // affect the kill-on-close behavior because the runner is
        // still alive holding an implicit handle on the job (it's a
        // member); the job is reaped when the runner exits below.
        unsafe { CloseHandle(job) };

        match status {
            Ok(s) => std::process::exit(s.code().unwrap_or(0)),
            Err(_) => std::process::exit(1),
        }
    }
}

fn main() {
    #[cfg(target_os = "windows")]
    {
        windows_impl::run();
    }
    #[cfg(not(target_os = "windows"))]
    {
        // The runner is Windows-only. On other platforms the supervisor
        // doesn't invoke it and Cargo's bin target compiles to this no-op
        // so the workspace still type-checks + lints uniformly.
        eprintln!("psycheros-daemon-runner is Windows-only");
        std::process::exit(1);
    }
}
