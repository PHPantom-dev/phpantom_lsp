//! Shared infrastructure for external tool proxies (PHPStan, PHPCS,
//! Mago): process spawning with a timeout and deadlock-safe
//! stdout/stderr draining, binary auto-detection, and tool-reported
//! path matching.

/// Result of running an external command via [`run_command_with_timeout`].
#[derive(Debug)]
pub struct CommandOutput {
    /// Exit code (or -1 if the process was killed / no code available).
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Spawn a command, feed it optional stdin, wait for it with a timeout,
/// and return its exit code plus captured stdout/stderr.
///
/// stdout and stderr are drained on dedicated reader threads that run
/// **while** the child is alive. This is essential: a child that writes
/// more than the OS pipe buffer (~64 KB — easily exceeded by the JSON
/// output of PHPStan/PHPCS/Mago on a real project) blocks on the write
/// until the pipe is drained. If we only read after the process exits,
/// the child can never exit and the call spins until it times out,
/// returning an error instead of the diagnostics. Reading concurrently
/// keeps the pipe from filling.
///
/// When `stdin_content` is `Some`, it is written to the child's stdin
/// and the pipe is then closed (EOF). The write happens after the reader
/// threads are started so a large stdin payload cannot deadlock against
/// the child's output. When `stdin_content` is `None`, stdin is set to
/// null so the child never inherits the server's stdin.
///
/// `tool_name` is used only for error messages. On timeout or
/// cancellation the child is killed and an `Err` is returned.
pub fn run_command_with_timeout(
    command: &mut std::process::Command,
    timeout: std::time::Duration,
    cancelled: &std::sync::atomic::AtomicBool,
    tool_name: &str,
    stdin_content: Option<&str>,
) -> Result<CommandOutput, String> {
    use std::io::{Read, Write};
    use std::process::Stdio;
    use std::sync::atomic::Ordering;

    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_content.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", tool_name, e))?;

    // Drain stdout/stderr concurrently so the child can never block
    // writing to a full pipe while we wait for it to exit.
    let stdout_reader = child.stdout.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf
        })
    });
    let stderr_reader = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf
        })
    });

    // Feed stdin (if any) and close it so the child sees EOF. A broken
    // pipe here means the child exited early; the status/output below is
    // what we care about, so the write error is intentionally ignored.
    if let Some((content, mut stdin)) = stdin_content.zip(child.stdin.take()) {
        let _ = stdin.write_all(content.as_bytes());
    }

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "{} timed out after {}ms",
                        tool_name,
                        timeout.as_millis()
                    ));
                }
                if cancelled.load(Ordering::Acquire) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} cancelled (server shutting down)", tool_name));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                return Err(format!("Error waiting for {}: {}", tool_name, e));
            }
        }
    };

    // The child has exited, so its pipe write ends are closed and the
    // reader threads will reach EOF; join them to collect the output.
    let stdout = stdout_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();
    let stderr = stderr_reader
        .and_then(|h| h.join().ok())
        .unwrap_or_default();

    Ok(CommandOutput {
        code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

// ── Binary location ─────────────────────────────────────────────────

/// Auto-detect an external tool binary: `<bin_dir>/<binary_name>` under
/// the workspace root (Composer's bin-dir, default `vendor/bin`), then
/// `$PATH`.
pub fn auto_detect_binary(
    workspace_root: Option<&std::path::Path>,
    bin_dir: Option<&str>,
    binary_name: &str,
) -> Option<std::path::PathBuf> {
    // Check the Composer bin directory first.
    if let Some(root) = workspace_root {
        let bin = bin_dir.unwrap_or("vendor/bin");
        let candidate = root.join(bin).join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // Fall back to $PATH.
    which(binary_name).ok()
}

/// Simple `which`-like lookup: search `$PATH` for an executable with
/// the given name.
pub fn which(binary_name: &str) -> Result<std::path::PathBuf, String> {
    let path_var = std::env::var("PATH").map_err(|_| "PATH not set".to_string())?;

    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary_name);
        if candidate.is_file() && is_executable(&candidate) {
            return Ok(candidate);
        }
    }

    Err(format!("{} not found on PATH", binary_name))
}

#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &std::path::Path) -> bool {
    true
}

// ── Path matching ───────────────────────────────────────────────────

/// Check whether two file paths refer to the same file.
///
/// External tools normalize paths to absolute form. We compare by
/// checking suffix matches (one path ends with the other) to handle
/// cases where one path is relative and the other is absolute, or
/// where symlinks produce different prefixes.
pub fn paths_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let a_norm = a.replace('\\', "/");
    let b_norm = b.replace('\\', "/");
    if a_norm == b_norm {
        return true;
    }
    // Check suffix match (one is a suffix of the other), requiring a
    // path separator boundary so that e.g. "AFoo.php" does not match "Foo.php".
    a_norm.ends_with(&format!("/{}", b_norm)) || b_norm.ends_with(&format!("/{}", a_norm))
}

/// Project-wide runs multiply the per-file timeout by this factor.
pub const WORKSPACE_TIMEOUT_FACTOR: u64 = 10;

/// Turn a project-wide tool run's exit code into diagnostics grouped by
/// file path.
///
/// The three proxies agree on the shape: one set of exit codes means
/// "ran, found something", anything higher means the tool failed. A
/// failing run still gets its output parsed, because a tool that reports
/// findings *and* exits non-zero (a baseline error, a partially
/// unreadable file) has already told us something worth showing; only a
/// run that produced nothing usable becomes an error.
///
/// `findings_codes` lists the exit codes that mean "parse the output"
/// (PHPCS uses both 1 and 2). `success_parses` is for tools whose
/// zero-exit output still carries diagnostics.
pub fn workspace_run_result<F>(
    output: &CommandOutput,
    tool_name: &str,
    findings_codes: &[i32],
    success_parses: bool,
    parse: F,
) -> Result<
    std::collections::HashMap<std::path::PathBuf, Vec<tower_lsp::lsp_types::Diagnostic>>,
    String,
>
where
    F: Fn(
        &str,
    ) -> Result<
        std::collections::HashMap<std::path::PathBuf, Vec<tower_lsp::lsp_types::Diagnostic>>,
        String,
    >,
{
    if output.code == 0 {
        if !success_parses || output.stdout.trim().is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        return parse(&output.stdout).or_else(|_| Ok(std::collections::HashMap::new()));
    }

    if findings_codes.contains(&output.code) {
        return parse(&output.stdout);
    }

    match parse(&output.stdout) {
        Ok(map) if !map.is_empty() => Ok(map),
        _ => Err(format!(
            "{} exited with code {} (stderr: {})",
            tool_name,
            output.code,
            output.stderr.trim()
        )),
    }
}

/// The whole of a single line, as external tools report positions.
///
/// PHPStan and PHPCS both report a line and an ambiguous (or absent)
/// column, so there is no reliable way to derive a precise range. Both
/// underline the full line instead; the editor clamps the end column to
/// the line's real length.
pub fn full_line_range(line: u32) -> tower_lsp::lsp_types::Range {
    tower_lsp::lsp_types::Range {
        start: tower_lsp::lsp_types::Position { line, character: 0 },
        end: tower_lsp::lsp_types::Position {
            line,
            character: u32::MAX,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── paths_match ─────────────────────────────────────────────────

    #[test]
    fn paths_match_identical() {
        assert!(paths_match(
            "/home/user/project/src/Foo.php",
            "/home/user/project/src/Foo.php"
        ));
    }

    #[test]
    fn paths_match_suffix() {
        assert!(paths_match("/home/user/project/src/Foo.php", "src/Foo.php"));
    }

    #[test]
    fn paths_match_reverse_suffix() {
        assert!(paths_match("src/Foo.php", "/home/user/project/src/Foo.php"));
    }

    #[test]
    fn paths_match_different_files() {
        assert!(!paths_match(
            "/home/user/project/src/Foo.php",
            "src/Bar.php"
        ));
    }

    #[test]
    fn paths_match_windows_separators() {
        assert!(paths_match(
            "C:\\Users\\project\\src\\Foo.php",
            "src/Foo.php",
        ));
    }

    #[test]
    fn paths_match_rejects_partial_filename_suffix() {
        assert!(!paths_match("/project/src/AFoo.php", "Foo.php",));
    }

    #[test]
    fn paths_match_rejects_partial_dirname_suffix() {
        assert!(!paths_match("/project/src/Foo.php", "rc/Foo.php",));
    }

    /// A child that writes far more than the OS pipe buffer (~64 KB)
    /// must not deadlock: the reader threads keep the pipe drained while
    /// the child runs, so it can exit and we collect the full output.
    #[cfg(unix)]
    #[test]
    fn run_command_drains_large_stdout_without_deadlock() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        // 200_000 NUL bytes (valid UTF-8), well above the pipe buffer.
        let mut cmd = Command::new("head");
        cmd.arg("-c").arg("200000").arg("/dev/zero");
        let cancelled = AtomicBool::new(false);
        let out =
            run_command_with_timeout(&mut cmd, Duration::from_secs(10), &cancelled, "test", None)
                .expect("command should complete");
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout.len(), 200000);
    }

    /// Feeding large stdin while the child writes large stdout (here
    /// `cat`, which echoes stdin) exercises both pipes at once. Under the
    /// old read-after-exit logic this deadlocked.
    #[cfg(unix)]
    #[test]
    fn run_command_echoes_large_stdin() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        let payload = "x".repeat(200000);
        let mut cmd = Command::new("cat");
        let cancelled = AtomicBool::new(false);
        let out = run_command_with_timeout(
            &mut cmd,
            Duration::from_secs(10),
            &cancelled,
            "test",
            Some(&payload),
        )
        .expect("command should complete");
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, payload);
    }

    /// A long-running child is killed when the timeout elapses, returning
    /// an error rather than hanging.
    #[cfg(unix)]
    #[test]
    fn run_command_times_out() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let cancelled = AtomicBool::new(false);
        let result = run_command_with_timeout(
            &mut cmd,
            Duration::from_millis(100),
            &cancelled,
            "test",
            None,
        );
        let err = result.expect_err("should time out");
        assert!(err.contains("timed out"), "unexpected error: {err}");
    }

    /// A spawn failure surfaces as an error rather than panicking.
    #[test]
    fn run_command_reports_spawn_failure() {
        use std::process::Command;
        use std::sync::atomic::AtomicBool;
        use std::time::Duration;

        let mut cmd = Command::new("phpantom-no-such-binary-xyz");
        let cancelled = AtomicBool::new(false);
        let result =
            run_command_with_timeout(&mut cmd, Duration::from_secs(1), &cancelled, "test", None);
        let err = result.expect_err("spawn should fail");
        assert!(
            err.contains("Failed to spawn test"),
            "unexpected error: {err}"
        );
    }
}
