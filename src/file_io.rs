//! File loading and staged saving for the shared editor.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use crate::buffer::TextBuffer;

/// Load UTF-8/LF text, or start an empty buffer for a nonexistent path.
/// Other I/O errors and unsupported text formats are returned to the caller.
pub fn load(path: &Path) -> io::Result<TextBuffer> {
    load_with_origin(path).map(|(buffer, _)| buffer)
}

/// A stable version of bytes read from one handle. Content is shared by editor
/// transaction clones; exact comparison also detects timestamp-preserving edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileVersion {
    text: Arc<str>,
    stamp: Stamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64),
}

impl Stamp {
    fn read(metadata: &fs::Metadata) -> io::Result<Self> {
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (metadata.dev(), metadata.ino())
            },
        })
    }
}

fn conflict() -> io::Error {
    io::Error::other("file changed since loading or saving; reload or write to a new path")
}

pub(crate) fn load_with_origin(path: &Path) -> io::Result<(TextBuffer, Option<FileVersion>)> {
    load_with(path, || Ok(()))
}

// The hook allows deterministic tests of replacement or rewriting during reads.
fn load_with(
    path: &Path,
    after_read: impl FnOnce() -> io::Result<()>,
) -> io::Result<(TextBuffer, Option<FileVersion>)> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((TextBuffer::default(), None));
        }
        Err(error) => return Err(error),
    };
    let before = file.metadata()?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    after_read()?;
    let after = file.metadata()?;
    let named = fs::metadata(path)?;
    if !same_destination(Some(&before), Some(&after))
        || !same_destination(Some(&after), Some(&named))
    {
        return Err(conflict());
    }
    let buffer = TextBuffer::from_text(&text)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
    let version = FileVersion {
        text: text.into(),
        stamp: Stamp::read(&after)?,
    };
    Ok((buffer, Some(version)))
}

/// Low-level staged save without a previously loaded version. Editor saves use
/// save_checked instead, to also protect changes made before saving starts.
pub fn save(path: &Path, buffer: &TextBuffer) -> io::Result<()> {
    save_with(path, |file| file.write_all(buffer.to_text().as_bytes()))
}

/// Export without overwriting an existing destination, including symlinks.
pub fn save_new(path: &Path, buffer: &TextBuffer) -> io::Result<()> {
    save_checked(path, buffer, None).map(|_| ())
}

pub(crate) fn save_checked(
    path: &Path,
    buffer: &TextBuffer,
    expected: Option<&FileVersion>,
) -> io::Result<FileVersion> {
    let text: Arc<str> = buffer.to_text().into();
    let stamp = stage(path, Some(expected), |file| file.write_all(text.as_bytes()))?;
    Ok(FileVersion { text, stamp })
}

fn save_with(path: &Path, write: impl FnOnce(&mut fs::File) -> io::Result<()>) -> io::Result<()> {
    stage(path, None, write).map(|_| ())
}

fn validate_version(path: &Path, expected: &FileVersion) -> io::Result<()> {
    let (_, actual) = load_with_origin(path)?;
    if actual.as_ref() != Some(expected) {
        return Err(conflict());
    }
    Ok(())
}

/// None means a low-level unguarded save; Some(None) requires a new path.
/// No fallible work follows publication. The next version comes from the staged
/// handle before rename (ctime is deliberately not part of the stable stamp).
fn stage(
    path: &Path,
    expected: Option<Option<&FileVersion>>,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<Stamp> {
    let before = destination(path)?;
    match expected {
        Some(Some(version)) => validate_version(path, version)?,
        Some(None) if before.is_some() => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination already exists",
            ));
        }
        _ => {}
    }
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
    let stamp = Stamp::read(&staged.as_file().metadata()?)?;
    if let Some(Some(version)) = expected {
        validate_version(path, version)?;
    }
    let after = destination(path)?;
    if !same_destination(before.as_ref(), after.as_ref()) {
        return Err(io::Error::other("file changed during save"));
    }
    if before.is_some() {
        staged.persist(path).map_err(|error| error.error)?;
    } else {
        staged
            .persist_noclobber(path)
            .map_err(|error| error.error)?;
    }
    Ok(stamp)
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
    fn load_rejects_changes_during_reading() {
        for replace in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("text");
            fs::write(&path, "original").unwrap();
            let result = load_with(&path, || {
                if replace {
                    let replacement = dir.path().join("replacement");
                    fs::write(&replacement, "original")?;
                    fs::rename(replacement, &path)
                } else {
                    fs::write(&path, "changed length")
                }
            });
            assert!(result.unwrap_err().to_string().contains("file changed"));
        }
    }

    #[test]
    fn guarded_save_rechecks_after_staging_and_cleans_up() {
        for delete in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("text");
            fs::write(&path, "original").unwrap();
            let (_, version) = load_with_origin(&path).unwrap();
            let result = stage(&path, Some(version.as_ref()), |file| {
                file.write_all(b"local")?;
                if delete {
                    fs::remove_file(&path)
                } else {
                    fs::write(&path, "external")
                }
            });
            assert!(result.unwrap_err().to_string().contains("file changed"));
            assert_eq!(
                fs::read_dir(dir.path()).unwrap().count(),
                usize::from(!delete)
            );
            if !delete {
                assert_eq!(fs::read_to_string(&path).unwrap(), "external");
            }
        }
    }

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
