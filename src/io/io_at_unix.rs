//! Functions to read/write from/to files at specified offset wrapping pread/write.
use crate::read::ReadError;
use crate::write::WriteError;
use std::fs::File;
use std::os::raw::c_void;
use std::os::unix::io::{AsRawFd, RawFd};

//----------------------------------------------------------------------------
// Mapping C types to Rust.
pub type ssize_t = isize;
pub type size_t = usize;
pub type off_t = isize;
extern "C" {
    fn pread(fd: RawFd, buf: *mut c_void, count: size_t, offset: off_t) -> ssize_t;
    fn pwrite(fd: RawFd, buf: *mut c_void, count: size_t, offset: off_t) -> ssize_t;
}

//-----------------------------------------------------------------------------
/// Read bytes from file at offset, inkoking `pread`.
pub fn read_bytes_at(buffer: &mut Vec<u8>, file: &File, mut offset: u64) -> Result<(), ReadError> {
    //td::fs::metadata(file).map_err(|err| ReadError::IO(err))?;
    let mut data_read = 0;
    let fd = file.as_raw_fd();
    while data_read < buffer.len() {
        let sz = buffer.len() - data_read as usize;
        data_read += unsafe {
            let ret = pread(
                fd,
                buffer.as_mut_ptr().offset(data_read as isize) as *mut c_void,
                sz as size_t,
                offset as off_t,
            );
            if ret < 0 {
                return Err(ReadError::Other(format!(
                    "{:?}",
                    std::io::Error::last_os_error()
                )));
            } else {
                ret as usize
            }
        };
        offset += data_read as u64;
    }
    Ok(())
}

//-----------------------------------------------------------------------------
/// Write bytes to file at offset, invoking `pwrite`.
pub fn write_bytes_at(buffer: &Vec<u8>, file: &File, mut offset: u64) -> Result<(), WriteError> {
    let fd = file.as_raw_fd();
    let mut written = 0;
    while written < buffer.len() {
        let sz = buffer.len() - written as usize;
        written += unsafe {
            let ret = pwrite(
                fd,
                buffer.as_ptr().offset(written as isize) as *mut c_void,
                sz as size_t,
                offset as off_t,
            );
            if ret < 0 {
                return Err(WriteError::Other(format!(
                    "{:?}",
                    std::io::Error::last_os_error()
                )));
            } else {
                ret as usize
            }
        };
        offset += written as u64;
    }
    Ok(())
}
