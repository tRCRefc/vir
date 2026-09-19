//! Frontend-owned crash snapshots. Editor transactions have no filesystem effects.
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::{buffer::TextBuffer, editor::Editor};

const MAGIC: &[u8; 8] = b"VIRREC01";
const PREFIX: &str = ".vir-recovery-";
const LIMIT: u64 = 128 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// Informational only: recovery never grants overwrite rights to this path.
    pub source: String,
    pub buffer: TextBuffer,
}

#[derive(Default)]
pub struct Recovery {
    directory: Option<PathBuf>,
    path: Option<PathBuf>,
    last: Option<Snapshot>,
}

impl Recovery {
    /// Disabled for injectable library frontends; the CLI enables snapshots.
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn in_directory(directory: PathBuf) -> Self {
        Self {
            directory: Some(directory),
            ..Self::default()
        }
    }

    pub fn for_editor(editor: &Editor) -> io::Result<Self> {
        let directory = if let Some(directory) = std::env::var_os("VIR_RECOVERY_DIR") {
            PathBuf::from(directory)
        } else {
            editor
                .path()
                .and_then(Path::parent)
                .filter(|path| !path.as_os_str().is_empty() && path.is_dir())
                .map(Path::to_owned)
                .unwrap_or(std::env::current_dir()?)
        };
        Ok(Self::in_directory(directory))
    }

    pub fn is_enabled(&self) -> bool {
        self.directory.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn observe(&mut self, editor: &Editor) -> io::Result<()> {
        self.checkpoint(editor.path(), editor.buffer(), editor.is_modified())
    }

    pub fn checkpoint(
        &mut self,
        source: Option<&Path>,
        buffer: &TextBuffer,
        dirty: bool,
    ) -> io::Result<()> {
        let Some(directory) = self.directory.as_ref() else {
            return Ok(());
        };
        if !dirty {
            return self.finish();
        }
        let source = match source {
            Some(path) => std::path::absolute(path)?.to_string_lossy().into_owned(),
            None => String::new(),
        };
        if self
            .last
            .as_ref()
            .is_some_and(|last| last.source == source && &last.buffer == buffer)
        {
            return Ok(());
        }
        let snapshot = Snapshot {
            source,
            buffer: buffer.clone(),
        };
        let bytes = encode(&snapshot)?;
        let mut staged = tempfile::Builder::new()
            .prefix(".vir-stage-")
            .tempfile_in(directory)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        if let Some(path) = &self.path {
            // Never follow a substituted link or publish over a non-regular file.
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_file() {
                return Err(invalid("recovery target is not a regular file"));
            }
            staged.persist(path).map_err(|error| error.error)?;
        } else {
            let suffix = staged.path().file_name().unwrap().to_string_lossy();
            let path = directory.join(format!(
                "{PREFIX}{}",
                suffix.trim_start_matches(".vir-stage-")
            ));
            staged
                .persist_noclobber(&path)
                .map_err(|error| error.error)?;
            // Retain ownership even if directory sync fails, so a retry is safe.
            self.path = Some(path);
        }
        // Publication already changed the on-disk snapshot. Invalidate the cache
        // before a potentially failing directory sync, including if text reverts.
        self.last = None;
        sync_directory(directory)?;
        self.last = Some(snapshot);
        Ok(())
    }

    /// Called only after successful save/reload or deliberate normal exit.
    /// No Drop cleanup: error returns and unwinding must retain the last snapshot.
    pub fn finish(&mut self) -> io::Result<()> {
        if let Some(path) = &self.path {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            self.path = None;
            self.last = None;
            sync_directory(self.directory.as_ref().unwrap())?;
        }
        Ok(())
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path; // Portable std does not provide directory durability here.
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

// FNV-1a detects accidental corruption, not malicious changes/authenticity.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn encode(snapshot: &Snapshot) -> io::Result<Vec<u8>> {
    let text = snapshot.buffer.to_text();
    let size = 32u64
        .saturating_add(snapshot.source.len() as u64)
        .saturating_add(text.len() as u64);
    if size > LIMIT {
        return Err(invalid("recovery snapshot exceeds 128 MiB"));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(snapshot.source.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u64).to_le_bytes());
    bytes.extend_from_slice(snapshot.source.as_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(&checksum(&bytes).to_le_bytes());
    Ok(bytes)
}

pub fn read(path: &Path) -> io::Result<Snapshot> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err(invalid("invalid recovery file type or size"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() < 32 || bytes.len() as u64 > LIMIT || &bytes[..8] != MAGIC {
        return Err(invalid("invalid recovery header or version"));
    }
    let number = |at| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    let path_len = number(8);
    let text_len = number(16);
    if path_len
        .checked_add(text_len)
        .and_then(|size| size.checked_add(32))
        != Some(bytes.len() as u64)
        || checksum(&bytes[..bytes.len() - 8]) != number(bytes.len() - 8)
    {
        return Err(invalid("truncated or corrupt recovery snapshot"));
    }
    let split = 24 + path_len as usize;
    let source = std::str::from_utf8(&bytes[24..split])
        .map_err(|_| invalid("invalid recovery source"))?
        .to_owned();
    let text = std::str::from_utf8(&bytes[split..bytes.len() - 8])
        .map_err(|_| invalid("invalid recovery text"))?;
    let buffer = TextBuffer::from_text(text).map_err(invalid)?;
    Ok(Snapshot { source, buffer })
}

/// List candidates, including corrupt/incomplete files for explicit inspection.
/// No file is removed or automatically considered stale (other sessions may live).
pub fn list(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(PREFIX) {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}
