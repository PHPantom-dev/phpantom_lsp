//! Spawning external tool processes (PHPStan, PHPCS, Mago) with a
//! timeout and deadlock-safe stdout/stderr draining.

/// Result of running an external command via [`run_command_with_timeout`].
#[derive(Debug)]
pub struct CommandOutput {
    /// Exit code (or -1 if the process was killed / no code available).
    pub code: i32,
    /// Captured stdout content.
    pub stdout: String,
    /// Captured stderr content.
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

#[cfg(test)]
mod tests {
    use super::*;

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
