//! Bounded file snapshots. The VM receives bytes, never filesystem handles.
use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::Path;

pub type FileTree = BTreeMap<String, Vec<u8>>;

#[derive(Clone, Copy)]
pub struct TreeLimits {
    pub bytes: usize,
    /// Includes directories, so empty trees cannot evade the traversal budget.
    pub entries: usize,
    pub depth: usize,
}

struct Builder {
    files: FileTree,
    remaining: TreeLimits,
}

impl Builder {
    fn new(limits: TreeLimits) -> Self {
        Self {
            files: FileTree::new(),
            remaining: limits,
        }
    }

    fn visit(&mut self, path: &str) -> io::Result<()> {
        if self.remaining.entries == 0 || path.len() > 240 || path.split('/').count() > self.remaining.depth {
            return Err(io::Error::other("file tree exceeds entry, path or depth limit"));
        }
        if path.is_empty()
            || path.chars().any(|c| c.is_control() || "\\:".contains(c))
            || path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(io::Error::other("invalid snapshot path"));
        }
        self.remaining.entries -= 1;
        Ok(())
    }

    fn file(&mut self, path: String, reader: impl Read) -> io::Result<()> {
        self.visit(&path)?;
        let mut bytes = Vec::new();
        reader
            .take(self.remaining.bytes.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > self.remaining.bytes {
            return Err(io::Error::other("file tree exceeds byte limit"));
        }
        self.remaining.bytes -= bytes.len();
        if self.files.insert(path, bytes).is_some() {
            return Err(io::Error::other("duplicate snapshot path"));
        }
        Ok(())
    }
}

/// Snapshot a virtual store whose paths are keys, without resolving any OS
/// paths. Shared by browser storage and in-memory adapters.
pub fn snapshot_files<'a>(
    root: &Path,
    files: impl IntoIterator<Item = (&'a Path, &'a [u8])>,
    limits: TreeLimits,
) -> io::Result<FileTree> {
    let mut tree = Builder::new(limits);
    for (path, bytes) in files {
        let Ok(path) = path.strip_prefix(root) else {
            continue;
        };
        let mut components = Vec::new();
        for component in path.components() {
            let std::path::Component::Normal(name) = component else {
                return Err(io::Error::other("invalid snapshot path"));
            };
            components.push(
                name.to_str()
                    .ok_or_else(|| io::Error::other("snapshot names must be UTF-8"))?,
            );
        }
        tree.file(components.join("/"), bytes)?;
    }
    Ok(tree.files)
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
pub(super) fn native(root: &Path, limits: TreeLimits) -> io::Result<FileTree> {
    use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
    use cap_std::fs::{Dir, OpenOptions};

    fn walk(dir: &Dir, prefix: &str, tree: &mut Builder) -> io::Result<()> {
        for entry in dir.entries()? {
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("snapshot names must be UTF-8"))?;
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let kind = entry.file_type()?;
            if kind.is_dir() {
                tree.visit(&path)?;
                // Each open is one component relative to an already-open
                // directory. Never reopen a concatenated ambient pathname.
                walk(&dir.open_dir_nofollow(&name)?, &path, tree)?;
            } else if kind.is_file() {
                let mut options = OpenOptions::new();
                options.read(true).follow(FollowSymlinks::No).nonblock(true);
                let file = dir.open_with(&name, &options)?;
                // Recheck the opened object: it may have changed since listing.
                if !file.metadata()?.is_file() {
                    return Err(io::Error::other("snapshot entry is not a regular file"));
                }
                tree.file(path, file)?;
            } else {
                return Err(io::Error::other(
                    "file snapshots cannot contain symlinks or special files",
                ));
            }
        }
        Ok(())
    }

    // This is the sole ambient access: the host-selected package root. Every
    // descendant access is relative to this capability, with symlinks disabled.
    let dir = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
    let mut tree = Builder::new(limits);
    walk(&dir, "", &mut tree)?;
    Ok(tree.files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> TreeLimits {
        TreeLimits {
            bytes: 1024,
            entries: 32,
            depth: 8,
        }
    }

    #[test]
    fn virtual_snapshot_bounds_and_path_isolation() {
        let files = [
            (Path::new("root/a.txt"), b"inside".as_slice()),
            (Path::new("root/sub/素材 file.txt"), b"nested".as_slice()),
            (Path::new("root-sibling/secret"), b"outside".as_slice()),
        ];
        let snapshot = snapshot_files(Path::new("root"), files, limits()).unwrap();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot["sub/素材 file.txt"], b"nested");
        assert!(snapshot_files(Path::new("root"), files, TreeLimits { bytes: 5, ..limits() }).is_err());
        assert!(snapshot_files(Path::new("root"), files, TreeLimits { entries: 1, ..limits() }).is_err());
        assert!(snapshot_files(Path::new("root"), files, TreeLimits { depth: 1, ..limits() }).is_err());
        assert!(snapshot_files(
            Path::new("root"),
            [(Path::new("root/../escape"), b"x".as_slice())],
            limits()
        )
        .is_err());
    }

    #[cfg(all(feature = "native", not(target_arch = "wasm32")))]
    #[test]
    fn native_snapshot_reads_undeclared_files_and_retains_captured_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let file = dir.path().join("sub/素材 file.txt");
        std::fs::write(&file, b"original").unwrap();
        let snapshot = native(dir.path(), limits()).unwrap();
        std::fs::write(&file, b"modified").unwrap();
        assert_eq!(snapshot["sub/素材 file.txt"], b"original");
        assert_eq!(native(dir.path(), limits()).unwrap()["sub/素材 file.txt"], b"modified");
        assert!(native(dir.path(), TreeLimits { bytes: 3, ..limits() }).is_err());
        assert!(native(dir.path(), TreeLimits { entries: 1, ..limits() }).is_err());
        assert!(native(dir.path(), TreeLimits { depth: 1, ..limits() }).is_err());
    }

    #[cfg(all(feature = "native", unix))]
    #[test]
    fn native_snapshot_rejects_file_directory_and_dangling_symlinks() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"outside").unwrap();
        for target in [
            outside.path().to_owned(),
            outside.path().join("secret"),
            outside.path().join("missing"),
        ] {
            let package = tempfile::tempdir().unwrap();
            symlink(target, package.path().join("linked")).unwrap();
            assert!(native(package.path(), limits()).is_err());
        }
        // Even links pointing back inside are rejected consistently with ZIP.
        let package = tempfile::tempdir().unwrap();
        std::fs::write(package.path().join("inside"), b"inside").unwrap();
        symlink("inside", package.path().join("linked")).unwrap();
        assert!(native(package.path(), limits()).is_err());
    }

    #[cfg(all(feature = "native", unix))]
    #[test]
    fn concurrent_symlink_replacement_never_reads_outside_the_root() {
        use std::os::unix::fs::symlink;
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"outside").unwrap();
        let package = tempfile::tempdir().unwrap();
        let path = package.path().join("entry");
        std::fs::write(&path, b"inside").unwrap();
        std::thread::scope(|scope| {
            let swapping = scope.spawn(|| {
                for i in 0..300 {
                    let replacement = package.path().join("replacement");
                    if i % 2 == 0 {
                        symlink(outside.path().join("secret"), &replacement).unwrap();
                    } else {
                        std::fs::write(&replacement, b"inside").unwrap();
                    }
                    std::fs::rename(replacement, &path).unwrap();
                }
            });
            for _ in 0..300 {
                if let Ok(snapshot) = native(package.path(), limits()) {
                    assert!(snapshot.values().all(|bytes| bytes == b"inside"));
                }
            }
            swapping.join().unwrap();
        });
    }
}
