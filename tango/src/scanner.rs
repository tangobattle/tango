struct Inner<T> {
    items: T,
    scanning: bool,
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
