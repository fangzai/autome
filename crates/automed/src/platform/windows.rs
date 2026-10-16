//! Windows session Jobs. Assign the suspended wrapper before it can fork.
//! The independent keeper holds the named Job across core restarts; Stop waits
//! for process handles because terminating a Job does not synchronously reap it.
//! Reference: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects

use std::ffi::c_void;
type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    name: [u16; 260],
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn WaitForSingleObject(handle: Handle, millis: u32) -> u32;
    fn GetProcessTimes(
        handle: Handle,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
}
#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    tid: u32,
    owner: u32,
    priority: i32,
    delta: i32,
    flags: u32,
}
#[repr(C)]
#[derive(Default)]
struct Accounting {
    user: i64,
    kernel: i64,
    period_user: i64,
    period_kernel: i64,
    faults: u32,
    total: u32,
    active: u32,
    terminated: u32,
}
#[repr(C)]
struct JobProcesses {
    assigned: u32,
    count: u32,
    pids: [usize; 1024],
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attrs: *const c_void, name: *const u16) -> Handle;
    fn OpenJobObjectW(access: u32, inherit: i32, name: *const u16) -> Handle;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn TerminateJobObject(job: Handle, code: u32) -> i32;
    fn QueryInformationJobObject(
        job: Handle,
        class: i32,
        info: *mut c_void,
        len: u32,
        returned: *mut u32,
    ) -> i32;
    fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, tid: u32) -> Handle;
    fn ResumeThread(thread: Handle) -> u32;
}
struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn job_name(pid: u32, created: u64) -> Vec<u16> {
    format!("Local\\Autome-session-{pid}-{created}")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}
pub fn start_job(
    child: &mut std::process::Child,
    identity: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    let pid = child.id();
    let created = creation_time(pid)?;
    unsafe {
        let name = job_name(pid, created);
        let job = CreateJobObjectW(std::ptr::null(), name.as_ptr());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let _job = OwnedHandle(job);
        // No KILL_ON_JOB_CLOSE: sessions continue across a core restart.
        if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        std::fs::write(identity, format!("{pid}:{created}"))?;
        // A named Job loses its name when its last user handle closes.
        // A small independent keeper retains that handle across core restarts.
        start_keeper(pid, created)?;
        let snapshot = CreateToolhelp32Snapshot(4, 0);
        if snapshot == -1isize as Handle {
            return Err(std::io::Error::last_os_error());
        }
        let _snapshot = OwnedHandle(snapshot);
        let mut entry: ThreadEntry = std::mem::zeroed();
        entry.size = std::mem::size_of::<ThreadEntry>() as u32;
        let mut next = Thread32First(snapshot, &mut entry);
        while next != 0 {
            if entry.owner == pid {
                let thread = OpenThread(2, 0, entry.tid);
                if thread.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let _thread = OwnedHandle(thread);
                if ResumeThread(thread) == u32::MAX {
                    return Err(std::io::Error::last_os_error());
                }
                return Ok(());
            }
            next = Thread32Next(snapshot, &mut entry);
        }
        Err(std::io::Error::other("找不到会话启动线程"))
    }
}
pub fn stop_job(pid: u32, created: u64) -> std::io::Result<()> {
    unsafe {
        let name = job_name(pid, created);
        let job = OpenJobObjectW(0x0008 | 0x0004, 0, name.as_ptr());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let _job = OwnedHandle(job);
        let mut processes: JobProcesses = std::mem::zeroed();
        if QueryInformationJobObject(
            job,
            3,
            &mut processes as *mut _ as *mut c_void,
            std::mem::size_of::<JobProcesses>() as u32,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        let mut handles = Vec::new();
        for &pid in processes.pids.iter().take(processes.count as usize) {
            let handle = OpenProcess(0x00100000, 0, pid as u32);
            if !handle.is_null() {
                handles.push(OwnedHandle(handle));
            }
        }
        if TerminateJobObject(job, 130) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let mut info = Accounting::default();
            if QueryInformationJobObject(
                job,
                1,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<Accounting>() as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if info.active == 0
                && handles
                    .iter()
                    .all(|handle| WaitForSingleObject(handle.0, 0) == 0)
            {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "会话子进程尚未退出",
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

fn start_keeper(pid: u32, created: u64) -> std::io::Result<()> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let mut executable = std::env::current_exe()?;
    if executable
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|s| s == "deps")
    {
        executable = executable
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("automed.exe");
    }
    let mut command = Command::new(executable);
    crate::platform::hide_window(&mut command);
    let mut keeper = command
        .args(["session-job-keeper", &pid.to_string(), &created.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let pipe = keeper.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::BufReader::new(pipe)
            .read_line(&mut line)
            .map(|_| line);
        let _ = tx.send(result);
    });
    let ready = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .ok()
        .and_then(Result::ok)
        .is_some_and(|line| line.trim() == "ready");
    if !ready {
        let _ = keeper.kill();
        let _ = keeper.wait();
        return Err(std::io::Error::other("会话 Job keeper 启动失败"));
    }
    std::thread::spawn(move || {
        let _ = keeper.wait();
    });
    Ok(())
}

pub fn hold_job(pid: u32, created: u64) -> std::io::Result<()> {
    use std::io::Write;
    unsafe {
        let name = job_name(pid, created);
        let job = OpenJobObjectW(4, 0, name.as_ptr());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let _job = OwnedHandle(job);
        println!("ready");
        std::io::stdout().flush()?;
        loop {
            let mut info = Accounting::default();
            if QueryInformationJobObject(
                job,
                1,
                &mut info as *mut _ as *mut c_void,
                std::mem::size_of::<Accounting>() as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if info.active == 0 {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}
pub fn descendants(root: u32) -> std::io::Result<Vec<(u32, u64)>> {
    let mut entries = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(2, 0);
        if snapshot == -1isize as Handle {
            return Err(std::io::Error::last_os_error());
        }
        let mut entry: ProcessEntry = std::mem::zeroed();
        entry.size = std::mem::size_of::<ProcessEntry>() as u32;
        let mut next = Process32FirstW(snapshot, &mut entry);
        while next != 0 {
            entries.push((entry.pid, entry.parent));
            next = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    let mut tree = vec![root];
    loop {
        let before = tree.len();
        for &(pid, parent) in &entries {
            if tree.contains(&parent) && !tree.contains(&pid) {
                tree.push(pid);
            }
        }
        if tree.len() == before {
            break;
        }
    }
    tree.into_iter()
        .skip(1)
        .filter_map(|pid| {
            match creation_time(pid) {
                Ok(time) => Some(Ok((pid, time))),
                Err(e) if e.raw_os_error() == Some(87) => None, // exited during snapshot
                Err(e) => Some(Err(e)),
            }
        })
        .collect()
}
pub fn creation_time(pid: u32) -> std::io::Result<u64> {
    unsafe {
        let handle = OpenProcess(0x1000, 0, pid);
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let (mut creation, mut exit, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        let error = std::io::Error::last_os_error();
        CloseHandle(handle);
        if ok == 0 {
            Err(error)
        } else {
            Ok(((creation.high as u64) << 32) | creation.low as u64)
        }
    }
}
pub fn is_alive(pid: u32) -> bool {
    unsafe {
        let handle = OpenProcess(0x00100000, 0, pid);
        if handle.is_null() {
            // Access denied means we cannot establish death. Do not fail a live session.
            return std::io::Error::last_os_error().raw_os_error() == Some(5);
        }
        let result = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        result != 0 // WAIT_TIMEOUT or an unknown/error result is not proof of death.
    }
}
