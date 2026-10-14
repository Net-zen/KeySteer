//! Real filesystem fixtures shared by discovery and persistence regression tests.

use std::path::{Path, PathBuf};

pub(crate) struct TestDirectory(pub(crate) PathBuf);

impl TestDirectory {
    pub(crate) fn new(label: &str) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "keysteer-{label}-{}-{timestamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn symlink_file(target: &Path, link: &Path) {
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}

pub(crate) fn symlink_dir(target: &Path, link: &Path) {
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}
