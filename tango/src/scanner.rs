struct Inner<T> {
    items: T,
    scanning: bool,
}

pub struct Scanner<T> {
    inner: std::sync::Arc<parking_lot::RwLock<Inner<T>>>,
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
            inner: std::sync::Arc::new(parking_lot::RwLock::new(Inner {
                items: T::default(),
                scanning: false,
            })),
        }
    }

    pub fn read(&self) -> parking_lot::MappedRwLockReadGuard<'_, T> {
        parking_lot::RwLockReadGuard::map(self.inner.read(), |g| &g.items)
    }

    pub fn is_scanning(&self) -> bool {
        self.inner.read().scanning
    }

    pub fn rescan(&self, scan: impl Fn() -> T) {
        {
            let mut inner = self.inner.write();
            if inner.scanning {
                return;
            }
            inner.scanning = true;
        }

        let items = scan();

        let mut inner = self.inner.write();
        inner.items = items;
        inner.scanning = false;
    }
}
