// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Writing a file that holds a secret, without a window where it is readable
//! by anyone else and without a crash leaving a half-written one behind.
//!
//! Both properties matter here and neither is the default.
//!
//! `fs::write` creates the file with `0666 & ~umask`, which is `0644` on a
//! normal desktop. Writing the token and then calling `chmod` afterwards leaves
//! it world-readable for the length of the write. The mode has to be set at
//! creation, which means `OpenOptions` rather than `fs::write`.
//!
//! Atomic replacement needs more than `rename`. The rename is atomic against
//! other readers, so nobody ever sees a half-written file, but that alone does
//! not survive a power cut: the metadata operation can reach the disk before
//! the data it points at, leaving a correctly named empty file. Fixing that
//! takes an `fsync` of the file before the rename and an `fsync` of the
//! directory after it.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Owner-only file mode. No group, no other, no execute.
#[cfg(unix)]
const PRIVATE_MODE: u32 = 0o600;

/// Write `contents` to `path`, replacing whatever was there.
///
/// The file is owner-readable only from the moment it exists, and the
/// replacement is atomic and durable. `unique` distinguishes the temporary
/// file from any other write in flight; callers that can race should pass
/// something that differs per writer.
pub fn write_private(path: &Path, contents: &[u8], unique: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    create_dir_private(parent)?;

    let temp = temp_path(path, unique);

    // Scoped so the handle is closed before the rename. Windows will not
    // rename over a file that is still open.
    {
        let mut file = create_private(&temp)?;
        file.write_all(contents)
            .map_err(|source| Error::secret_file(&temp, source))?;
        // Before the rename, not after. This is the half that makes the rename
        // meaningful rather than decorative.
        file.sync_all()
            .map_err(|source| Error::secret_file(&temp, source))?;
    }

    fs::rename(&temp, path).map_err(|source| {
        // Do not leave the temporary file behind on a failed rename. It holds
        // the same secret as the file we were trying to write.
        let _ = fs::remove_file(&temp);
        Error::secret_file(path, source)
    })?;

    sync_dir(parent);
    Ok(())
}

/// Remove a file, treating "it was not there" as success.
pub fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {
            sync_dir(path.parent().unwrap_or_else(|| Path::new(".")));
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::secret_file(path, source)),
    }
}

/// The temporary file a write goes through before being renamed into place.
///
/// Exposed for tests, which need to assert that no temporary file survives a
/// successful write.
pub fn temp_path(path: &Path, unique: &str) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("secret");
    path.with_file_name(format!(".{name}.{unique}.tmp"))
}

/// Create a directory tree that only the owner can enter.
///
/// `0700` rather than the default `0777 & ~umask`. A directory holding session
/// tokens should not be listable by other local users even when the files
/// inside are individually locked down.
pub fn create_dir_private(dir: &Path) -> Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }

    fs::create_dir_all(dir).map_err(|source| Error::secret_file(dir, source))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Only tighten a directory we can already reach. Failing here would
        // turn a cosmetic permissions issue into a failed login.
        if let Ok(metadata) = fs::metadata(dir) {
            let mut permissions = metadata.permissions();
            if permissions.mode() & 0o077 != 0 {
                permissions.set_mode(0o700);
                let _ = fs::set_permissions(dir, permissions);
            }
        }
    }

    Ok(())
}

/// Create a file that is owner-only from the instant it exists.
fn create_private(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_MODE);
    }

    options
        .open(path)
        .map_err(|source| Error::secret_file(path, source))
}

/// Flush the directory entry so the rename survives a power cut.
///
/// Best effort on purpose. Some filesystems reject opening a directory for
/// this, and a session that is written but not yet durable is worth far more
/// than a login that fails because the filesystem is unusual.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = File::open(dir) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn writes_the_contents_it_was_given() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        write_private(&path, b"token", "test").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"token");
    }

    #[test]
    #[cfg(unix)]
    fn the_file_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        write_private(&path, b"token", "test").unwrap();

        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    #[cfg(unix)]
    fn the_file_is_never_group_or_world_readable_even_under_a_permissive_umask() {
        // The bug this guards against is creating the file with the default
        // mode and chmod-ing afterwards. Under umask 0 that would produce 0666
        // for the duration of the write. Setting the umask wide here means a
        // regression shows up as a failure rather than as luck.
        let previous = unsafe { libc_umask(0) };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        write_private(&path, b"token", "test").unwrap();
        let mode = mode_of(&path);

        unsafe { libc_umask(previous) };
        assert_eq!(
            mode & 0o077,
            0,
            "mode was {mode:o}, expected no group or other bits"
        );
    }

    #[cfg(unix)]
    unsafe fn libc_umask(mask: u32) -> u32 {
        unsafe extern "C" {
            fn umask(mask: u32) -> u32;
        }
        unsafe { umask(mask) }
    }

    #[test]
    fn replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        write_private(&path, b"first", "test").unwrap();
        write_private(&path, b"second", "test").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
    }

    #[test]
    #[cfg(unix)]
    fn replacing_a_file_keeps_it_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        write_private(&path, b"first", "test").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"second", "test").unwrap();

        assert_eq!(mode_of(&path), 0o600);
    }

    #[test]
    fn leaves_no_temporary_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");

        write_private(&path, b"token", "test").unwrap();

        assert!(!temp_path(&path, "test").exists());
        let entries: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1, "unexpected leftovers: {entries:?}");
    }

    #[test]
    fn two_writers_with_different_keys_do_not_share_a_temporary_file() {
        let path = Path::new("/tmp/consort/session.json");
        assert_ne!(temp_path(path, "a"), temp_path(path, "b"));
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deeper").join("secret.json");

        write_private(&path, b"token", "test").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"token");
    }

    #[test]
    #[cfg(unix)]
    fn the_parent_directory_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");

        create_dir_private(&nested).unwrap();

        assert_eq!(mode_of(&nested), 0o700);
    }

    #[test]
    fn creating_a_directory_twice_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested");

        create_dir_private(&nested).unwrap();
        create_dir_private(&nested).unwrap();
    }

    #[test]
    fn creating_an_empty_path_is_a_no_op() {
        create_dir_private(Path::new("")).unwrap();
    }

    #[test]
    fn removing_a_missing_file_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        remove_if_present(&dir.path().join("absent.json")).unwrap();
    }

    #[test]
    fn removing_a_present_file_deletes_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        write_private(&path, b"token", "test").unwrap();

        remove_if_present(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn a_write_into_an_unwritable_place_is_an_error_not_a_panic() {
        // /proc is present on every Linux CI runner and rejects creation.
        let path = Path::new("/proc/consort-should-not-exist/secret.json");
        assert!(write_private(path, b"token", "test").is_err());
    }
}
