//! The chrome's shared pieces (docs/design.md "Controls"): the floating panel, buttons, the
//! search field, keycaps, hover fills and the focus ring. Every surface draws with these, so
//! a hover, a press or a focus looks and moves the same wherever it happens.

use crate::icons;
use crate::tokens::{Palette, SEMIBOLD, motion, radius, size, space, text};
use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Id, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

/// The frame of anything that floats over the window: popovers, the start panel, toasts.
pub fn panel_frame(p: &Palette) -> egui::Frame {
    egui::Frame::NONE
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(radius::POPOVER))
        .shadow(egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: p.shadow })
        .inner_margin(space::L)
}

/// A soft drop shadow under `rect`, `lift` (0–1) strong: for things that rise on hover.
pub fn shadow(painter: &egui::Painter, rect: Rect, corner: u8, p: &Palette, lift: f32) {
    if lift <= 0.01 {
        return;
    }
    let color = Palette::tint(Color32::BLACK, lift * if p.dark { 0.38 } else { 0.11 });
    let y = (2.0 + 4.0 * lift).round() as i8;
    let blur = (6.0 + 12.0 * lift).round() as u8;
    painter.add(egui::Shadow { offset: [0, y], blur, spread: 0, color }.as_shape(rect, CornerRadius::same(corner)));
}

/// The keyboard focus ring around `rect`: a 2 pt accent outline, 2 pt away.
pub fn focus_ring(painter: &egui::Painter, rect: Rect, corner: u8, p: &Palette) {
    painter.rect_stroke(rect.expand(2.0), CornerRadius::same(corner.saturating_add(2)), Stroke::new(2.0, p.accent), egui::StrokeKind::Outside);
}

/// How a hover-able thing looks right now: 0–1 hovered (eased), and whether it is pressed.
pub fn hover(ui: &Ui, resp: &Response) -> (f32, bool) {
    (motion::fade(ui.ctx(), resp.id.with("cw_hover"), resp.hovered() || resp.has_focus(), motion::HOVER), resp.is_pointer_button_down_on())
}

/// Paints the standard hover fill behind a row or an icon button and returns the 0–1 hover.
pub fn hover_fill(ui: &Ui, painter: &egui::Painter, p: &Palette, rect: Rect, corner: u8, resp: &Response) -> f32 {
    let (h, down) = hover(ui, resp);
    if h > 0.0 {
        let fill = if down { Palette::mix(p.surface_hover, p.text, 0.07) } else { p.surface_hover };
        painter.rect_filled(rect, CornerRadius::same(corner), Palette::tint(fill, h));
    }
    h
}

/// A button's weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// The one thing to do: filled with the accent.
    Primary,
    /// Anything else: an outline.
    Secondary,
}

fn button_font() -> FontId {
    FontId::new(text::body().size - 0.5, FontFamily::Name(SEMIBOLD.into()))
}

/// The width [`button`] takes for `label`.
pub fn button_width(ui: &Ui, label: &str) -> f32 {
    ui.painter().layout_no_wrap(label.to_string(), button_font(), Color32::WHITE).size().x + 2.0 * space::M
}

/// A button, laid out by `ui`.
pub fn button(ui: &mut Ui, p: &Palette, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    let w = button_width(ui, label);
    let (rect, _) = ui.allocate_exact_size(vec2(w, size::CONTROL_H), Sense::hover());
    button_at(ui, p, rect, label, kind, enabled)
}

/// A button in `rect`.
pub fn button_at(ui: &mut Ui, p: &Palette, rect: Rect, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    let resp = ui.interact(rect, ui.scope_id().with(("cw_button", label, rect.min.y.to_bits())), if enabled { Sense::click() } else { Sense::hover() });
    let (h, down) = if enabled { hover(ui, &resp) } else { (0.0, false) };
    let corner = CornerRadius::same(radius::CONTROL);
    let painter = ui.painter();
    let ink = match kind {
        ButtonKind::Primary => {
            let base = if down { Palette::mix(p.accent, Color32::BLACK, 0.14) } else { Palette::mix(p.accent, if p.dark { Color32::WHITE } else { Color32::BLACK }, 0.08 * h) };
            painter.rect_filled(rect, corner, base);
            p.accent_ink
        }
        ButtonKind::Secondary => {
            let fill = if down { Palette::mix(p.surface_hover, p.text, 0.07) } else { Palette::tint(p.surface_hover, h) };
            painter.rect_filled(rect, corner, fill);
            painter.rect_stroke(rect, corner, Stroke::new(1.0, p.border), egui::StrokeKind::Inside);
            p.text
        }
    };
    let ink = if enabled { ink } else { Palette::mix(ink, if kind == ButtonKind::Primary { p.accent } else { p.surface_raised }, 0.55) };
    if !enabled && kind == ButtonKind::Primary {
        painter.rect_filled(rect, corner, Palette::tint(p.surface_raised, 0.5));
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, label, button_font(), ink);
    if resp.has_focus() {
        focus_ring(painter, rect, radius::CONTROL, p);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    resp
}

/// A square button that shows only an icon; `paint` draws it in the colour it is given.
pub fn icon_button(ui: &mut Ui, p: &Palette, rect: Rect, id: Id, label: &str, on: bool, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let h = hover_fill(ui, ui.painter(), p, rect, radius::CONTROL, &resp);
    let color = if on { p.accent } else { Palette::mix(p.text_muted, p.text, h) };
    paint(ui.painter(), Rect::from_center_size(rect.center(), vec2(size::ICON, size::ICON)), color);
    if resp.has_focus() {
        focus_ring(ui.painter(), rect, radius::CONTROL, p);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label));
    resp
}

/// A keyboard shortcut drawn as a keycap, its right edge at `right`; returns its left edge.
pub fn keycap(painter: &egui::Painter, p: &Palette, right: egui::Pos2, keys: &str) -> f32 {
    let galley = painter.layout_no_wrap(keys.to_string(), text::meta(), p.text_muted);
    let r = Rect::from_min_max(pos2(right.x - galley.size().x - 12.0, right.y - 10.0), pos2(right.x, right.y + 10.0));
    painter.rect_filled(r, CornerRadius::same(radius::CHIP), p.sunken);
    painter.rect_stroke(r, CornerRadius::same(radius::CHIP), Stroke::new(1.0, p.hairline), egui::StrokeKind::Inside);
    painter.galley(pos2(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0), galley, p.text_muted);
    r.left()
}

/// A search field `width` wide: a recessed well with a magnifier, lit by the accent while it
/// has the keyboard.
pub fn search_field(ui: &mut Ui, p: &Palette, value: &mut String, id: Id, hint: &str, width: f32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(width, size::CONTROL_H), Sense::hover());
    let focused = ui.memory(|m| m.has_focus(id));
    let f = motion::fade(ui.ctx(), id.with("cw_focus"), focused, motion::HOVER);
    let corner = CornerRadius::same(radius::CONTROL);
    ui.painter().rect_filled(rect, corner, p.sunken);
    ui.painter().rect_stroke(rect, corner, Stroke::new(1.0 + 0.5 * f, Palette::mix(p.hairline, p.accent, f)), egui::StrokeKind::Inside);
    let icon = Rect::from_center_size(pos2(rect.left() + 15.0, rect.center().y), vec2(14.0, 14.0));
    icons::search(ui.painter(), icon, Palette::mix(p.text_faint, p.accent, f));
    // The text sits on the well's centre line: one row of the font, centred.
    let row = ui.fonts_mut(|f| f.row_height(&text::body()));
    let inner = Rect::from_min_max(pos2(rect.left() + 28.0, rect.center().y - row / 2.0), pos2(rect.right() - space::S, rect.center().y + row / 2.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .id(id)
                .hint_text(egui::RichText::new(hint).color(p.text_faint))
                .font(text::body())
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .desired_width(inner.width()),
        )
    })
    .inner
}

/// A section label in spaced small capitals, at `pos` (left, vertical centre).
pub fn caps(painter: &egui::Painter, pos: egui::Pos2, title: &str, color: Color32) {
    let galley = painter.layout_job(text::caps(title, color));
    painter.galley(pos2(pos.x, pos.y - galley.size().y / 2.0), galley, color);
}

/// A section label laid out by `ui`.
pub fn caps_label(ui: &mut Ui, p: &Palette, title: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.painter().layout_job(text::caps(title, p.text_muted)).size().x, 18.0), Sense::hover());
    caps(ui.painter(), pos2(rect.left(), rect.center().y), title, p.text_muted);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_button_reports_its_click_and_a_disabled_one_does_not() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(20.0, 20.0), vec2(80.0, 28.0));
        let click = |pos: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
            ]
        };
        for (enabled, want) in [(true, true), (false, false)] {
            let mut got = false;
            for events in [vec![], click(rect.center()), vec![]] {
                let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                    got |= button_at(ui, &Palette::LIGHT, rect, "Keep", ButtonKind::Primary, enabled).clicked();
                });
                out.textures_delta.clear();
            }
            assert_eq!(got, want, "enabled={enabled}");
        }
    }
}
