//! A connection's password read from a password manager's command -- `op read …`,
//! `pass show …`, `vault kv get …` -- instead of the keychain.

use std::process::Stdio;
use std::time::{Duration, Instant};

use secrecy::SecretString;

use crate::error::{AppError, ErrorCategory};

pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Runs `command` through the platform shell and returns what it printed, without the
/// trailing line break. Its stdout is the secret and is held in memory only; stderr is
/// never read, and an error names the command, never what it printed.
pub fn run(command: &str, timeout: Duration) -> Result<SecretString, AppError> {
    run_with(command, timeout, false)
}

/// [`run`] for the workbench, which owns the terminal: the command gets none, so one
/// that would prompt there fails at once instead of sharing the keyboard with Dexo.
pub fn run_without_terminal(command: &str, timeout: Duration) -> Result<SecretString, AppError> {
    run_with(command, timeout, true)
}

fn run_with(command: &str, timeout: Duration, detached: bool) -> Result<SecretString, AppError> {
    let hint = if detached {
        " -- it cannot ask on the terminal from the workbench: unlock its agent in a shell first, or use one that asks in a window"
    } else {
        ""
    };
    let fail = |reason: String| {
        AppError::new(
            ErrorCategory::Authentication,
            format!("password command `{command}` {reason}"),
        )
    };
    let mut shell = crate::process::shell(command);
    shell.stdin(Stdio::null()).stderr(Stdio::null());
    let stop = if detached {
        crate::process::without_terminal(&mut shell);
        crate::process::stop_group
    } else {
        // Left in the terminal's process group: a command that prompts there can read it.
        crate::process::stop_tree
    };
    let deadline = Instant::now() + timeout;
    let ran = crate::process::run_until(shell, deadline, stop)
        .map_err(|error| fail(format!("could not start: {error}")))?;
    let (status, output) = match ran {
        crate::process::Ran::TimedOut => {
            return Err(fail(format!(
                "did not finish within {}s{hint}",
                timeout.as_secs()
            )));
        }
        crate::process::Ran::Exited(status, output) => (status, output),
    };
    if !status.success() {
        return Err(fail(format!("failed ({status}){hint}")));
    }
    // A helper the command left running can hold stdout open after it exits; the read
    // got what was left of the deadline, not forever.
    let output = output.ok_or_else(|| fail("kept its output open after exiting".into()))?;
    let text =
        String::from_utf8(output).map_err(|_| fail("printed something that is not text".into()))?;
    let secret = text.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        return Err(fail("printed nothing".into()));
    }
    Ok(SecretString::from(secret.to_string()))
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

    /// From the workbench the command has no terminal: a session of its own, so one
    /// that would prompt fails rather than reading Dexo's keys, and says why.
    #[test]
    fn the_workbench_gives_the_command_no_terminal() {
        let session = super::run_without_terminal(
            "[ \"$(ps -o sid= -p $$ | tr -d ' ')\" = \"$$\" ] && echo detached",
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(session.expose_secret(), "detached");
        let error = super::run_without_terminal("exec < /dev/tty; read x", Duration::from_secs(5))
            .unwrap_err()
            .to_string();
        assert!(error.contains("cannot ask on the terminal"), "{error}");
    }

    #[test]
    fn an_empty_answer_is_refused() {
        assert!(run("true", Duration::from_secs(5)).is_err());
    }
}
