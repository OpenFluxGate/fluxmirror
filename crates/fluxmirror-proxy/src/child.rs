use std::io;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

pub struct ChildProc {
    inner: Child,
}

impl ChildProc {
    /// Spawn the real MCP server. Stdin/stdout are piped so we can bridge
    /// them; stderr is inherited so the child's diagnostics flow up to the
    /// parent terminal unchanged.
    pub fn spawn(command: &[String]) -> io::Result<Self> {
        if command.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty server command",
            ));
        }
        let mut cmd = Command::new(&command[0]);
        cmd.args(&command[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let inner = cmd.spawn()?;
        Ok(ChildProc { inner })
    }

    pub fn pid(&self) -> u32 {
        self.inner.id()
    }

    pub fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.inner.stdin.take()
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.inner.stdout.take()
    }

    /// SIGTERM, wait up to 2s, then SIGKILL with another 1s wait.
    pub fn shutdown(&mut self) {
        match self.inner.try_wait() {
            Ok(Some(status)) => {
                eprintln!(
                    "[fluxmirror-proxy] child already exited (status={status:?})"
                );
                return;
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("[fluxmirror-proxy] WARN try_wait failed: {e}");
            }
        }

        let pid = self.inner.id();
        eprintln!("[fluxmirror-proxy] sending SIGTERM to child pid={pid}");
        send_signal(pid as i32, Signal::Term);

        if wait_for_exit(&mut self.inner, Duration::from_secs(2)) {
            eprintln!("[fluxmirror-proxy] child exited after SIGTERM");
            return;
        }

        eprintln!("[fluxmirror-proxy] child did not exit, sending SIGKILL pid={pid}");
        if let Err(e) = self.inner.kill() {
            eprintln!("[fluxmirror-proxy] WARN kill failed: {e}");
        }
        let _ = self.inner.wait();
        eprintln!("[fluxmirror-proxy] child forcibly terminated");
    }
}

#[derive(Copy, Clone)]
enum Signal {
    Term,
}

#[cfg(unix)]
fn send_signal(pid: i32, sig: Signal) {
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    let signum = match sig {
        Signal::Term => 15, // SIGTERM
    };
    unsafe {
        let _ = kill(pid, signum);
    }
}

#[cfg(not(unix))]
fn send_signal(_pid: i32, _sig: Signal) {
    // On Windows there is no SIGTERM equivalent that matches POSIX
    // semantics; rely on Child::kill() in shutdown() instead.
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}
