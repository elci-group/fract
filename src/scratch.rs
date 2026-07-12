//! `tempfile`-free scratch directories: creates `{prefix}-{pid}-{counter}`
//! under the system temp folder. Cleanup is the caller's responsibility.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Creates a new temporary directory under the system temp folder.
///
/// The directory name combines the supplied prefix, the current process id,
/// and a per-process counter so repeated calls within the same process do
/// not collide. The caller is responsible for cleaning up the returned path.
///
/// # Errors
/// Returns an error if the directory cannot be created.
pub fn temp_dir(prefix: &str) -> io::Result<PathBuf> {
    let base = std::env::temp_dir();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = format!("{}-{}-{}", prefix, std::process::id(), n);
    let path = base.join(name);
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_temp_directory() {
        let path = temp_dir("fract_test").unwrap();
        assert!(path.exists());
        assert!(path.is_dir());
        std::fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn directories_are_unique() {
        let a = temp_dir("fract_test").unwrap();
        let b = temp_dir("fract_test").unwrap();
        assert_ne!(a, b);
        assert!(a.exists());
        assert!(b.exists());
        std::fs::remove_dir_all(&a).unwrap();
        std::fs::remove_dir_all(&b).unwrap();
    }

    #[test]
    fn directory_has_expected_prefix() {
        let path = temp_dir("fract_prefix").unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("fract_prefix-"));
        std::fs::remove_dir_all(&path).unwrap();
    }
}
