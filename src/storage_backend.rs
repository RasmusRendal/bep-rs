use std::{
    fs::OpenOptions,
    io::{self, Read, Seek, SeekFrom, Write},
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Invalid path")]
    InvalidPath,
    #[error("IO Error: {0}")]
    IOError(io::Error),
    #[error("Other")]
    Other,
}

pub trait StorageBackend {
    fn write_block(&self, url: &str, offset: usize, data: &[u8]) -> Result<(), StorageError>;
    fn read_block(&self, url: &str, offset: usize, len: usize) -> Result<Vec<u8>, StorageError>;
}

pub struct LocalStorageBackend {}

impl From<io::Error> for StorageError {
    fn from(value: io::Error) -> Self {
        StorageError::IOError(value)
    }
}

impl StorageBackend for LocalStorageBackend {
    fn write_block(&self, url: &str, offset: usize, data: &[u8]) -> Result<(), StorageError> {
        let mut f = OpenOptions::new().write(true).create(true).open(url)?;
        f.seek(SeekFrom::Start(offset as u64))?;
        f.write_all(data)?;
        Ok(())
    }

    fn read_block(&self, url: &str, offset: usize, len: usize) -> Result<Vec<u8>, StorageError> {
        let mut f = OpenOptions::new().read(true).open(url)?;
        f.seek(SeekFrom::Start(offset as u64))?;
        let mut buf = vec![0; len];
        f.read_exact(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localstorage() {
        let sb = LocalStorageBackend {};
        let test_dir = tempfile::tempdir().unwrap().into_path();
        let file_url = format!("{}/testfile", test_dir.to_str().unwrap());
        sb.write_block(&file_url, 6, b"world").unwrap();
        let written_string = sb.read_block(&file_url, 6, 5).unwrap();
        assert_eq!(b"world", &written_string[..]);
        sb.write_block(&file_url, 0, b"hello ").unwrap();

        let written_string = sb.read_block(&file_url, 0, 11).unwrap();
        assert_eq!(b"hello world", &written_string[..]);
    }
}
