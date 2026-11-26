//! A per-run secret published beside the instance descriptors.
//!
//! A helper process that quantick launches proves itself by reading a file
//! only the current user can read. That file lives in the same private
//! directory as the control-plane descriptor and is written and checked by
//! the same code ([`crate::discovery`]'s private-file helpers), so it has the
//! descriptor's protections: the user owns it, it carries a private mode or
//! ACL, and links are refused. Discovery reads only `*.json`, so the secret
//! file is never taken for a descriptor.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::discovery::{
    DiscoveryError, ensure_explicit_private_directory, private_new_file, remove_owned_descriptor,
    runtime_instances_dir, verify_private_file,
};

/// Extension of every secret file. Never `json`, which discovery reads.
pub const SECRET_FILE_EXTENSION: &str = "secret";

/// A published secret file, removed when dropped.
#[derive(Debug)]
pub struct PublishedSecret {
    path: PathBuf,
}

impl PublishedSecret {
    /// Where the file is, for the helper process to be pointed at.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PublishedSecret {
    fn drop(&mut self) {
        let _ = remove_owned_descriptor(&self.path);
    }
}

/// Publish `contents` as `<stem>.secret` in the private runtime directory.
///
/// # Errors
///
/// The directory cannot be made private, `stem` is not a plain file stem, or
/// the file cannot be written privately.
pub fn publish_secret(stem: &str, contents: &[u8]) -> Result<PublishedSecret, DiscoveryError> {
    publish_secret_in(&runtime_instances_dir()?, stem, contents)
}

/// [`publish_secret`] into an explicit directory.
///
/// # Errors
///
/// As [`publish_secret`].
pub fn publish_secret_in(
    directory: &Path,
    stem: &str,
    contents: &[u8],
) -> Result<PublishedSecret, DiscoveryError> {
    let plain = !stem.is_empty()
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if !plain {
        return Err(DiscoveryError::new(
            "a secret file stem is ASCII letters, digits, '-' and '_' only",
        ));
    }
    ensure_explicit_private_directory(directory)?;
    let target = directory.join(format!("{stem}.{SECRET_FILE_EXTENSION}"));
    // A file left by a crashed run whose process id this run inherited: it
    // is this user's own and private, or removing it refuses.
    remove_owned_descriptor(&target)?;
    // A file another publisher created between the removal and this open
    // fails `create_new` and is left alone; only a file this call created is
    // cleaned up when writing it fails.
    let mut file = private_new_file(&target)?;
    let written = file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|error| DiscoveryError::io("write private secret file", error))
        .and_then(|()| verify_private_file(&target));
    if let Err(error) = written {
        drop(file);
        let _ = std::fs::remove_file(&target);
        return Err(error);
    }
    Ok(PublishedSecret { path: target })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;

    #[test]
    fn the_file_holds_the_contents_and_goes_with_its_owner() {
        let scratch = ScratchDir::new("secret-file");
        let directory = scratch.join("instances");
        let published = publish_secret_in(&directory, "mt5-exec-1", b"abc").expect("publish");
        let path = published.path().to_path_buf();
        assert_eq!(path.extension().unwrap(), SECRET_FILE_EXTENSION);
        assert_eq!(std::fs::read(&path).unwrap(), b"abc");
        verify_private_file(&path).expect("private");
        drop(published);
        assert!(!path.exists(), "removed with its owner");
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_readable_by_its_owner_alone() {
        use std::os::unix::fs::PermissionsExt as _;
        let scratch = ScratchDir::new("secret-mode");
        let directory = scratch.join("instances");
        let published = publish_secret_in(&directory, "mt5-exec-2", b"abc").expect("publish");
        let mode = std::fs::metadata(published.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_leftover_file_is_replaced() {
        let scratch = ScratchDir::new("secret-leftover");
        let directory = scratch.join("instances");
        let first = publish_secret_in(&directory, "mt5-exec-3", b"old").expect("publish");
        let path = first.path().to_path_buf();
        std::mem::forget(first);
        let second = publish_secret_in(&directory, "mt5-exec-3", b"new").expect("republish");
        assert_eq!(second.path(), path);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn a_stem_that_could_name_another_path_is_refused() {
        let scratch = ScratchDir::new("secret-stem");
        let directory = scratch.join("instances");
        for stem in ["", "../x", "a/b", "a.json", "a b"] {
            assert!(
                publish_secret_in(&directory, stem, b"x").is_err(),
                "{stem:?}"
            );
        }
    }

    #[test]
    fn discovery_never_reads_a_secret_file() {
        let scratch = ScratchDir::new("secret-discovery");
        let directory = scratch.join("instances");
        let _published = publish_secret_in(&directory, "mt5-exec-4", b"x").expect("publish");
        let report = crate::discovery::discover_descriptors_in(&directory).expect("discover");
        assert!(report.candidates.is_empty());
        assert!(report.issues.is_empty(), "{:?}", report.issues);
    }
}
