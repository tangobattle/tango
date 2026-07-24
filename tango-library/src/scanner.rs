//! The shared rescan machinery every content scan sits behind.
//!
//! A [`Scanner`] owns the last scan's items behind an `RwLock`, so the
//! frontend reads them synchronously on every render while a rescan runs
//! elsewhere.
//!
//! Everything here is synchronous. The one asynchronous step in a
//! rescan — enumerating the directories — happens before any of this and
//! arrives as a [`Listing`], which then serves both as the
//! change-detection fingerprint and as the scan's input.

use crate::storage::Listing;

struct Inner<T> {
    items: T,
    scanning: bool,
    /// The listing the current `items` were scanned from, if they came
    /// through [`Scanner::rescan_if_changed`].
    listing: Option<Listing>,
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
                listing: None,
            })),
        }
    }

    pub fn read(&self) -> ScannerReadGuard<'_, T> {
        ScannerReadGuard {
            guard: self.inner.read().unwrap(),
        }
    }

    /// Run `scan` and adopt what it returns. A scan already in flight
    /// wins: this returns without running.
    pub fn rescan(&self, scan: impl FnOnce() -> Option<T>) {
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

    /// Like [`Self::rescan`], but skipped entirely when `listing` is
    /// identical to the one the current items were scanned from. This is
    /// what the automatic tab-entry rescan goes through, so switching
    /// tabs with nothing new on disk costs a metadata walk instead of
    /// re-reading and re-parsing every file.
    pub fn rescan_if_changed(&self, listing: &Listing, scan: impl FnOnce() -> Option<T>) {
        {
            let mut inner = self.inner.write().unwrap();
            if inner.scanning || inner.listing.as_ref() == Some(listing) {
                return;
            }
            inner.scanning = true;
        }

        let items = scan();

        let mut inner = self.inner.write().unwrap();
        if let Some(items) = items {
            inner.items = items;
            // The listing predates the reads, so a file changing
            // mid-scan errs toward one extra rescan, never a miss.
            inner.listing = Some(listing.clone());
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
