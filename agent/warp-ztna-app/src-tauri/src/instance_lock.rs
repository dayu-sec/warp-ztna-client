use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    thread,
    time::Duration,
};

pub(crate) enum LockOutcome {
    Acquired(File),
    Held(Option<String>),
    Degraded(String),
}

pub(crate) fn acquire(dir: &Path, attempts: u32, interval: Duration) -> LockOutcome {
    if let Err(error) = std::fs::create_dir_all(dir) {
        return LockOutcome::Degraded(error.to_string());
    }
    let path = dir.join("instance.lock");
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(error) => return LockOutcome::Degraded(error.to_string()),
    };
    for _ in 0..attempts {
        match file.try_lock() {
            Ok(()) => {
                let _ = file.set_len(0);
                let _ = file.seek(SeekFrom::Start(0));
                let _ = writeln!(file, "{}", std::process::id());
                return LockOutcome::Acquired(file);
            }
            Err(std::fs::TryLockError::WouldBlock) => thread::sleep(interval),
            Err(std::fs::TryLockError::Error(error)) => {
                return LockOutcome::Degraded(error.to_string());
            }
        }
    }
    let mut pid = String::new();
    let _ = file.read_to_string(&mut pid);
    let pid = pid.trim().to_owned();
    LockOutcome::Held((!pid.is_empty()).then_some(pid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("warp-ztna-lock-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn second_acquire_is_held_until_released() {
        let dir = temp_dir("held");
        let first = acquire(&dir, 1, Duration::ZERO);
        let LockOutcome::Acquired(file) = first else {
            panic!("first acquire failed");
        };
        assert!(matches!(acquire(&dir, 1, Duration::ZERO), LockOutcome::Held(_)));
        drop(file);
        assert!(matches!(acquire(&dir, 1, Duration::ZERO), LockOutcome::Acquired(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
