//! Read/write data at offset from/to files, conditionally including UNIX/Windows
//! implementations.
#[cfg(any(unix))]
pub mod io_at_unix;

#[cfg(any(windows))]
pub mod io_at_windows;
