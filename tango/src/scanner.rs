struct Inner<T> {
    pub items: T,
    last_rescan_time: std::time::Instant,
}

#[derive(Clone)]
pub struct Scanner<T> {
    inner: std::sync::Arc<parking_lot::RwLock<Inner<T>>>,
}

impl<T> Scanner<T>
where
    T: Default,
{
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::RwLock::new(Inner {
                items: T::default(),
                last_rescan_time: std::time::Instant::now(),
            })),
        }
    }

    pub fn read(&self) -> parking_lot::MappedRwLockReadGuard<'_, T> {
        parking_lot::RwLockReadGuard::map(self.inner.read(), |g| &g.items)
    }

    pub fn rescan(&self, scan: impl Fn() -> T) {
        if self.inner.is_locked_exclusive() {
            return;
        }

        let items = scan();
        let last_rescan_time = std::time::Instant::now();

        let mut inner = self.inner.write();
        if inner.last_rescan_time > last_rescan_time {
            return;
        }

        inner.items = items;
        inner.last_rescan_time = last_rescan_time;
    }
}
