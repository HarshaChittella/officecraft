//! The new-tab page (docs/design.md "New-tab page and start page"): four create cards,
//! Open File…, and recent files filtered as you type. Shown as a popover under "+" (⌘T), or
//! filling the window as the start page when no tab is open. Fully usable from the keyboard.

use crate::icons;
use crate::recent::{Recent, folder_label, relative_time};
use crate::tokens::{Palette, motion, radius, size, text};
use chrono::{DateTime, Local};
use craftworks_core::Kind;
use egui::{CornerRadius, Id, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::path::{Path, PathBuf};

/// What the user chose.
#[derive(Clone, Debug, PartialEq)]
pub enum NewTabAction {
    New(Kind),
    OpenPdf,
    OpenFile,
    OpenRecent(PathBuf),
    ForgetRecent(PathBuf),
    Dismiss,
}

/// The cards, in order.
const CARDS: [(Kind, &str); 4] =
    [(Kind::Document, "Document"), (Kind::Sheet, "Sheet"), (Kind::Presentation, "Presentation"), (Kind::Pdf, "Open PDF")];

/// How many recent files the page lists.
const RECENT_ROWS: usize = 12;

#[derive(Debug, Default)]
pub struct NewTab {
    /// The popover is showing.
    pub open: bool,
    filter: String,
    /// The keyboard selection: 0–3 the cards, 4 Open File…, 5… the recent rows.
    selected: usize,
    focus_filter: bool,
}

/// Context the page needs to show recent files.
pub struct RecentView<'a> {
    pub recent: &'a Recent,
    pub now: DateTime<Local>,
    pub home: Option<&'a Path>,
    /// Whether a file still exists (dimmed with "Not found" if not).
    pub exists: &'a dyn Fn(&Path) -> bool,
}

impl NewTab {
    pub fn show_popover(&mut self) {
        self.open = true;
        self.filter.clear();
        self.selected = 0;
        self.focus_filter = true;
    }

    pub fn toggle(&mut self) {
        if self.open { self.open = false } else { self.show_popover() }
    }

    /// The popover under `anchor` (the "+" button). Closes on Esc or a click outside it.
    pub fn popover(&mut self, ctx: &egui::Context, p: &Palette, anchor: Rect, view: &RecentView<'_>) -> Option<NewTabAction> {
        if !self.open {
            return None;
        }
        let reduce = ctx.global_style().animation_time <= 0.0;
        let t = ctx.animate_bool_with_time(Id::unique("cw_newtab_fade"), true, motion::dur(motion::PANEL, reduce));
        let screen = ctx.content_rect();
        let x = anchor.left().min(screen.right() - size::PANEL_W - 12.0).max(screen.left() + 12.0);
        let mut action = None;
        let area = egui::Area::new(Id::unique("cw_newtab"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(x, anchor.bottom() + 6.0 + (1.0 - t) * 4.0))
            .show(ctx, |ui| {
                ui.set_opacity(t);
                panel_frame(p).show(ui, |ui| {
                    ui.set_width(size::PANEL_W);
                    action = self.contents(ui, p, view);
                });
            });
        let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
            && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos));
        if action.is_none() && clicked_outside {
            action = Some(NewTabAction::Dismiss);
        }
        if action.is_some() {
            self.open = false;
            ctx.animate_bool_with_time(Id::unique("cw_newtab_fade"), false, 0.0);
        }
        action
    }

    /// The start page: the same choices, centred in `rect`, with a welcome above them.
    pub fn page(&mut self, ui: &mut Ui, rect: Rect, p: &Palette, view: &RecentView<'_>) -> Option<NewTabAction> {
        ui.painter().rect_filled(rect, 0.0, p.surface);
        let w = size::PANEL_W + 32.0;
        let top = rect.top() + (rect.height() * 0.12).max(24.0);
        let inner = Rect::from_min_size(pos2(rect.center().x - w / 2.0, top), vec2(w, rect.bottom() - top));
        let mut action = None;
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.label(egui::RichText::new("Craftworks").font(text::title()).color(p.text));
            ui.label(egui::RichText::new("Start something new, or pick up where you left off.").color(p.text_muted));
            ui.add_space(16.0);
            panel_frame(p).show(ui, |ui| {
                ui.set_width(size::PANEL_W);
                action = self.contents(ui, p, view);
            });
        });
        action.filter(|a| *a != NewTabAction::Dismiss)
    }

    fn contents(&mut self, ui: &mut Ui, p: &Palette, view: &RecentView<'_>) -> Option<NewTabAction> {
        let mut action = None;
        let rows: Vec<_> = view.recent.matching(&self.filter).into_iter().take(RECENT_ROWS).collect();
        let n = 5 + rows.len();
        // Keyboard: ↑/↓ move through everything, Enter opens, Esc closes.
        let (up, down, enter, esc) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if down {
            self.selected = (self.selected + 1).min(n - 1);
        }
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        self.selected = self.selected.min(n - 1);

        ui.label(egui::RichText::new("Create").font(text::section()).color(p.text_muted));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            // Four cards share the panel's width.
            let w = ((ui.available_width() - 3.0 * 12.0) / 4.0).floor().max(size::CARD_W);
            for (i, (kind, label)) in CARDS.iter().enumerate() {
                if card(ui, p, *kind, label, w, self.selected == i).clicked() || (enter && self.selected == i) {
                    action = Some(if *kind == Kind::Pdf { NewTabAction::OpenPdf } else { NewTabAction::New(*kind) });
                }
            }
        });
        ui.add_space(10.0);
        let open = row_frame(ui, p, self.selected == 4, |ui, p| {
            icons::folder(ui.painter(), Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(16.0, 16.0)), p.text_muted);
            ui.add_space(22.0);
            ui.label(egui::RichText::new("Open file…").color(p.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(egui::RichText::new("⌘O").color(p.text_muted)));
        });
        if open.clicked() || (enter && self.selected == 4) {
            action = Some(NewTabAction::OpenFile);
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Recent").font(text::section()).color(p.text_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let field =
                    ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Filter recent files").desired_width(220.0).margin(vec2(8.0, 4.0)));
                if self.focus_filter {
                    field.request_focus();
                    self.focus_filter = false;
                }
                if field.changed() {
                    self.selected = if self.filter.trim().is_empty() { 0 } else { 5 };
                }
            });
        });
        ui.add_space(4.0);
        if rows.is_empty() {
            let msg = if self.filter.trim().is_empty() { "Files you open show up here." } else { "No recent file matches." };
            ui.label(egui::RichText::new(msg).color(p.text_muted));
        }
        for (k, f) in rows.iter().enumerate() {
            let exists = (view.exists)(&f.path);
            let selected = self.selected == 5 + k;
            let name = f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let resp = row_frame(ui, p, selected, |ui, p| {
                let ink = if exists { p.text } else { p.text_muted };
                icons::kind(
                    ui.painter(),
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 2.0), vec2(16.0, 16.0)),
                    f.kind,
                    p.kind(f.kind),
                    p.text_muted,
                );
                ui.add_space(22.0);
                ui.label(egui::RichText::new(&name).color(ink));
                ui.label(egui::RichText::new(folder_label(&f.path, view.home)).font(text::meta()).color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let when = if exists { relative_time(f.opened, view.now) } else { "Not found".into() };
                    ui.label(egui::RichText::new(when).font(text::meta()).color(p.text_muted));
                });
            });
            let resp = resp.on_hover_text(f.path.display().to_string());
            if resp.clicked() || (enter && selected) {
                action = Some(if exists { NewTabAction::OpenRecent(f.path.clone()) } else { NewTabAction::ForgetRecent(f.path.clone()) });
            }
        }
        if esc {
            action = Some(NewTabAction::Dismiss);
        }
        action
    }
}

fn panel_frame(p: &Palette) -> egui::Frame {
    egui::Frame::NONE
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0, p.hairline))
        .corner_radius(CornerRadius::same(radius::POPOVER))
        .shadow(egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow })
        .inner_margin(16.0)
}

/// A create card: a large kind icon over its label; tinted with the kind's colour on hover.
fn card(ui: &mut Ui, p: &Palette, kind: Kind, label: &str, width: f32, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, size::CARD_H), Sense::click());
    let reduce = ui.ctx().global_style().animation_time <= 0.0;
    let h = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), motion::dur(motion::HOVER, reduce));
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(radius::CARD), Palette::tint(p.kind(kind), 0.10 * h));
    let stroke = if selected { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, p.hairline) };
    painter.rect_stroke(rect, CornerRadius::same(radius::CARD), stroke, egui::StrokeKind::Inside);
    let icon = Rect::from_center_size(rect.center() - vec2(0.0, 12.0), vec2(36.0, 36.0));
    icons::kind(painter, icon, kind, p.kind(kind), p.text_muted);
    painter.text(rect.center() + vec2(0.0, 26.0), egui::Align2::CENTER_CENTER, label, text::card(), p.text);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

/// A full-width clickable row with a hover and selection background.
fn row_frame(ui: &mut Ui, p: &Palette, selected: bool, add: impl FnOnce(&mut Ui, &Palette)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), size::ROW_H), Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(radius::CONTROL), p.surface_hover);
    }
    if selected {
        ui.painter().rect_stroke(rect, CornerRadius::same(radius::CONTROL), Stroke::new(1.0, Palette::tint(p.accent, 0.6)), egui::StrokeKind::Inside);
    }
    let inner = rect.shrink2(vec2(8.0, 5.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| add(ui, p));
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn frame(ctx: &egui::Context, nt: &mut NewTab, view: &RecentView<'_>, events: Vec<egui::Event>) -> Option<NewTabAction> {
        let mut got = None;
        let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            got = nt.popover(ui.ctx(), &Palette::DARK, Rect::from_min_size(pos2(10.0, 10.0), vec2(28.0, 28.0)), view);
        });
        out.textures_delta.clear();
        got
    }

    fn key(k: Key) -> Vec<egui::Event> {
        vec![
            egui::Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
            egui::Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: egui::Modifiers::NONE },
        ]
    }

    #[test]
    fn the_keyboard_reaches_every_choice() {
        let ctx = egui::Context::default();
        let mut recent = Recent::default();
        recent.opened(Path::new("/w/Budget.xlsx"), 0);
        let exists = |_: &Path| true;
        let view = RecentView {
            recent: &recent,
            now: Local.with_ymd_and_hms(2026, 11, 14, 12, 0, 0).single().unwrap_or_else(Local::now),
            home: None,
            exists: &exists,
        };
        let mut nt = NewTab::default();
        nt.show_popover();
        assert_eq!(frame(&ctx, &mut nt, &view, vec![]), None);
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::New(Kind::Document)));
        assert!(!nt.open, "choosing closes the popover");

        nt.show_popover();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        for _ in 0..3 {
            let _ = frame(&ctx, &mut nt, &view, key(Key::ArrowDown));
        }
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::OpenPdf));

        nt.show_popover();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        for _ in 0..9 {
            let _ = frame(&ctx, &mut nt, &view, key(Key::ArrowDown));
        }
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::OpenRecent(PathBuf::from("/w/Budget.xlsx"))));

        nt.show_popover();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Escape)), Some(NewTabAction::Dismiss));
    }

    #[test]
    fn a_missing_recent_file_offers_to_be_forgotten() {
        let ctx = egui::Context::default();
        let mut recent = Recent::default();
        recent.opened(Path::new("/gone/Old.pdf"), 0);
        let exists = |_: &Path| false;
        let view = RecentView { recent: &recent, now: Local::now(), home: None, exists: &exists };
        let mut nt = NewTab::default();
        nt.show_popover();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        for _ in 0..5 {
            let _ = frame(&ctx, &mut nt, &view, key(Key::ArrowDown));
        }
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::ForgetRecent(PathBuf::from("/gone/Old.pdf"))));
    }
}
