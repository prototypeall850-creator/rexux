//! Advisory file locking with an `fcntl` fallback for filesystems where
//! `flock` is unsupported (notably parts of Android/Termux storage, where
//! `flock` returns `ENOSYS`/`EOPNOTSUPP` and std reports "lock() not supported").
//!
//! The API mirrors [`std::fs::File::try_lock`] / `try_lock_shared`: `Ok(())`
//! on success, `Err(TryLockError::WouldBlock)` when another holder owns the
//! lock, and `Err(TryLockError::Error(..))` for genuine I/O failures.
//!
//! If the filesystem supports *no* advisory locking at all (both `flock` and
//! `fcntl` report `ENOSYS`/`EOPNOTSUPP`/`ENOLCK`), the helpers log one
//! warning and report success rather than failing the whole command: the
//! process still runs, just without cross-process mutual exclusion.
//!
//! `fcntl` (POSIX `F_SETLK`) locks are process-associated rather than
//! open-file-description associated, so — like any advisory scheme — they only
//! guard against *other processes*, not threads in the same process. All
//! current call sites use locks for exactly that purpose.

use std::fs::File;
use std::io;
use std::sync::OnceLock;

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
        Err(TryLockError::Error(err)) if is_lock_unsupported(&err) => fcntl_try_exclusive(file),
        Err(err) => Err(err),
    }
}

/// Try to acquire a shared advisory lock on `file`, with the same `fcntl`
/// fallback as [`try_lock_exclusive`].
pub fn try_lock_shared(file: &File) -> Result<(), TryLockError> {
    match file.try_lock_shared() {
        Ok(()) => Ok(()),
        Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
        Err(TryLockError::Error(err)) if is_lock_unsupported(&err) => fcntl_try_shared(file),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn fcntl_try_exclusive(file: &File) -> Result<(), TryLockError> {
    use std::os::unix::io::AsFd;
    fcntl_try_lock(
        file.as_fd(),
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    )
}

#[cfg(not(unix))]
fn fcntl_try_exclusive(_file: &File) -> Result<(), TryLockError> {
    Err(TryLockError::Error(io::Error::new(
        io::ErrorKind::Unsupported,
        "fcntl file locking is only supported on unix",
    )))
}

#[cfg(unix)]
fn fcntl_try_shared(file: &File) -> Result<(), TryLockError> {
    use std::os::unix::io::AsFd;
    fcntl_try_lock(
        file.as_fd(),
        rustix::fs::FlockOperation::NonBlockingLockShared,
    )
}

#[cfg(not(unix))]
fn fcntl_try_shared(_file: &File) -> Result<(), TryLockError> {
    Err(TryLockError::Error(io::Error::new(
        io::ErrorKind::Unsupported,
        "fcntl file locking is only supported on unix",
    )))
}

/// Acquire an exclusive advisory lock on `file`, blocking until it is
/// available. Uses the same flock-then-fcntl strategy as
/// [`try_lock_exclusive`].
pub fn lock_exclusive_blocking(file: &File) -> io::Result<()> {
    match file.lock() {
        Ok(()) => Ok(()),
        Err(err) if is_lock_unsupported(&err) => fcntl_blocking_exclusive(file),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn fcntl_blocking_exclusive(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsFd;
    fcntl_blocking_lock(file.as_fd(), rustix::fs::FlockOperation::LockExclusive)
}

#[cfg(not(unix))]
fn fcntl_blocking_exclusive(_file: &File) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "fcntl file locking is only supported on unix",
    ))
}

/// Acquire a shared advisory lock on `file`, blocking until it is available.
/// Uses the same flock-then-fcntl strategy as [`lock_exclusive_blocking`].
pub fn lock_shared_blocking(file: &File) -> io::Result<()> {
    match file.lock_shared() {
        Ok(()) => Ok(()),
        Err(err) if is_lock_unsupported(&err) => fcntl_blocking_shared(file),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn fcntl_blocking_shared(file: &File) -> io::Result<()> {
    use std::os::unix::io::AsFd;
    fcntl_blocking_lock(file.as_fd(), rustix::fs::FlockOperation::LockShared)
}

#[cfg(not(unix))]
fn fcntl_blocking_shared(_file: &File) -> io::Result<()> {
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
            fcntl_try_lock(fd, FlockOperation::NonBlockingLockExclusive)
        }
        Err(errno) => Err(TryLockError::Error(io::Error::from(errno))),
    }
}

#[cfg(not(unix))]
pub fn try_lock_exclusive_fd<Fd>(_fd: Fd) -> Result<(), TryLockError> {
    Err(TryLockError::Error(io::Error::new(
        io::ErrorKind::Unsupported,
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

fn is_lock_unsupported(err: &io::Error) -> bool {
    matches!(
        err.raw_os_error(),
        Some(code) if code == libc_enosys() || code == libc_eopnotsupp() || code == libc_enolck()
    ) || err.kind() == io::ErrorKind::Unsupported
}

#[cfg(unix)]
fn libc_enosys() -> i32 {
    // ENOSYS value is stable across Linux/Android/libc targets.
    38
}

#[cfg(unix)]
fn libc_eopnotsupp() -> i32 {
    // EOPNOTSUPP value is stable across Linux/Android/libc targets.
    95
}

#[cfg(unix)]
fn libc_enolck() -> i32 {
    // ENOLCK value is stable across Linux/Android/libc targets.
    37
}

#[cfg(not(unix))]
fn libc_enosys() -> i32 {
    -1
}

#[cfg(not(unix))]
fn libc_eopnotsupp() -> i32 {
    -1
}

#[cfg(not(unix))]
fn libc_enolck() -> i32 {
    -1
}

/// Warn once per process when the filesystem supports no advisory locking at
/// all; callers then proceed without the lock instead of failing.
fn warn_locking_unsupported_once() {
    static WARNED: OnceLock<()> = OnceLock::new();
    if WARNED.set(()).is_ok() {
        tracing::warn!(
            "advisory file locking is unsupported on this filesystem; \
             proceeding without cross-process locks"
        );
    }
}

/// Non-blocking `fcntl` try-lock. Maps contention (`EAGAIN`/`EACCES`) to
/// [`TryLockError::WouldBlock`] and treats "unsupported by filesystem" as
/// success (after warning once).
#[cfg(unix)]
fn fcntl_try_lock<Fd: std::os::unix::io::AsFd>(
    fd: Fd,
    operation: rustix::fs::FlockOperation,
) -> Result<(), TryLockError> {
    use rustix::io::Errno;
    match rustix::fs::fcntl_lock(fd.as_fd(), operation) {
        Ok(()) => Ok(()),
        Err(Errno::NOSYS | Errno::NOTSUP | Errno::NOLCK) => {
            warn_locking_unsupported_once();
            Ok(())
        }
        Err(Errno::WOULDBLOCK | Errno::ACCESS) => Err(TryLockError::WouldBlock),
        Err(errno) => Err(TryLockError::Error(io::Error::from(errno))),
    }
}

/// Blocking `fcntl` lock (`F_SETLKW`/`F_SETLKR` semantics): retries on
/// interrupt and treats "unsupported by filesystem" as success (after
/// warning once).
#[cfg(unix)]
fn fcntl_blocking_lock<Fd: std::os::unix::io::AsFd>(
    fd: Fd,
    operation: rustix::fs::FlockOperation,
) -> io::Result<()> {
    use rustix::io::Errno;
    loop {
        match rustix::fs::fcntl_lock(fd.as_fd(), operation) {
            Ok(()) => return Ok(()),
            Err(Errno::INTR) => continue,
            Err(Errno::NOSYS | Errno::NOTSUP | Errno::NOLCK) => {
                warn_locking_unsupported_once();
                return Ok(());
            }
            Err(errno) => return Err(io::Error::from(errno)),
        }
    }
}

#[cfg(not(unix))]
fn warn_locking_unsupported_once() {}
