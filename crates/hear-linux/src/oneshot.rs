use crate::{config::Config, recording::Recorder};
use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

struct Instance {
    _lock: File,
    listener: UnixListener,
    socket: PathBuf,
}
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}
fn connect_or_listen() -> Result<Option<Instance>> {
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let directory = base.join(format!("hear-oneshot-{uid}"));
    connect_or_listen_at(directory)
}
fn connect_or_listen_at(directory: PathBuf) -> Result<Option<Instance>> {
    // SAFETY: geteuid has no preconditions.
    let uid = unsafe { libc::geteuid() };
    match fs::DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let metadata = fs::symlink_metadata(&directory)?;
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
        bail!("one-shot runtime directory must be private and owned by the current user");
    }
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("lock"))?;
    let socket = directory.join("control.sock");
    // SAFETY: the file descriptor belongs to this live File; flock neither owns nor closes it.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error.into());
        }
        let start = Instant::now();
        loop {
            match UnixStream::connect(&socket) {
                Ok(mut stream) => {
                    stream.write_all(b"stop")?;
                    return Ok(None);
                }
                Err(_) if start.elapsed() < Duration::from_secs(2) => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                Err(e) => {
                    return Err(e).context("existing one-shot instance is not accepting commands");
                }
            }
        }
    }
    // Exclusive lock establishes ownership before replacing a stale socket.
    match fs::remove_file(&socket) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    Ok(Some(Instance {
        _lock: lock,
        listener,
        socket,
    }))
}
pub fn run() -> Result<()> {
    let Some(instance) = connect_or_listen()? else {
        return Ok(());
    };
    let config = Config::load()?;
    config.hear.preflight()?;
    let cancel = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, cancel.clone())?;
    }
    let target = crate::delivery::capture_target();
    let recorder = Recorder::start()?;
    eprintln!("Recording… (run `hear-app oneshot` again to stop)");
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        recorder.check()?;
        match instance.listener.accept() {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut recording = hear_core::helper::Recording::new(recorder.finish()?);
    let cancellation = hear_core::process::Cancellation::default();
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    let signal = cancellation.clone();
    let watcher = std::thread::spawn(move || {
        while !finished.load(Ordering::Relaxed) {
            if cancel.load(Ordering::Relaxed) {
                signal.cancel();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let result = crate::transcriber::run(&recording, &config.hear, &cancellation);
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let transcript = result?;
    recording.remember_transcript(&transcript);
    if crate::delivery::deliver(
        &transcript,
        config.paste_automatically,
        &config,
        target.as_ref(),
    )? {
        eprintln!("Pasted.");
    } else {
        eprintln!("Copied to clipboard.");
    }
    print!("{transcript}");
    recording.delivered();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exclusive_instance_toggle_and_stale_socket_recovery() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("runtime");
        let first = connect_or_listen_at(directory.clone()).unwrap().unwrap();
        assert!(connect_or_listen_at(directory.clone()).unwrap().is_none());
        assert!(first.listener.accept().is_ok());
        drop(first);
        let stale = UnixListener::bind(directory.join("control.sock")).unwrap();
        drop(stale);
        assert!(connect_or_listen_at(directory).unwrap().is_some());
    }
    #[test]
    fn rejects_symlink_runtime_directory() {
        let root = tempfile::tempdir().unwrap();
        let link = root.path().join("link");
        std::os::unix::fs::symlink(root.path(), &link).unwrap();
        assert!(connect_or_listen_at(link).is_err());
    }
}
