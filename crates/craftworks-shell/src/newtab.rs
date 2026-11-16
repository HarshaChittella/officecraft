//! Home, which is also the new tab (docs/design.md "Home"): what you see when Craftworks
//! opens, and what "+" and ⌘T show. A page of the window, not a panel over it: it fills the
//! space the document had and lays itself out to the window's width. A rail at the left
//! (Home, Open, Open Project), then "New" with each kind of file (choosing one opens that
//! app's own New page, with its templates), then "Recent" as a table with a search field.
//! Fully usable from the keyboard.

use crate::recent::{Recent, folder_label, relative_time};
use crate::tokens::{Palette, motion, radius, size, space, text};
use crate::{icons, widgets};
use chrono::{DateTime, Local, Timelike};
use craftworks_core::Kind;
use egui::{Align2, Color32, CornerRadius, Id, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::path::{Path, PathBuf};

/// What the user chose.
#[derive(Clone, Debug, PartialEq)]
pub enum NewTabAction {
    /// A blank file of this kind, straight away.
    New(Kind),
    /// The kind's own New page: a blank one or any of its app's templates.
    Templates(Kind),
    OpenPdf,
    OpenFile,
    /// Choose a folder to work in as a project (the Home rail).
    OpenProject,
    OpenRecent(PathBuf),
    ForgetRecent(PathBuf),
    Dismiss,
}

/// The "New" tiles, in order: kind, what it is called, and a line under it.
const CARDS: [(Kind, &str, &str); 4] = [
    (Kind::Document, "New document", "Blank, or from a template"),
    (Kind::Sheet, "New workbook", "Blank, or from a template"),
    (Kind::Presentation, "New presentation", "Blank, or from a template"),
    (Kind::Pdf, "Open a PDF", "Read, sign and edit"),
];

/// How many recent files the page lists.
const RECENT_ROWS: usize = 12;

#[derive(Debug, Default)]
pub struct NewTab {
    /// The page was asked for ("+", ⌘T or the Home tab) and shows in place of the document.
    pub open: bool,
    filter: String,
    /// The keyboard selection: 0–3 the tiles, 4 Open, 5… the recent rows.
    selected: usize,
    focus_filter: bool,
    /// When the page or popover came up (seconds of egui time), for its entrance.
    shown_at: Option<f64>,
}

fn filter_id() -> Id {
    Id::unique("cw_newtab_filter")
}

/// Context the page needs to show recent files.
pub struct RecentView<'a> {
    pub recent: &'a Recent,
    pub now: DateTime<Local>,
    pub home: Option<&'a Path>,
    /// Whether a file still exists (dimmed with "Not found" if not).
    pub exists: &'a dyn Fn(&Path) -> bool,
}

/// "Good morning", "Good afternoon" or "Good evening" for the hour of `now`.
pub fn greeting(now: DateTime<Local>) -> &'static str {
    match now.hour() {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}

impl NewTab {
    /// Brings the page up, from the top, with the keyboard in the search field.
    pub fn show(&mut self) {
        self.open = true;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.filter.clear();
        self.selected = 0;
        self.focus_filter = true;
        self.shown_at = None;
    }

    pub fn toggle(&mut self) {
        if self.open { self.open = false } else { self.show() }
    }

    /// Home, filling `rect`: the rail, a greeting, New and Recent.
    pub fn page(&mut self, ui: &mut Ui, rect: Rect, p: &Palette, view: &RecentView<'_>) -> Option<NewTabAction> {
        ui.painter().rect_filled(rect, 0.0, p.surface);
        let ctx = ui.ctx().clone();
        let since = *self.shown_at.get_or_insert_with(|| ctx.input(|i| i.time));
        let mut action = None;

        // The rail (dropped in a narrow window, where the content needs the room).
        let rail_w = if rect.width() >= 780.0 { size::RAIL_W } else { 0.0 };
        if rail_w > 0.0 {
            let rail = Rect::from_min_max(rect.min, pos2(rect.left() + rail_w, rect.bottom()));
            ui.painter().rect_filled(rail, 0.0, Palette::mix(p.surface, p.band, 0.55));
            ui.painter().vline(rail.right() - 0.5, rail.y_range(), Stroke::new(1.0, p.hairline));
            let mark = Rect::from_min_size(pos2(rail.left() + 20.0, rail.top() + 20.0), vec2(30.0, 30.0));
            icons::mark(ui.painter(), mark, p);
            let name = ui.painter().layout_job(text::tracked("Craftworks", text::heading(), p.text, -0.2));
            ui.painter().galley(pos2(mark.right() + 10.0, mark.center().y - name.size().y / 2.0), name, p.text);
            let mut y = mark.bottom() + space::XL;
            let mut item = |ui: &mut Ui, id: &str, label: &str, keys: Option<&str>, selected: bool, icon: fn(&egui::Painter, Rect, Color32)| {
                let r = Rect::from_min_size(pos2(rail.left() + 10.0, y), vec2(rail_w - 20.0, size::ROW_H));
                y += size::ROW_H + 2.0;
                nav_item(ui, p, r, Id::unique(("cw_home_nav", id)), label, keys, selected, icon)
            };
            let _ = item(ui, "home", "Home", None, true, icons::home);
            if item(ui, "open", "Open…", Some("⌘O"), self.selected == 4, icons::folder).clicked() {
                action = Some(NewTabAction::OpenFile);
            }
            if item(ui, "project", "Open Project…", None, false, icons::project).on_hover_text("Work in a folder: its files in the sidebar, searchable together").clicked() {
                action = Some(NewTabAction::OpenProject);
            }
            let tip = ui.painter().layout(
                "Tip: ⌘T opens a new tab from anywhere.".to_string(),
                text::meta(),
                p.text_muted,
                rail_w - 40.0,
            );
            ui.painter().galley(pos2(rail.left() + 20.0, rail.bottom() - 20.0 - tip.size().y), tip, p.text_muted);
        }

        // The content column: a greeting, then New and Recent, rising into place.
        let t = motion::enter(&ctx, since, 0.0, motion::MOVE);
        // The column takes the window's width up to a comfortable line length, with margins
        // that grow with it.
        let room = rect.width() - rail_w;
        let pad = (room * 0.055).clamp(space::L, 72.0);
        let w = (room - 2.0 * pad).min(1160.0);
        let left = rect.left() + rail_w + pad;
        let top = rect.top() + (rect.height() * 0.06).clamp(space::XL, 56.0);
        let mut hero = ui.painter().clone();
        hero.set_opacity(t);
        let title = hero.layout_job(text::tracked(greeting(view.now), text::title(), p.text, -0.5));
        let title_h = title.size().y;
        hero.galley(pos2(left, top + (1.0 - t) * 8.0), title, p.text);
        let inner = Rect::from_min_max(pos2(left, top + title_h + space::XL + (1.0 - t) * 12.0), pos2(left + w, rect.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.set_opacity(t);
            ui.set_width(w);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_width(w);
                if let Some(a) = self.contents(ui, p, view, since) {
                    action = Some(a);
                }
                ui.add_space(space::XL);
            });
        });
        // Anything chosen (or Esc) puts the page away; forgetting a missing file keeps it.
        if action.as_ref().is_some_and(|a| !matches!(a, NewTabAction::ForgetRecent(_))) {
            self.open = false;
            self.shown_at = None;
        }
        action
    }

    fn contents(&mut self, ui: &mut Ui, p: &Palette, view: &RecentView<'_>, since: f64) -> Option<NewTabAction> {
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
        ui.spacing_mut().item_spacing.y = 0.0;

        widgets::caps_label(ui, p, "New");
        ui.add_space(space::S);
        // The tiles share the width: four across, two when the window is narrow, one when
        // it is very narrow; they and their icons grow with the room they have.
        let avail = ui.available_width();
        let gap = space::M;
        let cols = if avail >= 4.0 * 150.0 + 3.0 * gap { 4 } else if avail >= 2.0 * 150.0 + gap { 2 } else { 1 };
        let w = ((avail - (cols as f32 - 1.0) * gap) / cols as f32).floor().min(300.0);
        let desk_h = (w * 0.5).clamp(96.0, 136.0);
        for (r, row) in CARDS.chunks(cols).enumerate() {
            if r > 0 {
                ui.add_space(gap);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (c, (kind, label, hint)) in row.iter().enumerate() {
                    let i = r * cols + c;
                    let arrive = motion::enter(ui.ctx(), since, i as f32 * motion::STAGGER, motion::MOVE);
                    if tile(ui, p, *kind, label, hint, vec2(w, desk_h), self.selected == i, arrive).clicked() || (enter && self.selected == i) {
                        action = Some(if *kind == Kind::Pdf { NewTabAction::OpenPdf } else { NewTabAction::Templates(*kind) });
                    }
                }
            });
        }
        // Open lives in the rail; Enter on it from the keyboard works wherever the rail is.
        if enter && self.selected == 4 {
            action = Some(NewTabAction::OpenFile);
        }
        ui.add_space(space::XL);
        ui.horizontal(|ui| {
            ui.set_height(size::CONTROL_H);
            widgets::caps_label(ui, p, "Recent");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let field = widgets::search_field(ui, p, &mut self.filter, filter_id(), "Search recent files", (avail * 0.34).clamp(180.0, 320.0));
                if self.focus_filter {
                    field.request_focus();
                    self.focus_filter = false;
                }
                if field.changed() {
                    self.selected = if self.filter.trim().is_empty() { 0 } else { 5 };
                }
            });
        });
        ui.add_space(space::S);
        // The table's columns: Name takes 46%, Location the rest up to the date.
        let col = |rect: Rect| (rect.left() + TEXT_X, rect.left() + rect.width() * 0.48, rect.right() - space::S);
        if !rows.is_empty() {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
            let (name_x, loc_x, right) = col(rect);
            let painter = ui.painter();
            painter.text(pos2(name_x, rect.center().y), Align2::LEFT_CENTER, "Name", text::meta(), p.text_muted);
            painter.text(pos2(loc_x, rect.center().y), Align2::LEFT_CENTER, "Location", text::meta(), p.text_muted);
            painter.text(pos2(right, rect.center().y), Align2::RIGHT_CENTER, "Opened", text::meta(), p.text_muted);
            painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.hairline));
            ui.add_space(2.0);
        } else {
            let (msg, sub) = if self.filter.trim().is_empty() {
                ("No recent files yet", "Files you open show up here.")
            } else {
                ("No recent file matches", "Try fewer letters, or open the file with ⌘O.")
            };
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::hover());
            ui.painter().rect_stroke(rect.shrink(0.5), CornerRadius::same(radius::CARD), Stroke::new(1.0, p.hairline), egui::StrokeKind::Inside);
            ui.painter().text(rect.center() - vec2(0.0, 9.0), Align2::CENTER_CENTER, msg, text::card(), p.text_muted);
            ui.painter().text(rect.center() + vec2(0.0, 10.0), Align2::CENTER_CENTER, sub, text::meta(), p.text_muted);
        }
        for (k, f) in rows.iter().enumerate() {
            let exists = (view.exists)(&f.path);
            let selected = self.selected == 5 + k;
            let name = f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let resp = row_frame(ui, p, selected, |ui, p, rect| {
                let painter = ui.painter();
                let (name_x, loc_x, right) = col(rect);
                let ink = if exists { p.text } else { p.text_muted };
                let color = if exists { Color32::WHITE } else { Color32::WHITE.gamma_multiply(0.45) };
                icons::kind(painter, icon_slot(rect), f.kind, color);
                let when = if exists { relative_time(f.opened, view.now) } else { "Not found".into() };
                let when = painter.layout_no_wrap(when, text::meta(), p.text_muted);
                let when_x = right - when.size().x;
                painter.galley(pos2(when_x, rect.center().y - when.size().y / 2.0), when, p.text_muted);
                let name = painter.layout_job(one_line(&name, text::body(), ink, (loc_x - space::M - name_x).max(0.0)));
                painter.galley(pos2(name_x, rect.center().y - name.size().y / 2.0), name, ink);
                let folder = painter.layout_job(one_line(&folder_label(&f.path, view.home), text::meta(), p.text_muted, (when_x - space::M - loc_x).max(0.0)));
                painter.galley(pos2(loc_x, rect.center().y - folder.size().y / 2.0 + 0.5), folder, p.text_muted);
            });
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
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

/// Where a row's text starts, after its icon.
const TEXT_X: f32 = 36.0;

/// The icon's place at the left of a row.
fn icon_slot(row: Rect) -> Rect {
    Rect::from_center_size(pos2(row.left() + 18.0, row.center().y), vec2(size::ICON, size::ICON))
}

/// `s` on one line, ending in "…" when it is wider than `max_w`.
fn one_line(s: &str, font: egui::FontId, color: Color32, max_w: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple_singleline(s.to_string(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
    job
}

/// One line of the Home rail: an icon, a label, and perhaps a shortcut. The current place is
/// marked with a bar of the accent at its left edge, as the office apps mark theirs.
#[allow(clippy::too_many_arguments)]
fn nav_item(ui: &mut Ui, p: &Palette, rect: Rect, id: Id, label: &str, keys: Option<&str>, selected: bool, icon: fn(&egui::Painter, Rect, Color32)) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    let corner = CornerRadius::same(radius::CONTROL);
    let sel = motion::fade(ui.ctx(), id.with("cw_sel"), selected, motion::HOVER);
    widgets::hover_fill(ui, ui.painter(), p, rect, radius::CONTROL, &resp);
    let painter = ui.painter();
    if sel > 0.0 {
        painter.rect_filled(rect, corner, Palette::tint(p.surface_raised, sel));
        painter.rect_stroke(rect, corner, Stroke::new(1.0, Palette::tint(p.hairline, sel)), egui::StrokeKind::Inside);
        let bar = Rect::from_min_max(pos2(rect.left(), rect.center().y - 8.0 * sel), pos2(rect.left() + 3.0, rect.center().y + 8.0 * sel));
        painter.rect_filled(bar, CornerRadius::same(2), p.accent);
    }
    if resp.has_focus() {
        widgets::focus_ring(painter, rect.shrink(2.0), radius::CONTROL, p);
    }
    icon(painter, Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(20.0, 20.0)), Color32::WHITE);
    let font = if selected { text::card() } else { text::body() };
    painter.text(pos2(rect.left() + 42.0, rect.center().y), Align2::LEFT_CENTER, label, font, p.text);
    if let Some(k) = keys {
        widgets::keycap(painter, p, pos2(rect.right() - space::S, rect.center().y), k);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    resp
}

/// A "New" tile: the kind's glass icon on a desk the colour of the apps' canvases, with its
/// name under it. On hover the desk takes the kind's colour and the icon rises and grows a
/// touch; `arrive` (0–1) is its entrance.
#[allow(clippy::too_many_arguments)]
fn tile(ui: &mut Ui, p: &Palette, kind: Kind, label: &str, hint: &str, size: egui::Vec2, selected: bool, arrive: f32) -> egui::Response {
    let (slot, resp) = ui.allocate_exact_size(vec2(size.x, size.y + 48.0), Sense::click());
    let (h, down) = widgets::hover(ui, &resp);
    let color = p.kind(kind);
    let mut painter = ui.painter().clone();
    if arrive < 1.0 {
        painter.multiply_opacity(arrive);
    }
    let rect = slot.translate(vec2(0.0, (1.0 - arrive) * 10.0));
    let desk = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + size.y));
    let corner = CornerRadius::same(radius::CARD);
    painter.rect_filled(desk, corner, Palette::mix(p.sunken, color, if p.dark { 0.18 } else { 0.12 } * h));
    let stroke = if selected { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, Palette::mix(p.border, color, 0.7 * h)) };
    painter.rect_stroke(desk, corner, stroke, egui::StrokeKind::Inside);
    // The icon: it rises 3 pt and grows on hover, and settles when pressed.
    let lift = if down { 0.0 } else { h };
    let side = size.y * 0.64 + 4.0 * lift;
    icons::kind(&painter, Rect::from_center_size(desk.center() - vec2(0.0, 3.0 * lift), vec2(side, side)), kind, Color32::WHITE);
    painter.text(pos2(rect.center().x, desk.bottom() + 17.0), Align2::CENTER_CENTER, label, text::card(), p.text);
    painter.text(pos2(rect.center().x, desk.bottom() + 35.0), Align2::CENTER_CENTER, hint, text::meta(), p.text_muted);
    if resp.has_focus() {
        widgets::focus_ring(&painter, desk, radius::CARD, p);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

/// A full-width clickable row with an eased hover and a selection background; `add` paints
/// into the row's rect.
fn row_frame(ui: &mut Ui, p: &Palette, selected: bool, add: impl FnOnce(&mut Ui, &Palette, Rect)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), size::ROW_H), Sense::click());
    let corner = CornerRadius::same(radius::CONTROL);
    let sel = motion::fade(ui.ctx(), resp.id.with("cw_sel"), selected, motion::HOVER);
    widgets::hover_fill(ui, ui.painter(), p, rect, radius::CONTROL, &resp);
    if sel > 0.0 {
        ui.painter().rect_filled(rect, corner, Palette::tint(p.accent, if p.dark { 0.18 } else { 0.09 } * sel));
        ui.painter().rect_stroke(rect, corner, Stroke::new(1.0, Palette::tint(p.accent, 0.45 * sel)), egui::StrokeKind::Inside);
    }
    add(ui, p, rect);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn frame(ctx: &egui::Context, nt: &mut NewTab, view: &RecentView<'_>, events: Vec<egui::Event>) -> Option<NewTabAction> {
        let mut got = None;
        let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            got = nt.page(ui, Rect::from_min_size(pos2(0.0, 40.0), vec2(1200.0, 800.0)), &Palette::DARK, view);
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
        nt.show();
        assert_eq!(frame(&ctx, &mut nt, &view, vec![]), None);
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::Templates(Kind::Document)));
        assert!(!nt.open, "choosing puts the page away");

        nt.show();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        for _ in 0..3 {
            let _ = frame(&ctx, &mut nt, &view, key(Key::ArrowDown));
        }
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::OpenPdf));

        nt.show();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        for _ in 0..9 {
            let _ = frame(&ctx, &mut nt, &view, key(Key::ArrowDown));
        }
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::OpenRecent(PathBuf::from("/w/Budget.xlsx"))));

        nt.show();
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
        nt.show();
        let _ = frame(&ctx, &mut nt, &view, vec![]);
        for _ in 0..5 {
            let _ = frame(&ctx, &mut nt, &view, key(Key::ArrowDown));
        }
        assert_eq!(frame(&ctx, &mut nt, &view, key(Key::Enter)), Some(NewTabAction::ForgetRecent(PathBuf::from("/gone/Old.pdf"))));
    }
}
