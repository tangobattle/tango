/// Holds the emulator (mgba CPU) thread inside the async runtime for its whole
/// lifetime. Stored in a thread-local so the
/// [`EnterGuard`](tokio::runtime::EnterGuard) is *dropped* — exiting the runtime
/// context — when the thread ends, instead of leaked.
struct EnteredRuntime {
    // Field order is drop order: `_guard` drops before `_handle`, so the guard
    // (its borrow extended to 'static below) never outlives the handle it
    // entered.
    _guard: tokio::runtime::EnterGuard<'static>,
    _handle: tokio::runtime::Handle,
}

thread_local! {
    static ENTERED_RUNTIME: std::cell::RefCell<Option<EnteredRuntime>> =
        const { std::cell::RefCell::new(None) };
}

/// Returns a closure to run once on the emulator (mgba CPU) thread at start —
/// via [`mgba::thread::Thread::set_start_callback`] — that enters the current
/// tokio runtime on that thread, so [`block_on`] and `Handle::current` resolve
/// from the per-game primary traps that run there. The runtime is exited — the
/// guard dropped — when the thread ends and its thread-local is torn down.
///
/// Capture happens here (on a thread that *is* in the runtime); the returned
/// closure runs later on the emulator thread.
pub fn enter_runtime_on_emulator_thread() -> impl FnOnce() + Send + 'static {
    let handle = tokio::runtime::Handle::current();
    move || {
        // SAFETY: extend the EnterGuard's borrow of `handle` to 'static so it
        // can live in the thread-local. `EnteredRuntime` keeps the owning
        // `handle` next to the guard and drops the guard first (field order),
        // so the guard never outlives the handle; the struct then sits in the
        // thread-local until the thread exits and drops it.
        let guard: tokio::runtime::EnterGuard<'static> =
            unsafe { std::mem::transmute(handle.enter()) };
        ENTERED_RUNTIME.with(|cell| {
            *cell.borrow_mut() = Some(EnteredRuntime {
                _guard: guard,
                _handle: handle,
            });
        });
    }
}

/// Drive `future` to completion, blocking the current thread. The per-game
/// primary traps call this from the emulator thread, which
/// [`enter_runtime_on_emulator_thread`] has entered into the runtime.
pub fn block_on<F>(future: F) -> F::Output
where
    F: std::future::Future,
{
    tokio::runtime::Handle::current().block_on(future)
}
