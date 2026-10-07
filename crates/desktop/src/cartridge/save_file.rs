//! Explicit raw-save storage. Never runs in the core or writes on an emulation error.
#[cfg(all(test, unix))]
mod tests;
use gba_core::{cartridge::SaveDevice, memory::Memory};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}
fn create_new(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}
fn read_image(path: &Path, size: usize) -> io::Result<Option<Vec<u8>>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !metadata.file_type().is_file() {
        return Err(io::Error::other(
            "save path must be a regular file, not a symlink or directory",
        ));
    }
    if metadata.len() != size as u64 {
        return Err(io::Error::other(format!(
            "save file must contain exactly {size} bytes"
        )));
    }
    if metadata.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "save file is read-only",
        ));
    }
    let file = File::open(path)?;
    #[cfg(unix)]
    if !same_file(&metadata, &file.metadata()?) {
        return Err(io::Error::other("save file changed while opening"));
    }
    let mut data = Vec::new();
    file.take(size as u64 + 1).read_to_end(&mut data)?;
    if data.len() != size {
        return Err(io::Error::other("save size changed while reading"));
    }
    Ok(Some(data))
}
struct Lease {
    path: PathBuf,
    file: File,
}
impl Drop for Lease {
    fn drop(&mut self) {
        // Never remove a lock that another actor replaced after we created ours.
        if let (Ok(current), Ok(owned)) = (fs::symlink_metadata(&self.path), self.file.metadata()) {
            if current.is_file() && same_file(&current, &owned) {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}
struct Temporary {
    path: PathBuf,
    retain: bool,
}
impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.retain {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn unique_file(path: &Path, kind: &str) -> io::Result<(Temporary, File)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..128 {
        let name = sibling(
            path,
            &format!(
                ".{kind}.{}.{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ),
        );
        match create_new(&name) {
            Ok(file) => {
                return Ok((
                    Temporary {
                        path: name,
                        retain: false,
                    },
                    file,
                ))
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other(
        "could not reserve a unique save sidecar file",
    ))
}

#[derive(Debug)]
pub(crate) struct Persisted {
    pub(crate) backup: Option<PathBuf>,
}

pub(crate) struct SaveFile {
    path: PathBuf,
    original: Option<Vec<u8>>,
    size: usize,
    _lease: Lease,
}
impl SaveFile {
    pub(crate) fn open(path: &Path, rom: &Path, device: SaveDevice) -> io::Result<Self> {
        #[cfg(not(unix))]
        {
            let _ = (path, rom, device);
            return Err(io::Error::other(
                "save-file persistence currently requires a Unix host",
            ));
        }
        #[cfg(unix)]
        {
            if device == SaveDevice::None {
                return Err(io::Error::other("--save-file requires --save-type"));
            }
            let filename = path
                .file_name()
                .ok_or_else(|| io::Error::other("save file needs a filename"))?;
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let path = parent.canonicalize()?.join(filename);
            let rom = rom.canonicalize()?;
            let rom_metadata = fs::metadata(&rom)?;
            if path == rom
                || fs::metadata(&path)
                    .ok()
                    .is_some_and(|m| same_file(&m, &rom_metadata))
            {
                return Err(io::Error::other("save file must not alias the ROM"));
            }
            let lock_path = sibling(&path, ".lock");
            let file = create_new(&lock_path).map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("cannot lock save file (another process or stale lock): {e}"),
                )
            })?;
            let mut lease = Lease {
                path: lock_path,
                file,
            };
            writeln!(lease.file, "gameboy-rust pid={}", std::process::id())?;
            lease.file.sync_all()?;
            let original = read_image(&path, device.capacity())?;
            Ok(Self {
                path,
                original,
                size: device.capacity(),
                _lease: lease,
            })
        }
    }
    pub(crate) fn initialize(&self, memory: &mut Memory) -> io::Result<()> {
        if let Some(image) = &self.original {
            memory.load_save_image(image).map_err(io::Error::other)?;
        }
        Ok(())
    }
    pub(crate) fn loaded(&self) -> bool {
        self.original.is_some()
    }
    pub(crate) fn persist(&mut self, memory: &Memory) -> io::Result<Option<Persisted>> {
        if memory.save_write_pending() {
            return Err(io::Error::other(
                "save not written: Flash command/operation is incomplete",
            ));
        }
        if !memory.save_modified() {
            return Ok(None);
        }
        let image = memory
            .save_image()
            .ok_or_else(|| io::Error::other("no save image"))?;
        if image.len() != self.size {
            return Err(io::Error::other("save device changed during execution"));
        }
        if self.original.as_deref() == Some(image) {
            return Ok(None);
        }
        if read_image(&self.path, self.size)? != self.original {
            return Err(io::Error::other(
                "save file changed externally; refusing overwrite",
            ));
        }
        let (temp, mut file) = unique_file(&self.path, "tmp")?;
        file.write_all(image)?;
        file.sync_all()?;
        drop(file);
        let backup = if let Some(original) = &self.original {
            let (mut backup, mut file) = unique_file(&self.path, "bak")?;
            file.write_all(original)?;
            file.sync_all()?;
            drop(file);
            backup.retain = true;
            Some(backup.path.clone())
        } else {
            None
        };
        // Recheck immediately before installation. The lock coordinates this frontend,
        // not unrelated programs; this is not an atomic compare-and-swap filesystem API.
        if read_image(&self.path, self.size)? != self.original {
            return Err(io::Error::other(
                "save file changed before replacement; refusing overwrite",
            ));
        }
        if self.original.is_some() {
            fs::rename(&temp.path, &self.path)?;
        } else {
            fs::hard_link(&temp.path, &self.path)?;
        } // No-clobber first creation, even if a new file raced us.
        self.original = Some(image.to_vec());
        fs::remove_file(&temp.path)
            .or_else(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("save installed, but temporary cleanup failed: {e}"),
                )
            })?;
        File::open(self.path.parent().unwrap())
            .and_then(|directory| directory.sync_all())
            .map_err(|e| {
                io::Error::new(
                    e.kind(),
                    format!("save installed, but directory sync failed: {e}"),
                )
            })?;
        Ok(Some(Persisted { backup }))
    }
}
