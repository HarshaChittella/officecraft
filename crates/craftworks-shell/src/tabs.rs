//! The tab strip (docs/design.md "Tab strip"): tabs that shrink to fit then scroll, animated
//! drag-to-reorder, a dirty dot that turns into × on hover, a context menu, file drops, and the
//! "+" button. It reports what the user did as [`TabAction`]s; the app acts on them.

use crate::icons;
use crate::tokens::{Palette, motion, radius, size, text};
use craftworks_core::Kind;
use egui::{Color32, CornerRadius, Id, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::path::PathBuf;

/// One tab as the strip shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct TabItem {
    pub id: u64,
    pub kind: Kind,
    /// The file name with its extension, or the engine's title for an untitled document.
    pub label: String,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    /// The file is still opening (PdfCraft reads in the background).
    pub busy: bool,
}

/// What the user did in the strip.
#[derive(Clone, Debug, PartialEq)]
pub enum TabAction {
    Activate(u64),
    Close(u64),
    CloseOthers(u64),
    CloseToRight(u64),
    Reveal(u64),
    CopyPath(u64),
    Move { id: u64, to: usize },
    NewTab,
    Drop { at: usize, paths: Vec<PathBuf> },
    DragWindow,
    ZoomWindow,
}

/// What the strip remembers between frames.
#[derive(Debug, Default)]
pub struct TabStripState {
    /// The tab being dragged, and where on it the pointer grabbed it.
    dragging: Option<(u64, f32)>,
    scroll: f32,
    last_active: Option<u64>,
    /// Where "+" was drawn, for the new-tab popover to hang from.
    pub plus_rect: Option<Rect>,
}

/// Lays `n` tabs into `avail` points: their width, and whether they overflow (then scroll).
pub fn layout(n: usize, avail: f32) -> (f32, bool) {
    if n == 0 {
        return (size::TAB_MAX, false);
    }
    let each = avail / n as f32 - size::TAB_GAP;
    let w = each.clamp(size::TAB_MIN, size::TAB_MAX);
    (w, each < size::TAB_MIN)
}

/// Where a tab dragged to x (its centre, in strip coordinates incl. scroll) lands.
pub fn drop_index(centre: f32, w: f32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let slot = (centre / (w + size::TAB_GAP)).floor();
    if slot.is_finite() && slot > 0.0 { (slot as usize).min(n - 1) } else { 0 }
}

/// `s` shortened in the middle with "…" so it fits `max_w`, keeping its end (the extension)
/// visible. `width` measures a candidate.
pub fn truncate_middle(s: &str, max_w: f32, width: impl Fn(&str) -> f32) -> String {
    if width(s) <= max_w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let candidate = |keep: usize| {
        let tail = keep / 2 + keep % 2;
        let head = keep - tail;
        let mut out: String = chars.iter().take(head).collect();
        out.push('…');
        out.extend(chars.iter().skip(chars.len().saturating_sub(tail)));
        out
    };
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&candidate(mid)) <= max_w { lo = mid } else { hi = mid - 1 }
    }
    candidate(lo)
}

/// Draws the strip in `rect` and returns what the user did.
pub fn tab_strip(ui: &mut Ui, rect: Rect, p: &Palette, items: &[TabItem], active: Option<u64>, st: &mut TabStripState) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let ctx = ui.ctx().clone();
    // Tabs fill the row less a small margin (the row is sized around the traffic lights).
    let th = (rect.height() - 6.0).clamp(22.0, size::TAB_HEIGHT);
    let reduce = ctx.global_style().animation_time <= 0.0;
    let plus_w = th;
    let avail = (rect.width() - plus_w - 8.0).max(0.0);
    let (w, overflow) = layout(items.len(), avail);
    let total = items.len() as f32 * (w + size::TAB_GAP);
    let tabs_area = Rect::from_min_max(rect.min, pos2(rect.left() + avail, rect.bottom()));

    // Empty title-row space moves and zooms the window (added first, so tabs sit on top).
    let bg = ui.interact(rect, Id::unique("cw_strip_bg"), Sense::click_and_drag());
    if bg.drag_started() {
        actions.push(TabAction::DragWindow);
    }
    if bg.double_clicked() {
        actions.push(TabAction::ZoomWindow);
    }

    // Scrolling: the wheel over the strip, and the active tab kept in view.
    let max_scroll = (total - avail).max(0.0);
    if overflow {
        if ui.rect_contains_pointer(tabs_area) {
            let d = ctx.input(|i| i.smooth_scroll_delta);
            st.scroll -= if d.x.abs() > d.y.abs() { d.x } else { d.y };
        }
        if active != st.last_active
            && let Some(i) = items.iter().position(|t| Some(t.id) == active)
        {
            let (l, r) = (i as f32 * (w + size::TAB_GAP), i as f32 * (w + size::TAB_GAP) + w);
            if l < st.scroll {
                st.scroll = l;
            } else if r > st.scroll + avail {
                st.scroll = r - avail;
            }
        }
    }
    st.scroll = st.scroll.clamp(0.0, max_scroll);
    st.last_active = active;

    // Where each tab goes, with the dragged one lifted out and its slot opened at the pointer.
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let dragged = st.dragging.filter(|(id, _)| items.iter().any(|t| t.id == *id));
    let mut order: Vec<usize> = (0..items.len()).collect();
    let mut drag_target = None;
    if let (Some((id, grab)), Some(pos)) = (dragged, pointer)
        && let Some(from) = items.iter().position(|t| t.id == id)
    {
        let centre = pos.x - grab - rect.left() + st.scroll + w / 2.0;
        let to = drop_index(centre, w, items.len());
        let moved = order.remove(from);
        order.insert(to, moved);
        drag_target = Some((id, to, pos.x - grab));
    }
    let y = rect.center().y - th / 2.0;
    let painter = ui.painter().with_clip_rect(tabs_area);
    let dur = motion::dur(motion::MOVE, reduce);
    let font = text::tab();

    for (slot, &i) in order.iter().enumerate() {
        let Some(tab) = items.get(i) else { continue };
        let target_x = rect.left() + slot as f32 * (w + size::TAB_GAP) - st.scroll;
        let is_dragged = drag_target.is_some_and(|(id, _, _)| id == tab.id);
        let x = match drag_target {
            Some((id, _, px)) if id == tab.id => px.clamp(rect.left() - w / 2.0, rect.left() + avail - w / 2.0),
            _ => ctx.animate_value_with_time(Id::unique(("cw_tab_x", tab.id)), target_x, dur),
        };
        let r = Rect::from_min_size(pos2(x, y), vec2(w, th));
        let visible = r.intersect(tabs_area);
        if visible.width() <= 0.0 {
            continue;
        }
        let resp = ui.interact(visible, Id::unique(("cw_tab", tab.id)), Sense::click_and_drag());
        let is_active = Some(tab.id) == active;
        let hovered = resp.hovered() || is_dragged;

        // Close button (or dirty dot / spinner) in the slot at the right.
        let slot_r = Rect::from_center_size(pos2(r.right() - 16.0, r.center().y), vec2(18.0, 18.0));
        let close = ui.interact(slot_r.intersect(tabs_area), Id::unique(("cw_tab_close", tab.id)), Sense::click());

        if resp.drag_started()
            && let Some(pos) = pointer
        {
            st.dragging = Some((tab.id, pos.x - r.left()));
        }
        if resp.drag_stopped()
            && let Some((id, to, _)) = drag_target
        {
            if items.iter().position(|t| t.id == id) != Some(to) {
                actions.push(TabAction::Move { id, to });
            }
            st.dragging = None;
        }
        if close.clicked() || resp.middle_clicked() {
            actions.push(TabAction::Close(tab.id));
        } else if resp.clicked() || (resp.drag_started() && !is_active) {
            actions.push(TabAction::Activate(tab.id));
        }
        resp.context_menu(|ui| {
            for (label, a) in [
                ("Close tab", TabAction::Close(tab.id)),
                ("Close other tabs", TabAction::CloseOthers(tab.id)),
                ("Close tabs to the right", TabAction::CloseToRight(tab.id)),
            ] {
                if ui.button(label).clicked() {
                    actions.push(a);
                    ui.close();
                }
            }
            ui.separator();
            let has_path = tab.path.is_some();
            if ui.add_enabled(has_path, egui::Button::new("Reveal in Finder")).clicked() {
                actions.push(TabAction::Reveal(tab.id));
                ui.close();
            }
            if ui.add_enabled(has_path, egui::Button::new("Copy path")).clicked() {
                actions.push(TabAction::CopyPath(tab.id));
                ui.close();
            }
        });
        let tip = match &tab.path {
            Some(path) => format!("{}\n{}{}", path.display(), tab.kind.label(), if tab.dirty { " · unsaved changes" } else { "" }),
            None => format!("{}{}", tab.kind.label(), if tab.dirty { " · unsaved changes" } else { " · not saved yet" }),
        };
        let resp = resp.on_hover_text(tip);
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, is_active, &tab.label));

        // Paint.
        let fill = if is_active {
            p.surface_raised
        } else {
            let t = ctx.animate_bool_with_time(Id::unique(("cw_tab_hover", tab.id)), hovered, motion::dur(motion::HOVER, reduce));
            Palette::tint(p.surface_hover, t)
        };
        let pr = if is_dragged { ui.painter().clone() } else { painter.clone() };
        if is_dragged {
            pr.rect_filled(r.translate(vec2(0.0, 2.0)), CornerRadius::same(radius::TAB), Palette::tint(Color32::BLACK, 0.12));
        }
        pr.rect_filled(r, CornerRadius::same(radius::TAB), fill);
        if is_active {
            let bar = Rect::from_min_max(pos2(r.left() + 8.0, r.bottom() - size::TAB_ACCENT), pos2(r.right() - 8.0, r.bottom()));
            pr.rect_filled(bar, CornerRadius::same(1), p.kind(tab.kind));
        }
        let icon_r = Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(size::ICON, size::ICON));
        icons::kind(&pr, icon_r, tab.kind, p.kind(tab.kind), if is_active { p.text } else { p.text_muted });
        let text_left = icon_r.right() + 7.0;
        let text_w = (slot_r.left() - 4.0 - text_left).max(0.0);
        let color = if is_active { p.text } else { p.text_muted };
        let label = truncate_middle(&tab.label, text_w, |s| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), color).size().x));
        pr.text(pos2(text_left, r.center().y), egui::Align2::LEFT_CENTER, label, font.clone(), color);
        if tab.busy {
            let c = slot_r.center();
            let t = ctx.input(|i| i.time) as f32 * 6.0;
            pr.circle_stroke(c, 5.0, Stroke::new(1.5, Palette::tint(p.text_muted, 0.25)));
            let a = pos2(c.x + 5.0 * t.cos(), c.y + 5.0 * t.sin());
            pr.line_segment([c, a], Stroke::new(1.5, p.text_muted));
            ctx.request_repaint();
        } else if hovered || is_active {
            if close.hovered() {
                pr.rect_filled(slot_r, CornerRadius::same(radius::CONTROL), p.surface_hover);
            }
            icons::close(&pr, slot_r.shrink(3.0), if close.hovered() { p.text } else { p.text_muted });
        } else if tab.dirty {
            pr.circle_filled(slot_r.center(), 3.5, p.text_muted);
        }
        if tab.dirty && (hovered || is_active) && !close.hovered() {
            // Unsaved and showing ×: a small dot beside the title keeps the state visible.
            pr.circle_filled(pos2(slot_r.left() - 6.0, r.center().y), 2.5, p.text_muted);
        }
    }
    if st.dragging.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        st.dragging = None;
    }

    // Edge fades while scrolled.
    if overflow {
        let fade = |left: bool| {
            for k in 0..8 {
                let a = 1.0 - k as f32 / 8.0;
                let step = size::EDGE_FADE / 8.0;
                let x = if left { tabs_area.left() + k as f32 * step } else { tabs_area.right() - (k + 1) as f32 * step };
                ui.painter().rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(step, rect.height())), 0.0, Palette::tint(p.surface, a));
            }
        };
        if st.scroll > 0.5 {
            fade(true);
        }
        if st.scroll < max_scroll - 0.5 {
            fade(false);
        }
    }

    // Files dragged from Finder: a caret where they'll land, and a drop opens them there.
    let (hovering_files, dropped) =
        ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>()));
    let over = pointer.is_some_and(|pt| rect.contains(pt));
    let at = pointer.map_or(items.len(), |pt| drop_index(pt.x - rect.left() + st.scroll, w, items.len() + 1));
    if hovering_files && over {
        let x = rect.left() + at as f32 * (w + size::TAB_GAP) - st.scroll - size::TAB_GAP / 2.0;
        ui.painter().line_segment([pos2(x, y + 4.0), pos2(x, y + th - 4.0)], Stroke::new(2.0, p.accent));
    }
    if !dropped.is_empty() && over {
        actions.push(TabAction::Drop { at, paths: dropped });
    }

    // "+" after the last tab (pinned at the end while the strip scrolls).
    let plus_x = (rect.left() + total - st.scroll).clamp(rect.left(), rect.left() + avail) + 4.0;
    let plus = Rect::from_min_size(pos2(plus_x, y), vec2(plus_w, th));
    let pr = ui.interact(plus, Id::unique("cw_plus"), Sense::click()).on_hover_text("New tab (⌘T)");
    if pr.hovered() {
        ui.painter().rect_filled(plus, CornerRadius::same(radius::TAB), p.surface_hover);
    }
    icons::plus(ui.painter(), plus.shrink(6.0), if pr.hovered() { p.text } else { p.text_muted });
    pr.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "New tab"));
    if pr.clicked() {
        actions.push(TabAction::NewTab);
    }
    st.plus_rect = Some(plus);
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_shrink_to_fit_then_overflow() {
        assert_eq!(layout(2, 1000.0), (size::TAB_MAX, false));
        let (w, o) = layout(5, 800.0);
        assert!((w - (160.0 - size::TAB_GAP)).abs() < 0.01 && !o);
        let (w, o) = layout(20, 800.0);
        assert_eq!(w, size::TAB_MIN);
        assert!(o);
        assert_eq!(layout(0, 800.0), (size::TAB_MAX, false));
    }

    #[test]
    fn a_dragged_tab_lands_in_the_slot_under_its_centre() {
        let w = 150.0;
        assert_eq!(drop_index(10.0, w, 4), 0);
        assert_eq!(drop_index(w + 10.0, w, 4), 1);
        assert_eq!(drop_index(10_000.0, w, 4), 3);
        assert_eq!(drop_index(-50.0, w, 4), 0);
        assert_eq!(drop_index(f32::NAN, w, 4), 0);
        assert_eq!(drop_index(10.0, w, 0), 0);
    }

    #[test]
    fn long_names_lose_their_middle_and_keep_their_extension() {
        let width = |s: &str| s.chars().count() as f32 * 7.0;
        assert_eq!(truncate_middle("Budget.xlsx", 200.0, width), "Budget.xlsx");
        let t = truncate_middle("Quarterly report final version.docx", 100.0, width);
        assert!(t.contains('…') && t.ends_with("docx"), "{t}");
        assert!(width(&t) <= 100.0);
        assert_eq!(truncate_middle("abc", 0.0, width), "…");
    }

    /// With nothing happening, the strip settles and stops asking for frames, with few tabs
    /// or with so many that they scroll.
    #[test]
    fn an_idle_strip_stops_repainting() {
        for n in [3u64, 16] {
            let ctx = egui::Context::default();
            let items: Vec<TabItem> =
                (1..=n).map(|i| TabItem { id: i, kind: Kind::Sheet, label: format!("Book {i}"), path: None, dirty: false, busy: false }).collect();
            let mut st = TabStripState::default();
            let rect = Rect::from_min_size(pos2(80.0, 0.0), vec2(1300.0, 32.0));
            let mut t = 0.0;
            let mut last_requested = true;
            for _ in 0..120 {
                t += 1.0 / 60.0;
                let mut out = ctx.run_ui(egui::RawInput { time: Some(t), ..Default::default() }, |ui| {
                    let _ = tab_strip(ui, rect, &Palette::LIGHT, &items, Some(n), &mut st);
                });
                out.textures_delta.clear();
                last_requested =
                    out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay < std::time::Duration::from_secs(1));
            }
            assert!(!last_requested, "{n} tabs: still asking for frames after 2 s idle");
        }
    }

    #[test]
    fn the_strip_reports_clicks_on_tabs_and_plus() {
        let ctx = egui::Context::default();
        let items: Vec<TabItem> =
            (1..=3).map(|i| TabItem { id: i, kind: Kind::Document, label: format!("Doc {i}"), path: None, dirty: i == 2, busy: false }).collect();
        let mut st = TabStripState::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 38.0));
        let run = |events: Vec<egui::Event>, st: &mut TabStripState| {
            let mut got = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                got = tab_strip(ui, rect, &Palette::LIGHT, &items, Some(1), st);
            });
            out.textures_delta.clear();
            got
        };
        let _ = run(vec![], &mut st);
        let click = |pos: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
            ]
        };
        // The second tab starts at TAB_MAX + gap.
        let second = pos2(size::TAB_MAX + size::TAB_GAP + 40.0, 19.0);
        assert_eq!(run(click(second), &mut st), vec![TabAction::Activate(2)]);
        let plus = st.plus_rect.map(|r| r.center()).unwrap_or_default();
        assert_eq!(run(click(plus), &mut st), vec![TabAction::NewTab]);
    }
}
