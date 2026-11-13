//! # Owner state directory
//!
//! The subdirectory local/ of a store, which pimdir reserves for the state
//! of its owner (pimdir STORAGE §3, §6): the format never defines what it
//! holds, a reader and a producer ignore it, the collector never walks it
//! and a backup copies it with the store. neverest keeps there what it
//! owns beside the store: its sidecar, its sync.lock and the mark of the
//! send it is handing over, and no file of its own anywhere else in the
//! store directory.
//!
//! The directory is its user's alone, created `0700`, and every file
//! neverest creates in it `0600` (pimdir STORAGE §18): io-pimdir refuses to
//! open, as its owner, a store whose local/ another user can reach.

use std::{
    fs::{self, File, OpenOptions},
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use io_pimdir::client::modes;

/// The name pimdir reserves for it in a store directory.
const DIR: &str = "local";

/// The local/ of the store at `dir`, whether it exists or not.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(DIR)
}

/// The local/ of the store at `dir`, created `0700` when absent.
///
/// The store directory itself is never created here. A link or a file
/// under the name is refused: nothing is written through one.
pub fn ensure(dir: &Path) -> io::Result<PathBuf> {
    let path = path(dir);

    match modes::create_dir(&path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    if !fs::symlink_metadata(&path)?.is_dir() {
        let reason = format!("{} is not a directory", path.display());
        return Err(io::Error::new(ErrorKind::InvalidInput, reason));
    }

    Ok(path)
}

/// Options opening a file of local/: one they create is `0600` on Unix.
pub fn options() -> OpenOptions {
    modes::options()
}

/// Creates the file `path` of local/ `0600`, emptying one already there.
pub fn create(path: &Path) -> io::Result<File> {
    options().write(true).create(true).truncate(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory and what is created in it are their user's alone,
    /// which io-pimdir asks of local/ before its owner opens the store.
    #[test]
    #[cfg(unix)]
    fn local_and_its_files_are_their_users_alone() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let local = ensure(dir.path()).unwrap();
        assert_eq!(local, dir.path().join("local"));
        assert_eq!(ensure(dir.path()).unwrap(), local, "kept when there");

        let file = local.join("state");
        create(&file).unwrap();

        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&local), 0o700);
        assert_eq!(mode(&file), 0o600);
    }

    /// A file under the name is refused rather than written beside.
    #[test]
    fn a_file_under_the_name_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(path(dir.path()), b"").unwrap();

        let err = ensure(dir.path()).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    /// The store directory is not created on the way.
    #[test]
    fn a_missing_store_directory_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");

        assert!(ensure(&store).is_err());
        assert!(!store.exists());
    }
}
