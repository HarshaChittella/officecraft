//! Telling macOS the app is doing work the user asked for, so App Nap doesn't throttle its
//! timers while the window is in the background. Used for `--check` runs, which measure and
//! wait in real time.

/// Held while the work lasts; dropping it ends the activity.
pub struct Activity {
    #[cfg(target_os = "macos")]
    _token: objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
}

/// Starts a user-initiated, latency-critical activity named `reason`.
pub fn begin(reason: &str) -> Activity {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let options = NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical;
        let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(options, &NSString::from_str(reason));
        Activity { _token: token }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = reason;
        Activity {}
    }
}
