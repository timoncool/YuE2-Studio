//! Children that do not outlive the studio.
//!
//! The engine and the assistant sidecar are separate processes. Dropping their
//! supervisor kills them politely, but a supervisor does not always get to
//! run: a hard kill of the studio, a crash, a taskkill from the task manager -
//! and `yue-server` is left holding the GPU with nobody to talk to it.
//!
//! Windows answers this with a job object carrying
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: every child spawned into it dies when
//! the last handle to the job closes, which the system does for us when this
//! process ends, however it ends.
//!
//! Linux has no job objects. There the answer is `PR_SET_PDEATHSIG`, set in the
//! forked child before it execs - but that signal is bound to the *thread* that
//! made the fork, not to the process (see `ensure_dies_with_parent`), so a Linux
//! child is spawned from a thread of our own that is never retired:
//! `spawn_dying_with_parent`. This module's `adopt` therefore does nothing off
//! Windows - it is the job-object side of the binding, not the only one.

use std::process::Child;

/// Puts a freshly spawned child into the studio's job, so it cannot survive us.
///
/// Failing to do so is not fatal - the child is still supervised normally -
/// so this never returns an error, it simply does what it can.
#[cfg(windows)]
pub fn adopt(child: &Child) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = job();
    if job == 0 {
        return;
    }
    unsafe {
        AssignProcessToJobObject(job as _, child.as_raw_handle() as _);
    }
}

#[cfg(not(windows))]
pub fn adopt(_child: &Child) {}

/// `adopt` for a raw handle, for children a tokio spawn owns.
#[cfg(windows)]
pub fn adopt_handle(handle: isize) {
    use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

    let job = job();
    if job == 0 || handle == 0 {
        return;
    }
    unsafe {
        AssignProcessToJobObject(job as _, handle as _);
    }
}

#[cfg(not(windows))]
pub fn adopt_handle(_handle: isize) {}

/// Arms the parent-death signal on the child, set inside `pre_exec`.
///
/// The invariant this rests on: `spawn_dying_with_parent` below is its only
/// caller, and it calls this from the thread that spawns the child.
/// `PR_SET_PDEATHSIG` is per-*thread* - the kernel sends the signal when the
/// thread that made the fork terminates, not when the process does - so arming
/// it from a thread that retires SIGKILLs a perfectly healthy child. That is
/// what `spawn_on_the_thread_that_never_retires` is for; see issue #246.
///
/// Linux only: Windows binds the child with a job object in `adopt` once it
/// exists, and macOS has no parent-death signal.
#[cfg(target_os = "linux")]
pub(crate) fn ensure_dies_with_parent(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

    let parent = std::process::id() as libc::pid_t;
    // `PR_SET_PDEATHSIG` is cleared across a successful `execve`, so it has to
    // be set in the forked child, which is what `pre_exec` brackets. Everything
    // inside is async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            // SIGKILL rather than SIGTERM: a model server holds no unsaved
            // state, and a signal it may handle slowly (the engine ignores
            // SIGTERM mid-inference) would leave exactly the orphan this
            // exists to prevent.
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The parent may have died between fork and the call above, in
            // which case the signal will never come. Noticing turns a quiet
            // orphan into a failed start the supervisor can retry.
            if libc::getppid() != parent {
                return Err(std::io::Error::other("the studio exited while the child was starting"));
            }
            Ok(())
        });
    }
}

/// Spawns `command` so the child cannot outlive this process, however it ends.
///
/// The engine and the assistant sidecar go through here instead of
/// `Command::spawn`: their supervisor is not enough when the studio is killed
/// rather than closed, which is the case this exists for.
///
/// Windows does the binding after the child exists, with the job object `adopt`
/// assigns; macOS leaves it to the supervisor. Linux uses `PR_SET_PDEATHSIG`,
/// and that signal is per-thread, so the spawn happens on
/// `spawn_on_the_thread_that_never_retires` below.
pub fn spawn_dying_with_parent(command: std::process::Command) -> std::io::Result<Child> {
    #[cfg(target_os = "linux")]
    {
        spawn_on_the_thread_that_never_retires(command)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut command = command;
        command.spawn()
    }
}

/// Spawns `command` from a thread that is never retired, so the parent-death
/// signal set in the child refers to a thread whose termination really does mean
/// "this process is going away".
///
/// One thread serves every child: it is created the first time one is spawned and
/// then blocks on a channel for the rest of the process' life. Its termination at
/// exit is what makes the kernel send the signal - which is why the loop below has
/// no exit.
#[cfg(target_os = "linux")]
fn spawn_on_the_thread_that_never_retires(command: std::process::Command) -> std::io::Result<Child> {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::mpsc::{self, Sender};
    use std::sync::OnceLock;

    type SpawnRequest = (std::process::Command, Sender<std::io::Result<Child>>);

    static SPAWNER: OnceLock<Sender<SpawnRequest>> = OnceLock::new();

    let spawner = SPAWNER.get_or_init(|| {
        let (spawner, requests) = mpsc::channel::<SpawnRequest>();
        let thread = std::thread::Builder::new().name("child-spawner".into()).spawn(move || {
            for (mut command, answer) in requests {
                // Armed here rather than at the call site: this is the thread that
                // forks, so this is the thread the signal will be tied to. The
                // catch is not belt and braces: a panic here would unwind the loop
                // and take every child already spawned with it, all of them bound
                // to this thread's death, so it becomes an error for the one caller
                // that asked instead.
                let spawned = catch_unwind(AssertUnwindSafe(|| {
                    ensure_dies_with_parent(&mut command);
                    command.spawn()
                }))
                .unwrap_or_else(|_| Err(std::io::Error::other("panicked while starting a child")));
                let _ = answer.send(spawned);
            }
        });
        match thread {
            Ok(_) => spawner,
            // No thread to spawn from. Dropping the receiver makes every send
            // below fail, and the caller spawns in place without the signal -
            // better no binding than one tied to a thread that retires - and says
            // so, because the child is now unbound for the rest of the process.
            Err(error) => {
                eprintln!(
                    "yue2-studio: no thread to spawn children from ({error}); they will not be bound to this process' lifetime"
                );
                let (spawner, requests) = mpsc::channel::<SpawnRequest>();
                drop(requests);
                spawner
            }
        }
    });

    let (answer, answered) = mpsc::channel();
    match spawner.send((command, answer)) {
        Ok(()) => answered.recv().unwrap_or_else(|_| {
            Err(std::io::Error::other("the child could not be spawned: the thread that spawns it died"))
        }),
        Err(mpsc::SendError((mut command, _))) => {
            eprintln!(
                "yue2-studio: the thread that spawns children is gone; starting this child unbound from the process' lifetime"
            );
            command.spawn()
        }
    }
}

/// The library search path a bundled program starts with on Linux: its own folder first, so it loads the libraries
/// beside it and not another version of the same name an AppImage put on the inherited path (the MIDI tool's older
/// ggml in `usr/lib`). None on other systems, where nothing is inherited this way.
pub fn own_libraries_first(program: impl AsRef<std::path::Path>) -> Option<std::ffi::OsString> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let mut paths = vec![program.as_ref().parent()?.to_path_buf()];
    if let Some(inherited) = std::env::var_os("LD_LIBRARY_PATH") {
        paths.extend(std::env::split_paths(&inherited));
    }
    std::env::join_paths(paths).ok()
}


/// The C++ runtime a program built with Visual Studio asks Windows for.
const RUNTIME_LIBRARIES: [&str; 5] = ["msvcp140.dll", "msvcp140_1.dll", "vcruntime140.dll", "vcruntime140_1.dll", "vcomp140.dll"];

static RUNTIME_SOURCE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Names the folder whose C++ runtime goes beside the programs the studio starts: the engine's, which the installer
/// brings whole.
pub fn runtime_libraries_from(folder: std::path::PathBuf) {
    let _ = RUNTIME_SOURCE.set(folder);
}

/// A program downloaded without the C++ runtime it was built with takes the one Windows has, and with an older one
/// it stops at its first lock with an access violation and not a word (0xc0000005). The studio's own runtime goes
/// beside it, where Windows looks first; what the program brought itself stays.
pub fn runtime_beside(program: impl AsRef<std::path::Path>) {
    if !cfg!(windows) {
        return;
    }
    let (Some(source), Some(folder)) = (RUNTIME_SOURCE.get(), program.as_ref().parent()) else { return };
    if let Err(error) = bring_missing(source, folder, &RUNTIME_LIBRARIES) {
        eprintln!("[ERROR] the C++ runtime was not put beside {}: {error}", program.as_ref().display());
    }
}

/// Copies into `folder` each of `names` that `source` has and `folder` lacks; says how many it copied. A library
/// arrives under another name and takes its own only whole, so a copy cut short is never what a program loads.
fn bring_missing(source: &std::path::Path, folder: &std::path::Path, names: &[&str]) -> std::io::Result<usize> {
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut brought = 0;
    for name in names {
        let (from, to) = (source.join(name), folder.join(name));
        if from.is_file() && !to.exists() {
            let arriving = folder.join(format!("{name}.arriving"));
            std::fs::copy(&from, &arriving)?;
            std::fs::rename(&arriving, &to)?;
            brought += 1;
        }
    }
    Ok(brought)
}

/// Where a child that crashed without a word went down, as Windows recorded it: the module the fault happened in
/// (a file name, never a path), the exception code and the offset into that module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub module: String,
    pub exception: String,
    pub offset: String,
}

/// Whether a Windows exit code is an exception (0xC0000000 and up: an access violation, an illegal instruction, a
/// failed library load) - the exits Windows keeps a record of. A program that returns an error code of its own has
/// none, and waiting for one would only slow its error down.
pub fn exited_on_exception(code: Option<i32>) -> bool {
    code.is_some_and(|code| (code as u32) >= 0xC000_0000)
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "faulting module {}, exception 0x{}, offset 0x{}", self.module, self.exception, self.offset)
    }
}

/// The Windows Error Reporting record (Application log, Application Error, event 1000) of the process `pid`, which
/// Windows writes a moment after the crash: looked for until `wait` runs out. None anywhere else, and when Windows
/// kept no record (reporting turned off, or the process ended without a fault).
pub fn fault_of(pid: u32, wait: std::time::Duration) -> Option<Fault> {
    #[cfg(windows)]
    {
        let deadline = std::time::Instant::now() + wait;
        loop {
            if let Some(fault) = latest_faults().into_iter().find_map(|xml| fault_in_event(&xml, pid)) {
                return Some(fault);
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (pid, wait);
        None
    }
}

/// The Application Error records of the last five minutes, newest first, as event XML.
#[cfg(windows)]
fn latest_faults() -> Vec<String> {
    use windows_sys::Win32::System::EventLog::{EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender, EvtRenderEventXml};

    let wide = |text: &str| text.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let channel = wide("Application");
    let query = wide("*[System[Provider[@Name='Application Error'] and (EventID=1000) and TimeCreated[timediff(@SystemTime) <= 300000]]]");
    let mut found = Vec::new();
    unsafe {
        let results = EvtQuery(0, channel.as_ptr(), query.as_ptr(), EvtQueryChannelPath | EvtQueryReverseDirection);
        if results == 0 {
            return found;
        }
        let mut events = [0isize; 16];
        let mut returned = 0u32;
        if EvtNext(results, events.len() as u32, events.as_mut_ptr(), 1000, 0, &mut returned) != 0 {
            for &event in &events[..returned as usize] {
                let mut used = 0u32;
                let mut properties = 0u32;
                EvtRender(0, event, EvtRenderEventXml, 0, std::ptr::null_mut(), &mut used, &mut properties);
                let mut buffer = vec![0u16; (used as usize).div_ceil(2) + 1];
                if EvtRender(0, event, EvtRenderEventXml, (buffer.len() * 2) as u32, buffer.as_mut_ptr().cast(), &mut used, &mut properties) != 0 {
                    let end = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
                    found.push(String::from_utf16_lossy(&buffer[..end]));
                }
                EvtClose(event);
            }
        }
        EvtClose(results);
    }
    found
}

/// The fault in one Application Error record when it is the one of `pid`.
fn fault_in_event(xml: &str, pid: u32) -> Option<Fault> {
    let data = |name: &str| -> Option<String> {
        let open = format!("<Data Name='{name}'>");
        let start = xml.find(&open)? + open.len();
        let end = xml[start..].find("</Data>")? + start;
        Some(xml[start..end].trim().to_string())
    };
    let recorded = data("ProcessId")?;
    let recorded = u32::from_str_radix(recorded.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()?;
    if recorded != pid {
        return None;
    }
    // the module's own name only: its path can carry the user's name
    let module = data("ModuleName")?;
    let module = module.rsplit(['\\', '/']).next().unwrap_or(&module).to_string();
    Some(Fault { module, exception: data("ExceptionCode")?, offset: data("FaultingOffset")? })
}

/// One job for the whole process, created the first time a child needs it.
#[cfg(windows)]
fn job() -> isize {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    static JOB: OnceLock<isize> = OnceLock::new();
    *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return 0;
        }
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        job as isize
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn a_child_in_the_job_dies_with_a_closed_job() {
        // The job is per process and cannot be closed here without taking the
        // test runner with it, so this only proves adoption is accepted for a
        // real child - the kill-on-close flag is the system's part.
        let mut child = std::process::Command::new("cmd").args(["/c", "timeout /t 5"]).spawn().expect("spawn a child");
        adopt(&child);
        child.kill().ok();
        child.wait().ok();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_child_outlives_the_thread_that_asked_for_it() {
        let asked = std::thread::spawn(|| {
            let mut command = std::process::Command::new("sleep");
            command.arg("30");
            spawn_dying_with_parent(command).expect("spawn a child")
        });
        let mut child = asked.join().expect("the thread that asked");
        std::thread::sleep(std::time::Duration::from_millis(500));
        let alive = child.try_wait().expect("the child's state").is_none();
        child.kill().ok();
        child.wait().ok();
        assert!(alive, "the child was killed when the thread that asked for it ended");
    }

    #[test]
    fn a_program_gets_the_runtime_it_lacks_and_keeps_the_one_it_brought() {
        let root = std::env::temp_dir().join(format!("music-core-runtime-{}", std::process::id()));
        let (source, folder) = (root.join("engine"), root.join("tool"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(source.join("msvcp140.dll"), b"the studio's").unwrap();
        std::fs::write(source.join("vcruntime140.dll"), b"the studio's").unwrap();
        std::fs::write(folder.join("vcruntime140.dll"), b"the tool's own").unwrap();
        let names = ["msvcp140.dll", "vcruntime140.dll", "vcomp140.dll"];
        assert_eq!(bring_missing(&source, &folder, &names).unwrap(), 1, "one was missing and the source had it");
        assert_eq!(std::fs::read(folder.join("msvcp140.dll")).unwrap(), b"the studio's");
        assert_eq!(std::fs::read(folder.join("vcruntime140.dll")).unwrap(), b"the tool's own");
        assert!(!folder.join("vcomp140.dll").exists(), "what the source lacks is not made up");
        assert!(!folder.join("msvcp140.dll.arriving").exists());
        assert_eq!(bring_missing(&source, &folder, &names).unwrap(), 0, "a second start copies nothing");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_crash_record_gives_the_module_the_code_and_the_offset_of_its_own_process() {
        let xml = "<Event><EventData><Data Name='AppName'>crash.exe</Data><Data Name='ModuleName'>ggml-cpu-haswell.dll</Data><Data Name='ExceptionCode'>c000001d</Data><Data Name='FaultingOffset'>0000000000001000</Data><Data Name='ProcessId'>0x17918</Data><Data Name='AppPath'>C:\\Users\\someone\\crash.exe</Data><Data Name='ModulePath'>C:\\Users\\someone\\ggml-cpu-haswell.dll</Data></EventData></Event>";
        let fault = fault_in_event(xml, 0x17918).expect("the record of this process");
        assert_eq!(fault, Fault { module: "ggml-cpu-haswell.dll".into(), exception: "c000001d".into(), offset: "0000000000001000".into() });
        assert_eq!(fault.to_string(), "faulting module ggml-cpu-haswell.dll, exception 0xc000001d, offset 0x0000000000001000");
        assert!(!fault.to_string().contains("someone"), "no path, so no user name");
        assert_eq!(fault_in_event(xml, 42), None, "another process's record is not this one's");
        assert!(exited_on_exception(Some(0xC000_0005u32 as i32)), "an access violation");
        assert!(exited_on_exception(Some(0xC000_001Du32 as i32)), "an illegal instruction");
        assert!(!exited_on_exception(Some(1)), "an error code of the program's own");
        assert!(!exited_on_exception(None), "killed by a signal, no code");
    }

    /// Run with `MUSIC_CORE_CRASHER` naming a program that crashes on start (a null write): Windows' record of it
    /// is found by its process id.
    #[test]
    #[ignore]
    #[cfg(windows)]
    fn a_real_crash_is_found_in_the_application_log() {
        let crasher = std::env::var("MUSIC_CORE_CRASHER").expect("MUSIC_CORE_CRASHER names a program that crashes");
        let mut child = std::process::Command::new(&crasher).spawn().expect("start the crasher");
        let pid = child.id();
        let status = child.wait().expect("the crasher ends");
        assert!(!status.success());
        let fault = fault_of(pid, std::time::Duration::from_secs(10)).expect("Windows recorded the crash");
        assert_eq!(fault.exception, "c0000005");
        println!("{fault}");
    }
}
