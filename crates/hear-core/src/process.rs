//! Bounded subprocess lifetime with concurrent pipe draining and cancellation.
use anyhow::{Context, Result, bail};
use std::{
    io::{Read, Write},
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
struct ChildGuard {
    child: Child,
    owns_group: bool,
    complete: bool,
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !self.complete {
            #[cfg(unix)]
            if self.owns_group {
                // SAFETY: this child was launched in a new process group with its PID as group ID.
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
            }
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
fn drain(mut stream: impl Read, forward: bool) -> Result<Vec<u8>> {
    let mut captured = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        // Never hold stderr's global lock across a blocking read from the child.
        if forward {
            std::io::stderr().lock().write_all(&buf[..n])?;
        }
        if captured.len() + n > 16 * 1024 * 1024 {
            bail!("helper output exceeded 16 MiB");
        }
        captured.extend_from_slice(&buf[..n]);
    }
    Ok(captured)
}
pub fn run(
    command: &mut Command,
    input: Option<Vec<u8>>,
    cancellation: &Cancellation,
    timeout: Duration,
    forward: bool,
) -> Result<Output> {
    let owns_group = std::env::var_os("HEAR_PROCESS_GROUP").is_none();
    #[cfg(unix)]
    if owns_group {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.env("HEAR_PROCESS_GROUP", "1");
    let mut child = ChildGuard {
        owns_group,
        complete: false,
        child: command
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("could not launch helper")?,
    };
    let stdout = child.child.stdout.take().context("missing helper stdout")?;
    let stderr = child.child.stderr.take().context("missing helper stderr")?;
    let out = thread::spawn(move || drain(stdout, forward));
    let err = thread::spawn(move || drain(stderr, forward));
    let writer = input.map(|input| {
        let mut stdin = child.child.stdin.take().expect("piped stdin");
        thread::spawn(move || stdin.write_all(&input))
    });
    let start = Instant::now();
    let mut exited = None;
    let status = loop {
        if cancellation.is_cancelled() {
            bail!("operation cancelled");
        }
        if start.elapsed() >= timeout {
            bail!("helper exceeded its {} second deadline", timeout.as_secs());
        }
        if exited.is_none() {
            exited = child.child.try_wait()?;
        }
        if let Some(status) = exited
            && out.is_finished()
            && err.is_finished()
            && writer.as_ref().is_none_or(|w| w.is_finished())
        {
            break status;
        }
        thread::sleep(Duration::from_millis(20));
    };
    if let Some(writer) = writer {
        writer
            .join()
            .map_err(|_| anyhow::anyhow!("helper input thread panicked"))??;
    }
    let stdout = out
        .join()
        .map_err(|_| anyhow::anyhow!("helper output thread panicked"))??;
    let stderr = err
        .join()
        .map_err(|_| anyhow::anyhow!("helper error thread panicked"))??;
    child.complete = true;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}
/// A worker whose lifetime is owned by the UI. Dropping it cancels and reaps its helper.
pub struct Job {
    cancellation: Cancellation,
    worker: Option<thread::JoinHandle<()>>,
}
impl Job {
    pub fn spawn(work: impl FnOnce(Cancellation) + Send + 'static) -> Self {
        let cancellation = Cancellation::default();
        let signal = cancellation.clone();
        Self {
            cancellation,
            worker: Some(thread::spawn(move || work(signal))),
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn dropping_job_cancels_and_waits_for_helper() {
        let (tx, rx) = std::sync::mpsc::channel();
        let job = Job::spawn(move |cancellation| {
            let result = run(
                Command::new("sleep").arg("10"),
                None,
                &cancellation,
                Duration::from_secs(2),
                false,
            );
            tx.send(result.unwrap_err().to_string()).unwrap();
        });
        drop(job);
        assert_eq!(rx.try_recv().unwrap(), "operation cancelled");
    }
    #[cfg(unix)]
    #[test]
    fn drains_both_pipes_and_times_out() {
        let mut c = Command::new("sh");
        c.args(["-c", "i=0; while [ $i -lt 3000 ]; do echo abcdefghijklmnopqrstuvwxyz; echo abcdefghijklmnopqrstuvwxyz >&2; i=$((i+1)); done"]);
        let o = run(
            &mut c,
            None,
            &Cancellation::default(),
            Duration::from_secs(5),
            false,
        )
        .unwrap();
        assert!(o.status.success());
        assert!(o.stdout.len() > 65536);
        assert!(o.stderr.len() > 65536);
        let mut c = Command::new("sleep");
        c.arg("5");
        assert!(
            run(
                &mut c,
                None,
                &Cancellation::default(),
                Duration::from_millis(50),
                false
            )
            .is_err()
        );
    }
}
