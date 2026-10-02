//! Commands Dexo runs on the user's behalf -- a password command, a pre-connect command,
//! `docker` -- through the platform shell or directly, and how they are stopped.

use std::io::Read;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

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

/// A command that never reads the terminal goes in a process group of its own: Ctrl+C
/// at the terminal -- under an external editor, say -- does not reach it, and it is
/// stopped whole, what it started included.
pub(crate) fn detach(command: &mut Command) {
    command.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
}

/// No terminal at all: a session of its own on Unix, no console on Windows. For a
/// command run while the workbench owns the terminal: one that would prompt there fails
/// at once instead of reading the keys the workbench is reading. Stopped as a group.
pub(crate) fn without_terminal(command: &mut Command) {
    command.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe, the only kind of call allowed between
        // fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
}

/// The detached commands running now, by process id -- the group's id on Unix -- so a
/// Dexo leaving on a signal, or quitting with one still starting, stops them.
static RUNNING: [AtomicI64; 64] = [const { AtomicI64::new(0) }; 64];

/// A command left in the terminal's group -- a password command -- is no group leader,
/// and stopping its id as a group finds nothing.
pub(crate) fn register(id: u32) {
    for slot in &RUNNING {
        if slot
            .compare_exchange(0, i64::from(id), Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return;
        }
    }
}

fn unregister(id: u32) {
    for slot in &RUNNING {
        let _ = slot.compare_exchange(i64::from(id), 0, Ordering::SeqCst, Ordering::SeqCst);
    }
}

/// Stops a detached command and everything in its group.
pub(crate) fn stop_group(child: &mut Child) {
    unregister(child.id());
    #[cfg(unix)]
    {
        // SAFETY: killpg only sends a signal; a group that has gone already is ESRCH.
        unsafe {
            libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    taskkill(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn taskkill(id: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &id.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Stops every detached command still running: for a Dexo on its way out.
pub fn stop_all() {
    for slot in &RUNNING {
        let id = slot.swap(0, Ordering::SeqCst);
        if id > 0 {
            #[cfg(unix)]
            // SAFETY: killpg only sends a signal.
            unsafe {
                libc::killpg(id as libc::pid_t, libc::SIGKILL);
            }
            #[cfg(windows)]
            taskkill(id as u32);
        }
    }
}

/// Stops the detached commands when Dexo is told to leave -- SIGTERM, SIGHUP, and
/// SIGINT when `interrupt` -- then leaves the way the signal says. Without it a tunnel
/// outlived an MCP server its client stopped.
pub fn stop_on_signals(interrupt: bool) {
    #[cfg(unix)]
    {
        extern "C" fn leave(signal: libc::c_int) {
            // Only async-signal-safe calls: atomics, killpg, signal, raise.
            for slot in &RUNNING {
                let id = slot.load(Ordering::SeqCst);
                if id > 0 {
                    // SAFETY: killpg only sends a signal.
                    unsafe {
                        libc::killpg(id as libc::pid_t, libc::SIGKILL);
                    }
                }
            }
            // SAFETY: back to the default action, then the signal again to take it.
            unsafe {
                libc::signal(signal, libc::SIG_DFL);
                libc::raise(signal);
            }
        }
        let handler = leave as extern "C" fn(libc::c_int) as libc::sighandler_t;
        let mut signals = vec![libc::SIGTERM, libc::SIGHUP];
        if interrupt {
            signals.push(libc::SIGINT);
        }
        for signal in signals {
            // SAFETY: installs a handler that only makes async-signal-safe calls.
            unsafe {
                libc::signal(signal, handler);
            }
        }
    }
    #[cfg(not(unix))]
    let _ = interrupt;
}

/// While it lives, Ctrl+C at the terminal does not end Dexo -- under an external editor,
/// out of raw mode -- and when it goes, SIGINT does again what it did before. A handler
/// that does nothing, not an ignore, which the editor would inherit and keep its own
/// Ctrl+C from working. Tokio's handler, which this replaces, stays for good: after one
/// external edit `kill -INT` was ignored for the rest of the session.
pub struct InterruptShield {
    #[cfg(unix)]
    previous: libc::sigaction,
}

impl InterruptShield {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[cfg(unix)]
        {
            extern "C" fn nothing(_: libc::c_int) {}
            // SAFETY: plain sigaction calls on zeroed structs; the handler does nothing.
            unsafe {
                let mut shield: libc::sigaction = std::mem::zeroed();
                shield.sa_sigaction = nothing as extern "C" fn(libc::c_int) as libc::sighandler_t;
                libc::sigemptyset(&mut shield.sa_mask);
                let mut previous: libc::sigaction = std::mem::zeroed();
                libc::sigaction(libc::SIGINT, &shield, &mut previous);
                Self { previous }
            }
        }
        #[cfg(not(unix))]
        Self {}
    }
}

impl Drop for InterruptShield {
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: puts back the action read when the shield went up.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
    }
}

/// How a command run with [`run_until`] ended.
pub(crate) enum Ran {
    /// It exited; its stdout, unless it was still held open at the deadline (by
    /// something it left running).
    Exited(ExitStatus, Option<Vec<u8>>),
    /// It was still running at the deadline, and was stopped.
    TimedOut,
}

/// Runs `command`, its stdout read on the side so a full pipe cannot stall it, until it
/// exits or `deadline` passes; then `stop` stops it.
pub(crate) fn run_until(
    mut command: Command,
    deadline: Instant,
    stop: fn(&mut Child),
) -> std::io::Result<Ran> {
    let mut child = command.stdout(Stdio::piped()).spawn()?;
    register(child.id());
    let mut stdout = child.stdout.take();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(stdout) = stdout.as_mut() {
            let _ = stdout.read_to_end(&mut output);
        }
        let _ = sender.send(output);
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                stop(&mut child);
                return Ok(Ran::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                stop(&mut child);
                return Err(error);
            }
        }
    };
    unregister(child.id());
    let output = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok();
    Ok(Ran::Exited(status, output))
}

/// Stops a command left in the terminal's process group -- a password command, which
/// may prompt there -- and what it started, found by parent. Each process is frozen
/// before its children are listed, so none can start another between the listing and
/// the kill, nor leave and have its children re-parented; then the frozen tree is
/// killed whole.
pub(crate) fn stop_tree(child: &mut Child) {
    unregister(child.id());
    #[cfg(unix)]
    {
        let mut tree = vec![child.id()];
        let mut at = 0;
        while let Some(&parent) = tree.get(at) {
            // SAFETY: kill only sends a signal; a process that has gone already is ESRCH.
            unsafe {
                libc::kill(parent as libc::pid_t, libc::SIGSTOP);
            }
            tree.extend(children_of(parent));
            at += 1;
        }
        for pid in tree {
            // SAFETY: as above.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    taskkill(child.id());
    let _ = child.kill();
    let _ = child.wait();
}

/// The processes whose parent is `pid`, read from /proc: nothing to install, unlike
/// the `pgrep` the cleanup needed before.
#[cfg(target_os = "linux")]
fn children_of(pid: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let child: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{child}/stat")).ok()?;
            // `pid (name) state ppid …`: the name may hold spaces and parentheses, so the
            // fields are counted from its last `)`.
            let parent: u32 = stat
                .rsplit_once(')')?
                .1
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            (parent == pid).then_some(child)
        })
        .collect()
}

/// The processes whose parent is `pid`, as `pgrep -P` lists them: macOS and the BSDs
/// have no /proc to read.
#[cfg(all(unix, not(target_os = "linux")))]
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

#[cfg(all(test, unix))]
mod tests {
    /// The shield keeps SIGINT from ending the process while it is up, and puts the
    /// action that was there back when it goes.
    #[test]
    fn the_interrupt_shield_puts_sigint_back() {
        let action = || {
            // SAFETY: reads the current action only.
            unsafe {
                let mut current: libc::sigaction = std::mem::zeroed();
                libc::sigaction(libc::SIGINT, std::ptr::null(), &mut current);
                current.sa_sigaction
            }
        };
        let before = action();
        {
            let _shield = super::InterruptShield::new();
            assert_ne!(action(), before);
            // SAFETY: the shield's handler does nothing; the test process lives on.
            unsafe {
                libc::raise(libc::SIGINT);
            }
        }
        assert_eq!(action(), before);
    }
}
