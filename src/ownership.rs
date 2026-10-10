use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::error::{Error, Result};

/// Lock files are never unlinked: removing a locked inode permits a second owner.
/// The database thread retains this guard until the connection is closed, even
/// when its async caller is canceled or the shutdown deadline expires.
pub struct StateOwner {
    root: PathBuf,
    _lock: File,
}

impl StateOwner {
    pub fn acquire(path: &Path) -> Result<Self> {
        if path.exists() {
            verify_private(path, true)?;
        } else {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(path).map_err(|_| Error::Io)?;
        }
        verify_private(path, true)?;
        let root = fs::canonicalize(path).map_err(|_| Error::Io)?;
        let lock = private_file(&root.join("owner.lock"))?;
        FileExt::try_lock_exclusive(&lock).map_err(|err| {
            if err.kind() == fs2::lock_contended_error().kind() {
                Error::AlreadyOwned
            } else {
                Error::Io
            }
        })?;
        Ok(Self { root, _lock: lock })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub(crate) fn verify_private(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::Io)?;
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(Error::Io);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Error::Io);
        }
    }
    Ok(())
}

pub(crate) fn private_file(path: &Path) -> Result<File> {
    match fs::symlink_metadata(path) {
        Ok(_) => verify_private(path, false)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err(Error::Io),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| Error::Io)?;
    verify_private(path, false)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_owner_rejected_and_drop_releases_same_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        let owner = StateOwner::acquire(&root).unwrap();
        assert!(matches!(
            StateOwner::acquire(&root),
            Err(Error::AlreadyOwned)
        ));
        assert!(root.join("owner.lock").exists());
        drop(owner);
        assert!(StateOwner::acquire(&root).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_public_state_and_symlinks_without_changing_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(StateOwner::acquire(&root), Err(Error::Io)));
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let linked = dir.path().join("linked");
        symlink(&root, &linked).unwrap();
        assert!(StateOwner::acquire(&linked).is_err());
        symlink(dir.path().join("elsewhere"), root.join("owner.lock")).unwrap();
        assert!(StateOwner::acquire(&root).is_err());
    }
}
