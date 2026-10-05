//! Small shared helpers.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

/// Encodes a Rust string as a NUL-terminated UTF-16 buffer for the Win32 API.
pub fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Development-only lifecycle logging.
///
/// Compiled to nothing in release builds (the GUI build has no console), so the
/// production executable stays quiet.
pub fn dlog(message: &str) {
    if cfg!(debug_assertions) {
        eprintln!("tbmon: {message}");
    }
}
