use std::path::PathBuf;

use rexux_utils_absolute_path::AbsolutePathBuf;

/// Runtime paths needed by exec-server child processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecServerRuntimePaths {
    /// Stable path to the Rexux executable used to launch hidden helper modes.
    pub rexux_self_exe: AbsolutePathBuf,
    /// Path to the Linux sandbox helper alias used when the platform sandbox
    /// needs to re-enter Rexux by argv0.
    pub rexux_linux_sandbox_exe: Option<AbsolutePathBuf>,
}

impl ExecServerRuntimePaths {
    pub fn from_optional_paths(
        rexux_self_exe: Option<PathBuf>,
        rexux_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let rexux_self_exe = rexux_self_exe.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Rexux executable path is not configured",
            )
        })?;
        Self::new(rexux_self_exe, rexux_linux_sandbox_exe)
    }

    pub fn new(
        rexux_self_exe: PathBuf,
        rexux_linux_sandbox_exe: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            rexux_self_exe: absolute_path(rexux_self_exe)?,
            rexux_linux_sandbox_exe: rexux_linux_sandbox_exe.map(absolute_path).transpose()?,
        })
    }
}

fn absolute_path(path: PathBuf) -> std::io::Result<AbsolutePathBuf> {
    AbsolutePathBuf::from_absolute_path(path.as_path())
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))
}
