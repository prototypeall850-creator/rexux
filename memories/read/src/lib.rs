//! Read-path helpers for Rexux memories.
//!
//! This crate owns memory injection, memory citation parsing, and telemetry
//! classification for read access to the memory folder. It intentionally does
//! not depend on the memory write pipeline.

pub mod citations;
mod metrics;
pub mod usage;

use rexux_utils_absolute_path::AbsolutePathBuf;

pub fn memory_root(rexux_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    rexux_home.join("memories")
}
