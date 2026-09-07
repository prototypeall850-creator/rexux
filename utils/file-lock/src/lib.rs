//! Advisory file locking with an `fcntl` fallback for filesystems where
//! `flock` is unsupported (notably parts of Android/Termux storage, where
//! `flock` returns `ENOSYS`/`EOPNOTSUPP` and std reports "lock() not supported").
//!
//! The API mirrors [`std::fs::File::try_lock`] / `try_lock_shared`: `Ok(())`
//! on success, `Err(TryLockError::WouldBlock)` when another holder owns the
//! lock, and `Err(TryLockError::Error(..))` for genuine I/O failures.
//!
//! `fcntl` (POSIX `F_SETLK`) locks are process-associated rather than
//! open-file-description associated, so — like any advisory scheme — they only
//! guard against *other processes*, not threads in the same process. All
//! current call sites use locks for exactly that purpose.

use std::fs::File;
use std::io;

/// Non-blocking lock attempt outcome, mirroring [`std::fs::TryLockError`].
pub type TryLockError = std::fs::TryLockError;

/// Try to acquire an exclusive advisory lock on `file`.
///
/// Falls back to `fcntl` (`F_SETLK`) when `flock` reports the operation as
/// unsupported on this filesystem.
pub fn try_lock_exclusive(file: &File) -> Result<(), TryLockError> {
    match file.try_lock() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        Err(TryLockError::Error(err)) if is_lock_unsupported(&err) => fcntl_exclusive(file),
        Err(err) => Err(err),
    }
}

/// Try to acquire a shared advisory lock on `file`, with the same `fcntl`
/// fallback as [`try_lock_exclusive`].
pub fn try_lock_shared(file: &File) -> Result<(), TryLockError> {
    match file.try_lock_shared() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        Err(TryLockError::Error(err)) if is_lock_unsupported(&err) => fcntl_shared(file),
        Err(err) => Err(err),
    }
}

fn is_lock_unsupported(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(code) if code == libc_enosys() || code == libc_eopnotsupp()
    ) || err.kind() == io::ErrorKind::Unsupported
}

#[cfg(unix)]
fn libc_enosys() -> i32 {
    libc_enosys_impl()
}

#[cfg(unix)]
fn libc_eopnotsupp() -> i32 {
    libc_eopnotsupp_impl()
}

#[cfg(not(unix))]
fn libc_enosys() -> i32 {
    -1
}

#[cfg(not(unix))]
fn libc_eopnotsupp() -> i32 {
    -1
}

#[cfg(unix)]
fn libc_enosys_impl() -> i32 {
    // ENOSYS value is stable across Linux/Android/libc targets.
    38
}

#[cfg(unix)]
fn libc_eopnotsupp_impl() -> i32 {
    // EOPNOTSUPP value is stable across Linux/Android/libc targets.
    95
}

#[cfg(unix)]
fn fcntl_exclusive(file: &File) -> Result<(), TryLockError> {
    use rustix::fs::FlockOperation;
    use std::os::unix::io::AsFd;
    rustix::fs::fcntl_lock(file.as_fd(), FlockOperation::NonBlockingLockExclusive).map_err(
        |errno| {
            let err = io::Error::from(errno);
            if err.kind() == io::ErrorKind::WouldBlock {
                TryLockError::WouldBlock
            } else {
                TryLockError::Error(err)
            }
        },
    )
}

#[cfg(unix)]
fn fcntl_shared(file: &File) -> Result<(), TryLockError> {
    use rustix::fs::FlockOperation;
    use std::os::unix::io::AsFd;
    rustix::fs::fcntl_lock(file.as_fd(), FlockOperation::NonBlockingLockShared).map_err(|errno| {
        let err = io::Error::from(errno);
        if err.kind() == io::ErrorKind::WouldBlock {
            TryLockError::WouldBlock
        } else {
            TryLockError::Error(err)
        }
    })
}

#[cfg(not(unix))]
fn fcntl_exclusive(_file: &File) -> Result<(), TryLockError> {
    Err(TryLockError::Error(io::Error::new(
        io::ErrorKind::Unsupported,
        "fcntl file locking is only supported on unix",
    )))
}

#[cfg(not(unix))]
fn fcntl_shared(_file: &File) -> Result<(), TryLockError> {
    Err(TryLockError::Error(io::Error::new(
        io::ErrorKind::Unsupported,
        "fcntl file locking is only supported on unix",
    )))
}

/// Try to acquire an exclusive advisory lock on `file`, blocking until it
/// is available. Uses the same flock-then-fcntl strategy as
/// [`try_lock_exclusive`].
pub fn lock_exclusive_blocking(file: &File) -> io::Result<()> {
    match file.lock() {
        Ok(()) => Ok(()),
        Err(err) if is_lock_unsupported(&err) => {
            lock_exclusive_blocking_fcntl(file)
        }
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn lock_exclusive_blocking_fcntl(file: &File) -> io::Result<()> {
    use rustix::fs::FlockOperation;
    use std::os::unix::io::AsFd;
    // Blocking fcntl exclusive lock (F_SETLKW): retry on interrupt.
    loop {
        match rustix::fs::fcntl_lock(file.as_fd(), FlockOperation::LockExclusive) {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::INTR) => continue,
            Err(errno) => return Err(io::Error::from(errno)),
        }
    }
}

#[cfg(not(unix))]
fn lock_exclusive_blocking_fcntl(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "fcntl file locking is only supported on unix",
    ))
}

/// [`try_lock_exclusive`] for call sites holding a plain [`File`] reference
/// under a different name; kept as an alias so daemon code reads naturally.
pub fn try_lock_exclusive_std(file: &File) -> Result<(), TryLockError> {
    try_lock_exclusive(file)
}

/// Non-blocking exclusive lock on any fd-like handle (e.g. `tokio::fs::File`),
/// with the same flock-then-fcntl strategy as [`try_lock_exclusive`].
#[cfg(unix)]
pub fn try_lock_exclusive_fd<Fd: std::os::unix::io::AsFd>(fd: Fd) -> Result<(), TryLockError> {
    use rustix::fs::FlockOperation;
    // First try flock; fall back to fcntl when the filesystem rejects it.
    match rustix::fs::flock(fd.as_fd(), FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::WOULDBLOCK) => Err(TryLockError::WouldBlock),
        Err(rustix::io::Errno::NOSYS) | Err(rustix::io::Errno::NOTSUP) => {
            rustix::fs::fcntl_lock(fd.as_fd(), FlockOperation::NonBlockingLockExclusive).map_err(
                |errno| {
                    let err = std::io::Error::from(errno);
                    if err.kind() == std::io::ErrorKind::WouldBlock {
                        TryLockError::WouldBlock
                    } else {
                        TryLockError::Error(err)
                    }
                },
            )
        }
        Err(errno) => Err(TryLockError::Error(std::io::Error::from(errno))),
    }
}

#[cfg(not(unix))]
pub fn try_lock_exclusive_fd<Fd>(_fd: Fd) -> Result<(), TryLockError> {
    Err(TryLockError::Error(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fd file locking is only supported on unix",
    )))
}

/// Release any advisory lock held on `file`.
///
/// Best-effort: releases both `flock` and `fcntl` locks so files locked via
/// the fallback path are always unlocked, even on filesystems where `flock`
/// itself errors.
pub fn unlock(file: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use rustix::fs::FlockOperation;
        use std::os::unix::io::AsFd;
        let _ = rustix::fs::fcntl_lock(file.as_fd(), FlockOperation::NonBlockingUnlock);
    }
    match file.unlock() {
        Ok(()) => Ok(()),
        Err(err) if is_lock_unsupported(&err) => Ok(()),
        Err(err) => Err(err),
    }
}
