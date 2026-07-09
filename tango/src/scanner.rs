/// Change-detection fingerprint of the filesystem a scan reads:
/// every file's path, size, and mtime under the given roots (a root
/// may be a file). Gathering it stats files but reads none of them,
/// so it's far cheaper than the scan it gates — an unchanged
/// fingerprint means rescanning would reproduce the current items.
#[derive(PartialEq, Eq)]
pub struct Fingerprint(Vec<(std::path::PathBuf, u64, Option<std::time::SystemTime>)>);

impl Fingerprint {
    pub fn new(roots: &[std::path::PathBuf]) -> Self {
        let mut files = vec![];
        for root in roots {
            for entry in walkdir::WalkDir::new(root) {
                let Ok(entry) = entry else {
                    continue;
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                let (len, mtime) = entry.metadata().map(|m| (m.len(), m.modified().ok())).unwrap_or((0, None));
                files.push((entry.into_path(), len, mtime));
            }
        }
        // Don't trust directory iteration order to be stable
        // between walks.
        files.sort_unstable();
        Fingerprint(files)
    }
}

struct Inner<T> {
    items: T,
    scanning: bool,
    /// Fingerprint the current `items` were scanned from, if they
    /// came through [`Scanner::rescan_if_changed`].
    fingerprint: Option<Fingerprint>,
}

pub struct Scanner<T> {
    inner: std::sync::Arc<std::sync::RwLock<Inner<T>>>,
}

impl<T> Clone for Scanner<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Scanner<T>
where
    T: Default,
{
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::RwLock::new(Inner {
                items: T::default(),
                scanning: false,
                fingerprint: None,
            })),
        }
    }

    pub fn read(&self) -> ScannerReadGuard<'_, T> {
        ScannerReadGuard {
            guard: self.inner.read().unwrap(),
        }
    }

    pub fn rescan(&self, scan: impl Fn() -> Option<T>) {
        {
            let mut inner = self.inner.write().unwrap();
            if inner.scanning {
                return;
            }
            inner.scanning = true;
        }

        let items = scan();

        let mut inner = self.inner.write().unwrap();
        if let Some(items) = items {
            inner.items = items;
        }
        inner.scanning = false;
    }

    /// Like [`Self::rescan`], but skipped entirely when nothing
    /// under `roots` changed since the last completed scan (by stat
    /// fingerprint). This is what the automatic tab-entry rescan
    /// goes through, so switching tabs with nothing new on disk
    /// costs a metadata walk instead of re-reading and re-parsing
    /// every file.
    pub fn rescan_if_changed(&self, roots: &[std::path::PathBuf], scan: impl Fn() -> Option<T>) {
        let fingerprint = Fingerprint::new(roots);
        {
            let mut inner = self.inner.write().unwrap();
            if inner.scanning || inner.fingerprint.as_ref() == Some(&fingerprint) {
                return;
            }
            inner.scanning = true;
        }

        let items = scan();

        let mut inner = self.inner.write().unwrap();
        if let Some(items) = items {
            inner.items = items;
            // The fingerprint predates the scan, so a file changing
            // mid-scan errs toward one extra rescan, never a miss.
            inner.fingerprint = Some(fingerprint);
        }
        inner.scanning = false;
    }
}

/// Read guard returned by [`Scanner::read`] that derefs straight to the
/// scanned items, hiding the wrapping `Inner` from callers.
pub struct ScannerReadGuard<'a, T> {
    guard: std::sync::RwLockReadGuard<'a, Inner<T>>,
}

impl<T> std::ops::Deref for ScannerReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.guard.items
    }
}
