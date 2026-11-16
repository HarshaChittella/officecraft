//! The AI's status in the title row and the review panel (docs/design.md "AI status and
//! review"): a pill at the right of the tabs (a sparkle, then `AI · editing Budget.xlsx` or
//! `AI · 3 changes to review`), and, from it, a popover listing the files the AI changed with Keep
//! (save) and Undo for each. Nothing the AI does is saved until you keep it (unless the
//! project allows the AI to save).

use crate::tokens::{Palette, motion, size, space, text};
use crate::widgets::{self, ButtonKind};
use crate::icons;
use craftworks_core::Kind;
use egui::{Align2, CornerRadius, Id, Rect, Sense, Stroke, pos2, vec2};
use std::path::PathBuf;

/// One file the AI changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    pub path: PathBuf,
    pub kind: Kind,
    /// Edits since you last kept or undid them.
    pub edits: u32,
}

/// What the pill says.
#[derive(Clone, Debug, PartialEq)]
pub enum AiStatus {
    /// Connected, nothing to review.
    Idle,
    /// Editing this file now.
    Editing(String),
    /// Changes waiting for review.
    Review(usize),
}

impl AiStatus {
    pub fn label(&self) -> String {
        match self {
            AiStatus::Idle => "AI · idle".into(),
            AiStatus::Editing(f) => format!("AI · editing {f}"),
            AiStatus::Review(1) => "AI · 1 change to review".into(),
            AiStatus::Review(n) => format!("AI · {n} changes to review"),
        }
    }
}

/// What the user did with the AI's changes.
#[derive(Clone, Debug, PartialEq)]
pub enum ReviewAction {
    Keep(PathBuf),
    Undo(PathBuf),
    KeepAll,
}

/// The pill's width for `status`.
pub fn pill_width(ui: &egui::Ui, status: &AiStatus) -> f32 {
    ui.fonts_mut(|f| f.layout_no_wrap(status.label(), text::meta(), egui::Color32::WHITE).size().x).min(240.0) + 42.0
}

/// Draws the pill in `r`; true when clicked.
pub fn pill(ui: &mut egui::Ui, r: Rect, p: &Palette, status: &AiStatus) -> bool {
    let resp = ui.interact(r, Id::unique("cw_ai_pill"), Sense::click());
    let attention = matches!(status, AiStatus::Review(_));
    let (h, down) = widgets::hover(ui, &resp);
    let att = motion::fade(ui.ctx(), resp.id.with("cw_att"), attention, motion::MOVE);
    let rest = Palette::tint(p.surface_hover, if down { 1.0 } else { h });
    let fill = Palette::mix(rest, Palette::tint(p.accent, if p.dark { 0.24 } else { 0.13 } + 0.05 * h), att);
    let corner = CornerRadius::same(255);
    ui.painter().rect_filled(r, corner, fill);
    ui.painter().rect_stroke(r, corner, Stroke::new(1.0, Palette::mix(p.hairline, Palette::tint(p.accent, 0.45), att)), egui::StrokeKind::Inside);
    if resp.has_focus() {
        widgets::focus_ring(ui.painter(), r, 12, p);
    }
    let color = Palette::mix(Palette::mix(p.text_muted, p.text, h), p.accent, att);
    // The sparkle is drawn (the interface font has no ✦); it breathes while the AI is editing.
    let breathe = if matches!(status, AiStatus::Editing(_)) && !motion::reduced(ui.ctx()) {
        ui.ctx().request_repaint();
        0.65 + 0.35 * ((ui.input(|i| i.time) * 3.0).sin() as f32 * 0.5 + 0.5)
    } else {
        1.0
    };
    icons::sparkle(ui.painter(), Rect::from_center_size(pos2(r.left() + 17.0, r.center().y), vec2(16.0, 16.0)), egui::Color32::WHITE.gamma_multiply(breathe));
    let galley = ui.fonts_mut(|f| {
        let mut job = egui::text::LayoutJob::simple_singleline(status.label(), text::meta(), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 40.0);
        f.layout_job(job)
    });
    ui.painter().galley(pos2(r.left() + 30.0, r.center().y - galley.size().y / 2.0), galley, color);
    let resp = resp.on_hover_text("What the AI changed: keep or undo each file");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, status.label()));
    resp.clicked()
}

/// The review popover under `anchor`. Returns what was chosen; `open` goes false when it
/// should close.
pub fn review(ctx: &egui::Context, p: &Palette, anchor: Rect, changed: &[Changed], can_save_all: bool, open: &mut bool) -> Vec<ReviewAction> {
    let mut out = Vec::new();
    let width = 360.0;
    // It drops in from under the pill, as the new-tab panel does from "+".
    let t = motion::fade(ctx, Id::unique("cw_ai_review_in"), true, motion::PANEL);
    let pos = pos2((anchor.right() - width).max(8.0), anchor.bottom() + 6.0 - (1.0 - t) * 6.0);
    let area = egui::Area::new(Id::unique("cw_ai_review")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        ui.set_opacity(t);
        widgets::panel_frame(p).show(ui, |ui| {
            ui.set_width(width - 2.0 * space::L);
            ui.spacing_mut().item_spacing.y = space::XS;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(size::ICON, size::ICON), Sense::hover());
                icons::sparkle(ui.painter(), r, egui::Color32::WHITE);
                ui.label(egui::RichText::new("AI changes").font(text::heading()).color(p.text));
            });
            if changed.is_empty() {
                ui.add_space(space::XS);
                ui.label(
                    egui::RichText::new("Nothing to review. Edits the AI makes to open files show up here before anything is saved.")
                        .font(text::meta())
                        .color(p.text_muted),
                );
                return;
            }
            ui.label(egui::RichText::new("Nothing is saved until you keep it.").font(text::meta()).color(p.text_muted));
            ui.add_space(space::S);
            for c in changed {
                let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                ui.painter().hline(row.x_range(), row.top(), Stroke::new(1.0, p.hairline));
                let icon = Rect::from_center_size(pos2(row.left() + 10.0, row.center().y), vec2(18.0, 18.0));
                icons::kind(ui.painter(), icon, c.kind, egui::Color32::WHITE);
                let name = c.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let summary = if c.edits == 1 { "1 edit".to_string() } else { format!("{} edits", c.edits) };
                let (undo_w, keep_w) = (widgets::button_width(ui, "Undo"), widgets::button_width(ui, "Keep"));
                let keep = Rect::from_min_size(pos2(row.right() - keep_w, row.center().y - size::CONTROL_H / 2.0), vec2(keep_w, size::CONTROL_H));
                let undo = Rect::from_min_size(pos2(keep.left() - space::S - undo_w, keep.top()), vec2(undo_w, size::CONTROL_H));
                let mut job = egui::text::LayoutJob::simple_singleline(name, text::body(), p.text);
                job.wrap = egui::text::TextWrapping::truncate_at_width((undo.left() - space::M - (row.left() + 28.0)).max(10.0));
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(pos2(row.left() + 28.0, row.center().y - 8.0 - galley.size().y / 2.0), galley, p.text);
                ui.painter().text(pos2(row.left() + 28.0, row.center().y + 9.0), Align2::LEFT_CENTER, summary, text::meta(), p.text_muted);
                let id = ui.scope_id().with(&c.path);
                if ui.push_id(id.with("u"), |ui| widgets::button_at(ui, p, undo, "Undo", ButtonKind::Secondary, true)).inner.on_hover_text("Undo the AI's edits to this file").clicked() {
                    out.push(ReviewAction::Undo(c.path.clone()));
                }
                if ui.push_id(id.with("k"), |ui| widgets::button_at(ui, p, keep, "Keep", ButtonKind::Primary, true)).inner.on_hover_text("Save the file with the AI's edits").clicked() {
                    out.push(ReviewAction::Keep(c.path.clone()));
                }
            }
            let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().hline(line.x_range(), line.top(), Stroke::new(1.0, p.hairline));
            ui.add_space(space::S);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::button(ui, p, "Keep all", ButtonKind::Secondary, can_save_all).on_hover_text("Save every file above").clicked() {
                    out.push(ReviewAction::KeepAll);
                }
            });
        });
    });
    let clicked_outside = ctx.input(|i| i.pointer.any_click())
        && !area.response.contains_pointer()
        && !ctx.input(|i| i.pointer.interact_pos().is_some_and(|q| anchor.contains(q)));
    if clicked_outside || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = false;
        // The next opening fades in from nothing again.
        ctx.animate_bool_with_time(Id::unique("cw_ai_review_in"), false, 0.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pill_says_what_the_ai_is_doing() {
        assert_eq!(AiStatus::Idle.label(), "AI · idle");
        assert_eq!(AiStatus::Editing("Budget.xlsx".into()).label(), "AI · editing Budget.xlsx");
        assert_eq!(AiStatus::Review(1).label(), "AI · 1 change to review");
        assert_eq!(AiStatus::Review(3).label(), "AI · 3 changes to review");
    }
}
