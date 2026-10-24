use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// Runs `cmd` to completion or kills it after `timeout`. Output is read on
/// helper threads so a chatty child cannot block on a full pipe.
pub fn run(tool: &'static str, cmd: &mut Command, timeout: Duration) -> Result<Output> {
    run_until(tool, cmd, timeout, None)
}

/// [`run`], also killing the child as soon as `cancel` is set.
pub fn run_until(
    tool: &'static str,
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
) -> Result<Output> {
    let (code, stdout, stderr) = wait(tool, cmd, timeout, cancel)?;
    Ok(Output {
        code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// [`run`] for a child whose output is binary: stdout comes back as it was written.
pub fn run_raw(tool: &'static str, cmd: &mut Command, timeout: Duration) -> Result<(Option<i32>, Vec<u8>, String)> {
    let (code, stdout, stderr) = wait(tool, cmd, timeout, None)?;
    Ok((code, stdout, String::from_utf8_lossy(&stderr).into_owned()))
}

fn wait(
    tool: &'static str,
    cmd: &mut Command,
    timeout: Duration,
    cancel: Option<&AtomicBool>,
) -> Result<(Option<i32>, Vec<u8>, Vec<u8>)> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|source| Error::Spawn { tool, source })?;
    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let out_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let err_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Timeout {
                    tool,
                    secs: timeout.as_secs(),
                });
            }
            Ok(None) if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(source) => return Err(Error::Spawn { tool, source }),
        }
    };
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    Ok((status.code(), stdout, stderr))
}

/// Which of the child's outputs a line came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pipe {
    Out,
    Err,
}

/// A piece of the child's output as it arrives. `partial` is the start of a line the child
/// has not finished yet (fastboot prints a step, works, then ends the same line with its
/// result); the whole line comes again once it is finished.
pub struct Chunk<'a> {
    pub pipe: Pipe,
    pub text: &'a str,
    pub partial: bool,
}

/// Runs `cmd` with its output handed to `on_output` as it comes, and returns the exit code.
/// The child runs in a process group of its own, so a timeout or `cancel` ends everything it
/// started, not just the child.
pub fn stream(
    tool: &'static str,
    cmd: &mut Command,
    timeout: Duration,
    cancel: &AtomicBool,
    on_output: &mut dyn FnMut(Chunk),
) -> Result<Option<i32>> {
    use std::sync::mpsc;

    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(cmd, 0);
    let mut child = cmd.spawn().map_err(|source| Error::Spawn { tool, source })?;
    let (tx, rx) = mpsc::channel::<(Pipe, Option<Vec<u8>>)>();
    for (pipe, mut reader) in [
        (
            Pipe::Out,
            Box::new(child.stdout.take().expect("piped stdout")) as Box<dyn Read + Send>,
        ),
        (Pipe::Err, Box::new(child.stderr.take().expect("piped stderr"))),
    ] {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send((pipe, None));
                        break;
                    }
                    Ok(n) => {
                        if tx.send((pipe, Some(buf[..n].to_vec()))).is_err() {
                            break;
                        }
                    }
                }
            }
        });
    }
    drop(tx);

    let deadline = Instant::now() + timeout;
    let mut pending: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    let mut open = 2;
    let mut exited: Option<(std::process::ExitStatus, Instant)> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((pipe, Some(bytes))) => {
                let held = &mut pending[pipe as usize];
                held.extend_from_slice(&bytes);
                while let Some(end) = held.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = held.drain(..=end).collect();
                    let text = String::from_utf8_lossy(&line[..line.len() - 1]);
                    on_output(Chunk {
                        pipe,
                        text: text.trim_end_matches('\r'),
                        partial: false,
                    });
                }
                if !held.is_empty() {
                    on_output(Chunk {
                        pipe,
                        text: &String::from_utf8_lossy(held),
                        partial: true,
                    });
                }
            }
            Ok((pipe, None)) => {
                let held = std::mem::take(&mut pending[pipe as usize]);
                if !held.is_empty() {
                    on_output(Chunk {
                        pipe,
                        text: &String::from_utf8_lossy(&held),
                        partial: false,
                    });
                }
                open -= 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                open = 0;
                thread::sleep(Duration::from_millis(50));
            }
        }
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => exited = Some((status, Instant::now())),
                Ok(None) if Instant::now() >= deadline => {
                    end_group(&mut child);
                    return Err(Error::Timeout {
                        tool,
                        secs: timeout.as_secs(),
                    });
                }
                Ok(None) if cancel.load(Ordering::Relaxed) => {
                    end_group(&mut child);
                    return Err(Error::Cancelled);
                }
                Ok(None) => {}
                Err(source) => return Err(Error::Spawn { tool, source }),
            }
        }
        // Something the child left running may hold the pipes open; it gets two seconds.
        if let Some((status, at)) = exited
            && (open == 0 || at.elapsed() > Duration::from_secs(2))
        {
            return Ok(status.code());
        }
    }
}

/// Ends the child's whole process group: politely, then for good after three seconds.
fn end_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let group = child.id() as libc::pid_t;
        // SAFETY: kill(2) with a negative pid signals the group the child leads.
        unsafe { libc::kill(-group, libc::SIGTERM) };
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Ok(Some(_)) = child.try_wait() {
                unsafe { libc::kill(-group, libc::SIGKILL) };
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}
