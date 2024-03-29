//! Functions to read/write from/to files at offset.
use crate::read::ReadError;
use crate::write::WriteError;
use std::fs::File;

//-----------------------------------------------------------------------------
/// Read bytes from file at offset.
pub fn read_bytes_at(buffer: &mut Vec<u8>, file: &File, mut offset: u64) -> Result<(), ReadError> {
    use std::os::windows::fs::FileExt;
    let mut data_read = 0;
    while data_read < buffer.len() {
        data_read += file
            .seek_read(&mut buffer[data_read..], offset)
            .map_err(|err| ReadError::IO(err))?;
        offset += data_read as u64;
    }
    Ok(())
}

//-----------------------------------------------------------------------------
/// Write bytes to file at offset.
pub fn write_bytes_at(buffer: &Vec<u8>, file: &File, mut offset: u64) -> Result<(), WriteError> {
    use std::os::windows::fs::FileExt;
    let mut written = 0;
    while written < buffer.len() {
        written += file
            .seek_write(&buffer[written..], offset)
            .map_err(|err| WriteError::IO(err))?;
        offset += written as u64;
    }
    Ok(())
}
