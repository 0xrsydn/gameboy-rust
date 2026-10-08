//! Save-state files for the ROM window. The core never touches the file system.
//!
//! A state is replaced atomically: bytes go to a unique sibling file that is
//! synced and renamed over the destination, so an interrupted write leaves the
//! previous state intact.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// A state is under a megabyte. Refuse anything far larger before reading it.
const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;

fn temporary_sibling(path: &Path) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "state path has no file name")
    })?;
    let mut name = name.to_os_string();
    name.push(format!(
        ".tmp.{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    Ok(path.with_file_name(name))
}

pub(crate) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = temporary_sibling(path)?;
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("state path must be a regular file"));
    }
    if metadata.len() > MAX_STATE_BYTES {
        return Err(io::Error::other("state file is too large"));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(io::Error::other("state file grew while reading"));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("gba-state-file-{name}-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        directory.join("slot.state")
    }

    #[test]
    fn write_replaces_the_previous_state_and_leaves_no_temporary_files() {
        let path = scratch("replace");
        write(&path, b"first").unwrap();
        write(&path, b"second state").unwrap();
        assert_eq!(read(&path).unwrap(), b"second state");
        let leftovers = fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(leftovers, 1);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_and_oversized_files_are_errors() {
        let path = scratch("errors");
        assert_eq!(read(&path).unwrap_err().kind(), io::ErrorKind::NotFound);
        let file = File::create(&path).unwrap();
        file.set_len(MAX_STATE_BYTES + 1).unwrap();
        assert!(read(&path).is_err());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
