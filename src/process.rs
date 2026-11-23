//! Running a child process with a timeout and a cap on the output kept.
//!
//! Shared by version probing and the shell invocation. The child gets no stdin. On timeout (or
//! when it floods its output) it is killed, together with its whole process group on Unix so
//! wrappers such as version-manager shims do not leave orphans behind.

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

/// How often a running child is checked for flooding its output.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// How long to wait for the output readers once the child is gone. A grandchild that outlived
/// it (and kept the pipe open) must not hang the run.
const READER_GRACE: Duration = Duration::from_millis(200);

/// How to run a child.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Kill the child after this long.
    pub timeout: Duration,
    /// Keep at most this many bytes of each output stream. A child that writes more than this
    /// to stdout is killed, since what is needed is at the start.
    pub max_output: usize,
    /// Keep stderr too. When false it is discarded by the operating system.
    pub capture_stderr: bool,
}

/// How the child ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It exited on its own.
    Exited(ExitStatus),
    /// It was killed after the timeout.
    TimedOut,
    /// It was killed after writing more than `max_output` bytes to stdout.
    Flooded,
}

/// What a child printed and how it ended.
#[derive(Debug)]
pub struct Captured {
    /// First bytes of stdout.
    pub stdout: Vec<u8>,
    /// First bytes of stderr (empty unless requested).
    pub stderr: Vec<u8>,
    /// How the child ended.
    pub outcome: Outcome,
}

/// Bytes read from one stream by its reader thread.
struct Reader {
    /// Bytes kept so far.
    buffer: Arc<Mutex<Vec<u8>>>,
    /// Receives a message once the stream reaches its end.
    finished: Receiver<()>,
}

/// Starts a thread that reads `stream` to its end, keeping the first `max` bytes.
///
/// The rest is read and dropped so the child never blocks on a full pipe. `overflow` is set when
/// more than `max` bytes arrive.
fn spawn_reader(
    mut stream: impl Read + Send + 'static,
    max: usize,
    overflow: Arc<AtomicBool>,
) -> Reader {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let (done, finished) = mpsc::channel();
    let kept = Arc::clone(&buffer);
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(read) = stream.read(&mut chunk) {
            if read == 0 {
                break;
            }
            let mut kept = kept.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let room = max.saturating_sub(kept.len());
            kept.extend_from_slice(&chunk[..read.min(room)]);
            if read > room {
                overflow.store(true, Ordering::Relaxed);
            }
        }
        // The receiver may be gone already; nobody is waiting then.
        let _ = done.send(());
    });
    Reader { buffer, finished }
}

impl Reader {
    /// Returns what was read, waiting briefly for the stream to end.
    fn collect(self) -> Vec<u8> {
        let _ = self.finished.recv_timeout(READER_GRACE);
        let mut buffer = self
            .buffer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut buffer)
    }
}

/// Kills `child`, and on Unix its whole process group.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            // SAFETY: plain syscall with integer arguments; the child leads its own group (see
            // `run`), so `-pid` addresses only the child and its descendants.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
        }
    }
    // Also covers Windows, where only the child itself is killed. It may already be gone.
    let _ = child.kill();
}

/// Runs `command` to completion within `limits`.
///
/// Fails only if the process cannot be started or waited for; a timeout is an [`Outcome`].
pub fn run(command: &mut Command, limits: Limits) -> io::Result<Captured> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if limits.capture_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe, which is all `pre_exec` allows between fork and
        // exec. A new session makes the child lead its own process group (so the whole tree can
        // be killed) and detaches it from the controlling terminal. Without that, an
        // interactive shell started from a terminal, in a background process group, is stopped
        // by SIGTTOU/SIGTTIN when it touches the tty and always runs into the timeout.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }

    let mut child = command.spawn()?;
    let flooded = Arc::new(AtomicBool::new(false));
    let stdout = spawn_reader(
        child.stdout.take().expect("stdout is piped"),
        limits.max_output,
        Arc::clone(&flooded),
    );
    let stderr = child.stderr.take().map(|stream| {
        // A noisy stderr is cut off but does not kill the child.
        spawn_reader(stream, limits.max_output, Arc::new(AtomicBool::new(false)))
    });

    let deadline = Instant::now() + limits.timeout;
    let outcome = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            kill_tree(&mut child);
            let _ = child.wait();
            break Outcome::TimedOut;
        }
        if let Some(status) = child.wait_timeout(remaining.min(POLL_INTERVAL))? {
            break Outcome::Exited(status);
        }
        if flooded.load(Ordering::Relaxed) {
            kill_tree(&mut child);
            let _ = child.wait();
            break Outcome::Flooded;
        }
    };
    Ok(Captured {
        stdout: stdout.collect(),
        stderr: stderr.map(Reader::collect).unwrap_or_default(),
        outcome,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut command = Command::new("sh");
        command.arg("-c").arg(script);
        command
    }

    fn limits(timeout_ms: u64) -> Limits {
        Limits {
            timeout: Duration::from_millis(timeout_ms),
            max_output: 4096,
            capture_stderr: true,
        }
    }

    #[test]
    fn captures_stdout_and_stderr() {
        let out = run(&mut sh("echo out; echo err >&2"), limits(2000)).unwrap();
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
        assert!(matches!(out.outcome, Outcome::Exited(s) if s.success()));
    }

    #[test]
    fn stderr_is_discarded_unless_requested() {
        let limits = Limits {
            capture_stderr: false,
            ..limits(2000)
        };
        let out = run(&mut sh("echo err >&2"), limits).unwrap();
        assert!(out.stderr.is_empty());
    }

    #[test]
    fn slow_child_is_killed_at_the_timeout() {
        let started = Instant::now();
        let out = run(&mut sh("sleep 10"), limits(200)).unwrap();
        assert_eq!(out.outcome, Outcome::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn endless_output_is_cut_and_the_child_killed() {
        let started = Instant::now();
        let out = run(&mut sh("yes"), limits(5000)).unwrap();
        assert_eq!(out.outcome, Outcome::Flooded);
        assert_eq!(out.stdout.len(), 4096);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn descendants_are_killed_with_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let script = format!("sleep 30 & echo $! > {}; wait", pid_file.display());
        let out = run(&mut sh(&script), limits(300)).unwrap();
        assert_eq!(out.outcome, Outcome::TimedOut);
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // Signal 0 only checks that the process exists. Allow a moment for it to be reaped.
        let mut alive = true;
        for _ in 0..50 {
            // SAFETY: signal 0 sends nothing.
            alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!alive, "background process {pid} survived");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn child_leads_its_own_session_without_a_terminal() {
        // A session leader has sid == pid, and cannot be stopped by terminal job control.
        let out = run(&mut sh("echo $$; ps -o sid= -p $$"), limits(2000)).unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let mut numbers = text.split_whitespace();
        assert_eq!(numbers.next(), numbers.next(), "{text:?}");
    }

    #[test]
    fn missing_program_is_an_io_error() {
        let result = run(&mut Command::new("/nonexistent/tool"), limits(100));
        assert!(result.is_err());
    }
}
