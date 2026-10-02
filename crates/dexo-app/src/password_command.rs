//! A connection's password read from a password manager's command -- `op read …`,
//! `pass show …`, `vault kv get …` -- instead of the keychain.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use secrecy::SecretString;

use crate::error::{AppError, ErrorCategory};

pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `command` through the platform shell and returns what it printed, without the
/// trailing line break. Its stdout is the secret and is held in memory only; stderr is
/// never read, and an error names the command, never what it printed.
pub fn run(command: &str, timeout: Duration) -> Result<SecretString, AppError> {
    let fail = |reason: String| {
        AppError::new(
            ErrorCategory::Authentication,
            format!("password command `{command}` {reason}"),
        )
    };
    let mut child = shell(command)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| fail(format!("could not start: {error}")))?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (sender, receiver) = mpsc::channel();
    // Read on the side: a command that prints more than the pipe holds would otherwise
    // wait on us while we wait on it.
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = stdout.read_to_end(&mut output);
        let _ = sender.send(output);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                stop_tree(&mut child);
                return Err(fail(format!(
                    "did not finish within {}s",
                    timeout.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(fail(format!("could not be waited on: {error}"))),
        }
    };
    if !status.success() {
        return Err(fail(format!("failed ({status})")));
    }
    // A helper the command left running can hold stdout open after it exits; the read
    // gets what is left of the deadline, not forever.
    let output = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| fail("kept its output open after exiting".into()))?;
    let text =
        String::from_utf8(output).map_err(|_| fail("printed something that is not text".into()))?;
    let secret = text.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        return Err(fail("printed nothing".into()));
    }
    Ok(SecretString::from(secret.to_string()))
}

/// Stops the shell and everything it started. Killing only the shell left the command
/// itself (`sleep 60`, a password manager waiting on a prompt) running, re-parented.
fn stop_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // The command stays in the terminal's process group, where one that prompts on
        // the terminal can read it (in a group of its own it was stopped by SIGTTIN, and
        // Ctrl+C never reached it); so its descendants are found by parent instead,
        // all of them before any is stopped and re-parented.
        let mut tree = vec![child.id()];
        let mut at = 0;
        while let Some(&parent) = tree.get(at) {
            tree.extend(children_of(parent));
            at += 1;
        }
        for pid in tree.into_iter().skip(1) {
            // SAFETY: kill only sends a signal; a process that has gone already is ESRCH.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The processes whose parent is `pid`, as `pgrep -P` lists them.
#[cfg(unix)]
fn children_of(pid: u32) -> Vec<u32> {
    Command::new("pgrep")
        .args(["-P", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn shell(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut shell = Command::new("cmd");
    // Verbatim: `args` would escape the command's quotes as `\"`, which cmd does not
    // read, and `op read "op://vault/item"` would arrive mangled.
    shell.arg("/C").raw_arg(command);
    shell
}

#[cfg(not(windows))]
fn shell(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.args(["-c", command]);
    shell
}

#[cfg(all(test, unix))]
mod tests {
    use std::time::Duration;

    use secrecy::ExposeSecret;

    use super::run;

    #[test]
    fn stdout_is_the_secret_without_its_line_break() {
        let secret = run("printf 'hunter2\\n'", Duration::from_secs(5)).unwrap();
        assert_eq!(secret.expose_secret(), "hunter2");
    }

    /// A failing command names itself, and nothing it printed reaches the message.
    #[test]
    fn a_failure_never_carries_the_output() {
        // What it prints (`out-42`, `err-42`) is not in its own text, which the message
        // names.
        let error = run(
            "printf 'out-%s' $((40+2)); printf 'err-%s' $((40+2)) >&2; exit 3",
            Duration::from_secs(5),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("password command"), "{error}");
        assert!(
            !error.contains("out-42") && !error.contains("err-42"),
            "{error}"
        );
    }

    #[test]
    fn a_command_that_hangs_is_stopped_at_the_deadline() {
        let started = std::time::Instant::now();
        // The marker makes the sleep findable; it must be gone, not re-parented.
        let error = run("sleep 41.273; true", Duration::from_millis(300))
            .unwrap_err()
            .to_string();
        assert!(error.contains("did not finish"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(200));
        let left = std::process::Command::new("pgrep")
            .args(["-f", "^sleep 41.273"])
            .output()
            .map(|output| output.stdout)
            .unwrap_or_default();
        assert!(
            left.is_empty(),
            "still running: {}",
            String::from_utf8_lossy(&left)
        );
    }

    /// The command shares the terminal's process group, so one that reads the terminal
    /// is not stopped for it.
    #[test]
    fn the_command_runs_in_dexos_process_group() {
        let group = run("ps -o pgid= -p $$", Duration::from_secs(5)).unwrap();
        // SAFETY: getpgrp has no preconditions.
        let ours = unsafe { libc::getpgrp() };
        assert_eq!(group.expose_secret().trim(), ours.to_string());
    }

    #[test]
    fn an_empty_answer_is_refused() {
        assert!(run("true", Duration::from_secs(5)).is_err());
    }
}
