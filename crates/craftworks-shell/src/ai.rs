//! The AI's status in the title row and the review panel (docs/design.md "AI status and
//! review"): a pill at the right of the tabs (a sparkle, then `AI · editing Budget.xlsx` or
//! `AI · 3 changes to review`), and, from it, a popover listing the files the AI changed with Keep
//! (save) and Undo for each. Nothing the AI does is saved until you keep it (unless the
//! project allows the AI to save).

use crate::{icons, tokens::Palette, tokens::radius, tokens::text};
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
    ui.fonts_mut(|f| f.layout_no_wrap(status.label(), text::meta(), egui::Color32::WHITE).size().x).min(240.0) + 40.0
}

/// Draws the pill in `r`; true when clicked.
pub fn pill(ui: &mut egui::Ui, r: Rect, p: &Palette, status: &AiStatus) -> bool {
    let resp = ui.interact(r, Id::unique("cw_ai_pill"), Sense::click());
    let attention = matches!(status, AiStatus::Review(_));
    let fill = if attention {
        Palette::tint(p.accent, 0.16)
    } else if resp.hovered() {
        p.surface_hover
    } else {
        p.surface
    };
    ui.painter().rect_filled(r, CornerRadius::same(radius::TAB), fill);
    ui.painter().rect_stroke(r, CornerRadius::same(radius::TAB), Stroke::new(1.0, p.hairline), egui::StrokeKind::Inside);
    let color = if attention { p.accent } else { p.text_muted };
    // The sparkle is drawn: the interface font has no ✦.
    icons::sparkle(ui.painter(), Rect::from_center_size(pos2(r.left() + 17.0, r.center().y), vec2(14.0, 14.0)), p.accent);
    let galley = ui.fonts_mut(|f| {
        let mut job = egui::text::LayoutJob::simple_singleline(status.label(), text::meta(), color);
        job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 36.0);
        f.layout_job(job)
    });
    ui.painter().galley(pos2(r.left() + 29.0, r.center().y - galley.size().y / 2.0), galley, color);
    let resp = resp.on_hover_text("What the AI changed: keep or undo each file");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, status.label()));
    resp.clicked()
}

/// The review popover under `anchor`. Returns what was chosen; `open` goes false when it
/// should close.
pub fn review(ctx: &egui::Context, p: &Palette, anchor: Rect, changed: &[Changed], can_save_all: bool, open: &mut bool) -> Vec<ReviewAction> {
    let mut out = Vec::new();
    let width = 340.0;
    let pos = pos2((anchor.right() - width).max(8.0), anchor.bottom() + 6.0);
    let area = egui::Area::new(Id::unique("cw_ai_review")).order(egui::Order::Foreground).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::new()
            .fill(p.surface_raised)
            .stroke(Stroke::new(1.0, p.hairline))
            .corner_radius(CornerRadius::same(radius::POPOVER))
            .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow })
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.label(egui::RichText::new("AI changes").font(text::card()).color(p.text));
                ui.add_space(2.0);
                if changed.is_empty() {
                    ui.label(
                        egui::RichText::new("Nothing to review. Edits the AI makes to open files show up here before anything is saved.")
                            .font(text::meta())
                            .color(p.text_muted),
                    );
                    return;
                }
                ui.label(egui::RichText::new("Nothing is saved until you keep it.").font(text::meta()).color(p.text_muted));
                ui.add_space(6.0);
                for c in changed {
                    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
                    let icon = Rect::from_center_size(pos2(row.left() + 10.0, row.center().y), vec2(16.0, 16.0));
                    icons::kind(ui.painter(), icon, c.kind, p.kind(c.kind), p.text_muted);
                    let name = c.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let summary = if c.edits == 1 { "1 edit".to_string() } else { format!("{} edits", c.edits) };
                    ui.painter().text(pos2(row.left() + 26.0, row.center().y - 7.0), Align2::LEFT_CENTER, &name, text::body(), p.text);
                    ui.painter().text(pos2(row.left() + 26.0, row.center().y + 8.0), Align2::LEFT_CENTER, summary, text::meta(), p.text_muted);
                    let undo = Rect::from_min_size(pos2(row.right() - 120.0, row.center().y - 12.0), vec2(56.0, 24.0));
                    let keep = Rect::from_min_size(pos2(row.right() - 60.0, row.center().y - 12.0), vec2(60.0, 24.0));
                    if ui.put(undo, egui::Button::new("Undo")).on_hover_text("Undo the AI's edits to this file").clicked() {
                        out.push(ReviewAction::Undo(c.path.clone()));
                    }
                    if ui.put(keep, egui::Button::new("Keep")).on_hover_text("Save the file with the AI's edits").clicked() {
                        out.push(ReviewAction::Keep(c.path.clone()));
                    }
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(can_save_all, egui::Button::new("Keep all")).on_hover_text("Save every file above").clicked() {
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
