//! Quitting on macOS. The app menu that winit installs owns ⌘Q, and its
//! Quit calls `terminate:`, which exits the process at once: the key never
//! reaches the window, so `App::quit` never ran — an active PvP session
//! skipped its bounded `Goodbye` send and a queued config write was lost.
//! The Dock's Quit and logging out take the same path. AppKit asks the app
//! delegate first (`applicationShouldTerminate:`), so this gives winit's
//! delegate that method: it cancels, and closes the window instead, which
//! arrives as `CloseRequested` and quits from there (`iced::exit` stops the
//! run loop with `stop:`, not `terminate:`, so it doesn't come back here).

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{class, msg_send, sel};

/// NSApplicationTerminateReply
const TERMINATE_CANCEL: usize = 0;
const TERMINATE_NOW: usize = 1;

/// Hook quitting. Call after the event loop exists (winit sets the
/// delegate then): in the app's boot.
pub fn install() {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let delegate: *mut AnyObject = msg_send![app, delegate];
        if delegate.is_null() {
            return;
        }
        let cls: *const AnyClass = (*delegate).class();
        let imp = std::mem::transmute::<
            extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
            unsafe extern "C" fn(),
        >(should_terminate);
        // returns NO if it's already there (a second boot): that's the same method
        objc2::ffi::class_addMethod(
            cls as *mut _,
            sel!(applicationShouldTerminate:).as_ptr(),
            Some(imp),
            c"Q@:@".as_ptr(),
        );
        // AppKit may note which delegate methods exist when the delegate is set
        let _: () = msg_send![app, setDelegate: delegate];
    }
}

/// `applicationShouldTerminate:` — close the window rather than quit.
extern "C" fn should_terminate(_this: *mut AnyObject, _cmd: Sel, app: *mut AnyObject) -> usize {
    unsafe {
        let mut window: *mut AnyObject = msg_send![app, mainWindow];
        if window.is_null() {
            let windows: *mut AnyObject = msg_send![app, windows];
            window = msg_send![windows, firstObject];
        }
        if window.is_null() {
            return TERMINATE_NOW;
        }
        let _: () = msg_send![window, performClose: std::ptr::null_mut::<AnyObject>()];
    }
    TERMINATE_CANCEL
}
