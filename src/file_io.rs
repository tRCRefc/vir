//! File loading and staged saving for the shared editor.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::buffer::TextBuffer;

/// Load UTF-8/LF text, or start an empty buffer for a nonexistent path.
/// Other I/O errors and unsupported text formats are returned to the caller.
pub fn load(path: &Path) -> io::Result<TextBuffer> {
    load_with_origin(path).map(|(buffer, _)| buffer)
}

/// Report whether bytes were loaded, so a new buffer cannot overwrite a file
/// created by another process between opening and its first save.
pub(crate) fn load_with_origin(path: &Path) -> io::Result<(TextBuffer, bool)> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((TextBuffer::default(), false));
        }
        Err(error) => return Err(error),
    };
    TextBuffer::from_text(&text)
        .map(|buffer| (buffer, true))
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))
}

pub fn save(path: &Path, buffer: &TextBuffer) -> io::Result<()> {
    save_with(path, |file| file.write_all(buffer.to_text().as_bytes()))
}

/// Export without overwriting an existing destination, including symlinks.
pub fn save_new(path: &Path, buffer: &TextBuffer) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(".vir-save-")
        .tempfile_in(parent)?;
    staged.write_all(buffer.to_text().as_bytes())?;
    staged.as_file().sync_all()?;
    staged
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    Ok(())
}

/// Stage all bytes in the destination directory before replacing a regular
/// file. No fallible work follows publication: Ok means the new file is visible.
/// File contents are synced, but directory crash durability is not guaranteed.
fn save_with(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    let before = destination(path)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staged = tempfile::Builder::new()
        .prefix(".vir-save-")
        .tempfile_in(parent)?;
    if let Some(metadata) = &before {
        // Renaming must not bypass the existing file's write permissions.
        fs::OpenOptions::new().write(true).open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let staged_metadata = staged.as_file().metadata()?;
            if metadata.uid() != staged_metadata.uid() || metadata.gid() != staged_metadata.gid() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "cannot preserve file ownership",
                ));
            }
        }
    }
    write(staged.as_file_mut())?;
    if let Some(metadata) = &before {
        staged.as_file().set_permissions(metadata.permissions())?;
    }
    staged.as_file().sync_all()?;
    let after = destination(path)?;
    if !same_destination(before.as_ref(), after.as_ref()) {
        return Err(io::Error::other("file changed during save"));
    }
    if before.is_some() {
        staged.persist(path).map_err(|error| error.error)?;
    } else {
        // Never overwrite a file created by someone else while we were writing.
        staged
            .persist_noclobber(path)
            .map_err(|error| error.error)?;
    }
    Ok(())
}

fn destination(path: &Path) -> io::Result<Option<fs::Metadata>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "save requires a regular file (symbolic links are not supported)",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "saving files with multiple hard links is not supported",
            ));
        }
        if metadata.mode() & 0o7000 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "saving files with special permission bits is not supported",
            ));
        }
    }
    if metadata.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is read only",
        ));
    }
    Ok(Some(metadata))
}

fn same_destination(before: Option<&fs::Metadata>, after: Option<&fs::Metadata>) -> bool {
    match (before, after) {
        (None, None) => true,
        (Some(before), Some(after)) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if (
                    before.dev(),
                    before.ino(),
                    before.ctime(),
                    before.ctime_nsec(),
                    before.mode(),
                    before.uid(),
                    before.gid(),
                ) != (
                    after.dev(),
                    after.ino(),
                    after.ctime(),
                    after.ctime_nsec(),
                    after.mode(),
                    after.uid(),
                    after.gid(),
                ) {
                    return false;
                }
            }
            before.len() == after.len() && before.modified().ok() == after.modified().ok()
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_write_failure_preserves_original_and_removes_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original");
        fs::write(&path, "original\n").unwrap();
        let error = save_with(&path, |file| {
            file.write_all(b"partial replacement")?;
            Err(io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "injected write failure");
        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_new_file_write_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new");
        assert!(
            save_with(&path, |file| {
                file.write_all(b"partial")?;
                Err(io::Error::other("injected write failure"))
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn concurrent_destination_change_is_not_overwritten() {
        for existing in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("target");
            if existing {
                fs::write(&path, "old").unwrap();
            }
            assert!(
                save_with(&path, |file| {
                    file.write_all(b"editor text")?;
                    fs::write(&path, "external change")
                })
                .is_err()
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), "external change");
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}
