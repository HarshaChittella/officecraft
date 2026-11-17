//! The system's display accessibility settings the chrome follows (Phase 8.2): Reduce motion
//! and Increase contrast (System Settings ▸ Accessibility ▸ Display). Read through AppKit's safe
//! calls (`NSWorkspace`), so no `unsafe` (D24).

/// What the system asks of the interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayPrefs {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
}

/// The settings as they are now (all off where the system has none).
pub fn display_prefs() -> DisplayPrefs {
    #[cfg(target_os = "macos")]
    {
        let ws = objc2_app_kit::NSWorkspace::sharedWorkspace();
        DisplayPrefs {
            reduce_motion: ws.accessibilityDisplayShouldReduceMotion(),
            increase_contrast: ws.accessibilityDisplayShouldIncreaseContrast(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    DisplayPrefs::default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_settings_can_be_read() {
        // Whatever this Mac says; the call itself must work.
        let _ = super::display_prefs();
    }
}
