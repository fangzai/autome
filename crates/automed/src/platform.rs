//! Process and path conventions shared by the launcher and environment probe.
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
pub(crate) mod windows;

pub fn find_on_path(path: &str, name: &str, is_exec: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    for dir in std::env::split_paths(path).filter(|p| !p.as_os_str().is_empty()) {
        for candidate in executable_names(name) {
            let candidate = dir.join(candidate);
            if is_exec(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_names(name: &str) -> Vec<String> {
    #[allow(unused_mut)]
    let mut names = vec![name.to_owned()];
    #[cfg(windows)]
    if Path::new(name).extension().is_none() {
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into());
        names.extend(
            extensions
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|s| format!("{name}{s}")),
        );
    }
    names
}

/// Resolve only a Git for Windows bash, never a bare PATH bash (possibly WSL).
pub fn git_bash() -> std::io::Result<PathBuf> {
    if let Some(path) = std::env::var_os("AUTOMED_BASH") {
        let path = PathBuf::from(path);
        return validate_bash(path);
    }
    let mut candidates = Vec::new();
    if let Ok(path) = std::env::var("PATH")
        && let Some(git) = find_on_path(&path, "git", Path::is_file)
        && let Some(root) = git.parent().and_then(Path::parent)
    {
        candidates.push(root.join("bin/bash.exe"));
        candidates.push(root.join("usr/bin/bash.exe"));
    }
    for base in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(base) {
            candidates.push(PathBuf::from(&root).join("Git/bin/bash.exe"));
            candidates.push(PathBuf::from(root).join("Programs/Git/bin/bash.exe"));
        }
    }
    for path in ["D:/Git/bin/bash.exe", "C:/Git/bin/bash.exe"] {
        candidates.push(PathBuf::from(path));
    }
    candidates
        .into_iter()
        .find_map(|p| validate_bash(p).ok())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "找不到 Git Bash；请安装 Git for Windows 或设置 AUTOMED_BASH",
            )
        })
}

fn validate_bash(path: PathBuf) -> std::io::Result<PathBuf> {
    let root = path.parent().and_then(Path::parent);
    // Git installations have usr/bin/msys-2.0.dll; WSL's bash does not.
    let msys = root.is_some_and(|r| {
        r.join("usr/bin/msys-2.0.dll").is_file() || r.join("bin/msys-2.0.dll").is_file()
    });
    if path.is_file() && msys {
        Ok(path)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("无效的 Git Bash：{}", path.display()),
        ))
    }
}

/// Convert canonical Windows spelling at the shell boundary, including UNC.
pub fn shell_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    #[cfg(windows)]
    {
        let text = text
            .strip_prefix(r"\\?\UNC\")
            .map(|s| format!(r"\\{s}"))
            .unwrap_or_else(|| text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned());
        text.replace('\\', "/")
    }
    #[cfg(not(windows))]
    text.into_owned()
}

pub fn path_is_within(path: &Path, parent: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
    let parent = parent.canonicalize().unwrap_or_else(|_| parent.to_owned());
    #[cfg(windows)]
    {
        Path::new(&shell_path(&path).to_lowercase()).starts_with(shell_path(&parent).to_lowercase())
    }
    #[cfg(not(windows))]
    path.starts_with(parent)
}

/// Invocation for native programs, npm shell shims and test scripts.
pub fn command(program: &Path) -> std::io::Result<Command> {
    if !program.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("找不到程序 {}", program.display()),
        ));
    }
    #[cfg(windows)]
    {
        let ext = program
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut command = if ext.is_empty() || ext == "sh" {
            let bash = git_bash()?;
            let root = bash.parent().and_then(Path::parent).unwrap();
            let mut paths = vec![
                root.join("usr/bin"),
                root.join("bin"),
                root.join("mingw64/bin"),
            ];
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            let mut c = Command::new(bash);
            c.env(
                "PATH",
                std::env::join_paths(paths).map_err(std::io::Error::other)?,
            );
            c.arg(shell_path(program));
            c
        } else {
            // Rust's Windows Command implementation supports cmd/bat wrappers.
            Command::new(program)
        };
        hide_window(&mut command);
        Ok(command)
    }
    #[cfg(not(windows))]
    Ok(Command::new(program))
}

pub fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Bounded capture for discovery commands. Pipe readers never hold the caller
/// past the deadline, and the managed process group is cleaned up on every exit.
pub fn capture(
    program: &Path,
    args: &[&str],
    timeout: std::time::Duration,
    limit: usize,
) -> std::io::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let mut command = command(program)?;
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000004);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    #[cfg(windows)]
    let identity =
        std::env::temp_dir().join(format!("autome-query-{}.identity", uuid::Uuid::new_v4()));
    #[cfg(windows)]
    if let Err(e) = windows::start_job(&mut child, &identity) {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&identity);
        return Err(e);
    }
    fn reader(
        pipe: impl Read + Send + 'static,
        limit: usize,
    ) -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = pipe
                .take(limit as u64 + 1)
                .read_to_end(&mut bytes)
                .and_then(|_| {
                    if bytes.len() > limit {
                        Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "命令输出超过大小限制",
                        ))
                    } else {
                        Ok(bytes)
                    }
                });
            let _ = tx.send(result);
        });
        rx
    }
    let stdout_rx = reader(child.stdout.take().unwrap(), limit);
    let stderr_rx = reader(child.stderr.take().unwrap(), limit);
    let deadline = Instant::now() + timeout;
    let result = (|| {
        let (mut stdout, mut stderr, mut status) = (None, None, None);
        loop {
            if stdout.is_none()
                && let Ok(bytes) = stdout_rx.try_recv()
            {
                stdout = Some(bytes?);
            }
            if stderr.is_none()
                && let Ok(bytes) = stderr_rx.try_recv()
            {
                stderr = Some(bytes?);
            }
            if status.is_none() {
                status = child.try_wait()?;
            }
            if let Some(status) = status
                && let (Some(stdout), Some(stderr)) = (&mut stdout, &mut stderr)
            {
                return Ok(std::process::Output {
                    status,
                    stdout: std::mem::take(stdout),
                    stderr: std::mem::take(stderr),
                });
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "命令执行超时",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    #[cfg(windows)]
    {
        let _ = crate::launcher::stop_owned_session(pid as i32, &identity.with_extension("log"));
        let _ = std::fs::remove_file(identity);
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_search_respects_directory_order_and_empty_entries() {
        let path = std::env::join_paths([Path::new(""), Path::new("/a"), Path::new("/b")]).unwrap();
        let found = find_on_path(&path.to_string_lossy(), "tool", |p| {
            p == Path::new("/a/tool") || p == Path::new("/b/tool.exe")
        });
        assert_eq!(found, Some(PathBuf::from("/a/tool")));
    }

    #[cfg(windows)]
    #[test]
    fn capture_bounds_runtime_and_output_without_model_access() {
        let shell = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        let started = std::time::Instant::now();
        let error = capture(
            &shell,
            &["-NoProfile", "-Command", "Start-Sleep -Seconds 30"],
            std::time::Duration::from_millis(250),
            1024,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let error = capture(
            &shell,
            &["-NoProfile", "-Command", "[Console]::Write(('x' * 8192))"],
            std::time::Duration::from_secs(10),
            128,
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[cfg(windows)]
    #[test]
    fn a_single_windows_path_keeps_the_drive_letter() {
        let found = find_on_path(r"D:\tools", "git", |p| p == Path::new(r"D:\tools\git.EXE"));
        assert_eq!(found, Some(PathBuf::from(r"D:\tools\git.EXE")));
        assert!(validate_bash(PathBuf::from(r"C:\Windows\System32\bash.exe")).is_err());
    }
}
