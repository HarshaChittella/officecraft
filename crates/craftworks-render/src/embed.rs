//! Input for an engine drawn inside an area of another egui window (the Phase 1 harness):
//! the window's input, moved into the engine's own coordinates.

use egui::{Event, Pos2, RawInput, Rect, Vec2, ViewportId, ViewportInfo};

/// The engine's input for one frame, from the window's `parent` input. `area` is where the
/// engine is drawn, in the window's points; `pixels_per_point` the window's scale. Events at a
/// point move by `-area.min`; a press outside the area is the window's own (its tab strip) and
/// doesn't reach the engine; a pointer leaving the area is `PointerGone` to the engine.
/// `keep_key` decides which key events the engine gets (the window keeps its own shortcuts).
pub fn input_for_area(parent: &RawInput, area: Rect, pixels_per_point: f32, keep_key: impl Fn(&Event) -> bool) -> RawInput {
    let offset = area.min.to_vec2();
    let local = Rect::from_min_size(Pos2::ZERO, area.size());
    let inside = |p: Pos2| area.contains(p);
    let mut events = Vec::with_capacity(parent.events.len());
    let mut was_inside: Option<bool> = None;
    for e in &parent.events {
        match e {
            Event::PointerMoved(p) => {
                if inside(*p) {
                    events.push(Event::PointerMoved(*p - offset));
                    was_inside = Some(true);
                } else if was_inside != Some(false) {
                    events.push(Event::PointerGone);
                    was_inside = Some(false);
                }
            }
            Event::PointerButton { pos, button, pressed, modifiers } => {
                // A press starts in the engine only inside it; a release always reaches it, so a
                // drag that leaves the area still ends.
                if !*pressed || inside(*pos) {
                    events.push(Event::PointerButton { pos: *pos - offset, button: *button, pressed: *pressed, modifiers: *modifiers });
                }
            }
            Event::Touch { device_id, id, phase, pos, force } => {
                events.push(Event::Touch { device_id: *device_id, id: *id, phase: *phase, pos: *pos - offset, force: *force });
            }
            Event::MiddleClickPaste { pos, .. } if !inside(*pos) => {}
            Event::MiddleClickPaste { pos, text } => events.push(Event::MiddleClickPaste { pos: *pos - offset, text: text.clone() }),
            Event::Key { .. } | Event::Text(_) | Event::Copy | Event::Cut | Event::Paste(_) => {
                if keep_key(e) {
                    events.push(e.clone());
                }
            }
            other => events.push(other.clone()),
        }
    }
    let focused = parent.focused;
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(
        ViewportId::ROOT,
        ViewportInfo {
            native_pixels_per_point: Some(pixels_per_point),
            focused: Some(focused),
            inner_rect: Some(local),
            outer_rect: Some(local),
            ..Default::default()
        },
    );
    RawInput {
        viewport_id: ViewportId::ROOT,
        viewports,
        screen_rect: Some(local),
        max_texture_side: parent.max_texture_side,
        predicted_dt: parent.predicted_dt,
        events,
        hovered_files: parent.hovered_files.clone(),
        dropped_files: parent.dropped_files.clone(),
        focused,
        system_theme: parent.system_theme,
        ..Default::default()
    }
}

/// `r` moved from the engine's coordinates into the window's.
pub fn to_window(r: Rect, area: Rect) -> Rect {
    r.translate(area.min.to_vec2())
}

/// A size in points as whole physical pixels (at least 1).
pub fn pixels(size: Vec2, pixels_per_point: f32) -> [u32; 2] {
    let px = |v: f32| {
        let p = (v * pixels_per_point).round();
        // Saturating float → int: NaN and negatives become 1, huge sizes the u32 maximum.
        if p.is_finite() && p >= 1.0 { p.min(u32::MAX as f32) as u32 } else { 1 }
    };
    [px(size.x), px(size.y)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Modifiers, PointerButton, pos2, vec2};

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 40.0), vec2(800.0, 600.0))
    }

    fn parent(events: Vec<Event>) -> RawInput {
        RawInput { events, focused: true, ..Default::default() }
    }

    fn all(_: &Event) -> bool {
        true
    }

    #[test]
    fn pointer_events_move_into_the_engine_coordinates() {
        let p = parent(vec![
            Event::PointerMoved(pos2(10.0, 50.0)),
            Event::PointerButton { pos: pos2(10.0, 50.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        ]);
        let i = input_for_area(&p, area(), 2.0, all);
        assert_eq!(i.events[0], Event::PointerMoved(pos2(10.0, 10.0)));
        assert!(matches!(i.events[1], Event::PointerButton { pos, pressed: true, .. } if pos == pos2(10.0, 10.0)));
        assert_eq!(i.screen_rect, Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))));
        assert_eq!(i.viewports.get(&ViewportId::ROOT).and_then(|v| v.native_pixels_per_point), Some(2.0));
    }

    #[test]
    fn a_press_on_the_tab_strip_stays_with_the_window_but_a_release_reaches_the_engine() {
        let p = parent(vec![
            Event::PointerButton { pos: pos2(10.0, 5.0), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::PointerButton { pos: pos2(10.0, 5.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ]);
        let i = input_for_area(&p, area(), 1.0, all);
        assert_eq!(i.events.len(), 1);
        assert!(matches!(i.events[0], Event::PointerButton { pressed: false, .. }));
    }

    #[test]
    fn leaving_the_area_is_pointer_gone_once() {
        let p = parent(vec![Event::PointerMoved(pos2(10.0, 50.0)), Event::PointerMoved(pos2(10.0, 5.0)), Event::PointerMoved(pos2(11.0, 5.0))]);
        let i = input_for_area(&p, area(), 1.0, all);
        assert_eq!(i.events, vec![Event::PointerMoved(pos2(10.0, 10.0)), Event::PointerGone]);
    }

    #[test]
    fn the_window_keeps_the_keys_it_asks_for() {
        let p = parent(vec![Event::Text("a".into()), Event::Copy]);
        let i = input_for_area(&p, area(), 1.0, |e| !matches!(e, Event::Copy));
        assert_eq!(i.events, vec![Event::Text("a".into())]);
    }

    #[test]
    fn pixel_sizes_round_and_never_reach_zero() {
        assert_eq!(pixels(vec2(800.0, 600.5), 2.0), [1600, 1201]);
        assert_eq!(pixels(vec2(0.0, -3.0), 2.0), [1, 1]);
        assert_eq!(pixels(vec2(f32::NAN, f32::INFINITY), 2.0), [1, 1]);
    }
}
