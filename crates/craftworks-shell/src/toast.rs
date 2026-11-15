//! Short, non-blocking notices at the bottom centre of the window (docs/design.md "Toasts").

use crate::tokens::{Palette, motion, radius, text};
use egui::{CornerRadius, Id, Rect, Stroke, pos2, vec2};

/// How long a toast stays (unless hovered), in seconds.
pub const SHOW_FOR: f64 = 4.0;

#[derive(Clone, Debug)]
struct Toast {
    id: u64,
    text: String,
    error: bool,
    action: Option<String>,
    shown_at: Option<f64>,
}

#[derive(Debug, Default)]
pub struct Toasts {
    list: Vec<Toast>,
    next: u64,
}

impl Toasts {
    pub fn info(&mut self, text: impl Into<String>) -> u64 {
        self.push(text.into(), false, None)
    }

    pub fn error(&mut self, text: impl Into<String>) -> u64 {
        self.push(text.into(), true, None)
    }

    /// A toast with a button; [`Toasts::show`] returns its id when the button is clicked.
    pub fn with_action(&mut self, text: impl Into<String>, action: impl Into<String>) -> u64 {
        self.push(text.into(), false, Some(action.into()))
    }

    fn push(&mut self, text: String, error: bool, action: Option<String>) -> u64 {
        self.next += 1;
        // The same message twice in a row shows once.
        self.list.retain(|t| t.text != text);
        self.list.push(Toast { id: self.next, text, error, action, shown_at: None });
        if self.list.len() > 3 {
            self.list.remove(0);
        }
        self.next
    }

    /// The texts of the toasts waiting or showing (for checks).
    pub fn texts(&self) -> Vec<String> {
        self.list.iter().map(|t| t.text.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Draws the toasts above the bottom of `area`; returns the id of a toast whose action
    /// button was clicked.
    pub fn show(&mut self, ctx: &egui::Context, p: &Palette, area: Rect) -> Option<u64> {
        let now = ctx.input(|i| i.time);
        let reduce = ctx.global_style().animation_time <= 0.0;
        let mut clicked = None;
        let mut y = area.bottom() - 20.0;
        let mut keep = Vec::new();
        for mut t in std::mem::take(&mut self.list).into_iter().rev() {
            let since = now - *t.shown_at.get_or_insert(now);
            let id = Id::unique(("cw_toast", t.id));
            let rise = ctx.animate_bool_with_time(id, since < SHOW_FOR, motion::dur(motion::MOVE, reduce));
            let resp = egui::Area::new(id)
                .order(egui::Order::Tooltip)
                .pivot(egui::Align2::CENTER_BOTTOM)
                .fixed_pos(pos2(area.center().x, y + (1.0 - rise) * 12.0))
                .show(ctx, |ui| {
                    ui.set_opacity(rise);
                    egui::Frame::NONE
                        .fill(p.surface_raised)
                        .stroke(Stroke::new(1.0, p.hairline))
                        .corner_radius(CornerRadius::same(radius::POPOVER))
                        .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: p.shadow })
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if t.error {
                                    let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
                                    ui.painter().circle_filled(r.center(), 4.0, p.danger);
                                }
                                ui.label(egui::RichText::new(&t.text).font(text::body()).color(p.text));
                                if let Some(a) = &t.action
                                    && ui.link(a).clicked()
                                {
                                    clicked = Some(t.id);
                                }
                            });
                        });
                });
            let hovered = resp.response.contains_pointer();
            if hovered {
                t.shown_at = Some(now - 1.0);
            }
            y = resp.response.rect.top() - 8.0;
            if since < SHOW_FOR + f64::from(motion::MOVE) || hovered {
                keep.push(t);
            }
        }
        keep.reverse();
        self.list = keep;
        if !self.list.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_stack_dedupe_and_expire() {
        let mut t = Toasts::default();
        t.info("Saved");
        t.info("Saved");
        assert_eq!(t.list.len(), 1, "the same message shows once");
        for i in 0..5 {
            t.error(format!("e{i}"));
        }
        assert_eq!(t.list.len(), 3, "at most three at a time");
        let ctx = egui::Context::default();
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        for time in [0.0, 1.0, 10.0] {
            let mut out = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, |ui| {
                let _ = t.show(ui.ctx(), &Palette::LIGHT, area);
            });
            out.textures_delta.clear();
        }
        assert!(t.is_empty(), "gone after {SHOW_FOR} s");
    }
}
