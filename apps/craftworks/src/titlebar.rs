//! The title row around the macOS traffic lights. AppKit draws the lights itself and moving
//! them would take `unsafe` AppKit calls (forbidden here, as in the Craft repos), so the row is
//! sized to centre them: its height is twice the lights' centre, and the tabs start the same gap
//! after them. The lights moved and grew in macOS 26, so they are measured, as WordCraft does,
//! through `objc2-app-kit`'s safe bindings.

use craftworks_shell::tokens::size;

/// Where the tab strip goes in the title row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleBar {
    /// Height of the title row, in points.
    pub height: f32,
    /// Where the tabs start, in points from the window's left edge.
    pub content_left: f32,
}

impl Default for TitleBar {
    /// The layout before the window can be measured (and off macOS).
    fn default() -> Self {
        TitleBar { height: size::TITLE_ROW_MIN, content_left: if cfg!(target_os = "macos") { 80.0 } else { 12.0 } }
    }
}

impl TitleBar {
    /// The row for lights whose right edge is `right`, centre `centre_y` below the top of the
    /// window, and diameter `light`.
    pub fn around_lights(right: f32, centre_y: f32, light: f32) -> TitleBar {
        let height = (2.0 * centre_y).max(32.0);
        let gap = ((height - light) / 2.0).max(8.0);
        TitleBar { height, content_left: (right + gap + 4.0).clamp(60.0, 140.0) }
    }

    /// In full screen the lights are hidden: the tabs start at the edge.
    pub fn fullscreen(self) -> TitleBar {
        TitleBar { height: self.height, content_left: 12.0 }
    }
}

/// Measures the app window's traffic lights; `None` before the window exists.
#[cfg(target_os = "macos")]
pub fn measure() -> Option<TitleBar> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWindowButton};
    let mtm = MainThreadMarker::new()?;
    let window = NSApplication::sharedApplication(mtm).windows().firstObject()?;
    let zoom = window.standardWindowButton(NSWindowButton::ZoomButton)?;
    // In window coordinates, which count up from the bottom.
    let frame = zoom.convertRect_toView(zoom.bounds(), None);
    let window_height = window.frame().size.height;
    let right = frame.origin.x + frame.size.width;
    let centre_from_top = window_height - (frame.origin.y + frame.size.height / 2.0);
    if !(right.is_finite() && centre_from_top.is_finite() && right > 0.0 && centre_from_top > 0.0) {
        return None;
    }
    Some(TitleBar::around_lights(right as f32, centre_from_top as f32, frame.size.height as f32))
}

#[cfg(not(target_os = "macos"))]
pub fn measure() -> Option<TitleBar> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_centres_the_lights() {
        let t = TitleBar::around_lights(68.0, 18.0, 14.0);
        assert_eq!(t.height, 36.0);
        assert!(t.content_left > 68.0);
        assert_eq!(TitleBar::around_lights(68.0, 10.0, 12.0).height, 32.0, "never shorter than the tabs need");
        assert_eq!(TitleBar::around_lights(1e9, 18.0, 14.0).content_left, 140.0, "clamped");
    }
}
