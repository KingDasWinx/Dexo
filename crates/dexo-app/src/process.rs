//! Commands Dexo runs on the user's behalf -- a password command, a pre-connect
//! command -- through the platform shell, and how they are stopped.

use std::process::{Command, Stdio};

/// Stops the shell and everything it started. Killing only the shell left the command
/// itself (`sleep 60`, a password manager waiting on a prompt) running, re-parented.
pub(crate) fn stop_tree(child: &mut std::process::Child) {
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
pub(crate) fn shell(command: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut shell = Command::new("cmd");
    // Verbatim: `args` would escape the command's quotes as `\"`, which cmd does not
    // read, and `op read "op://vault/item"` would arrive mangled.
    shell.arg("/C").raw_arg(command);
    shell
}

#[cfg(not(windows))]
pub(crate) fn shell(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.args(["-c", command]);
    shell
}
