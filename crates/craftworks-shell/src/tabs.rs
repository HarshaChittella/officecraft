//! The tab strip (docs/design.md "Tab strip"): tabs that shrink to fit then scroll, animated
//! drag-to-reorder, a dirty dot that turns into × on hover, a context menu, file drops, the
//! "+" button, and tab groups (one per open project, as Chrome groups tabs): a coloured label
//! before each group's tabs, a line of its colour under them, and a click on the label to
//! collapse it. It reports what the user did as [`TabAction`]s; the app acts on them.

use crate::tokens::{GROUP_NAMES, Palette, motion, radius, size, text};
use crate::{icons, widgets};
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
    pub group: Option<u64>,
}

/// One tab group as the strip shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupItem {
    pub id: u64,
    pub name: String,
    pub color: u8,
    pub collapsed: bool,
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
    Move {
        id: u64,
        to: usize,
    },
    NewTab,
    /// A blank file of this kind straight away (the "+" button's long-press menu).
    NewBlank(Kind),
    Drop {
        at: usize,
        paths: Vec<PathBuf>,
    },
    DragWindow,
    ZoomWindow,
    /// A group's label was clicked: collapse or expand it.
    ToggleGroup(u64),
    RenameGroup {
        id: u64,
        name: String,
    },
    SetGroupColor {
        id: u64,
        color: u8,
    },
    /// Put the tab's file in a project: the one at `root`, or a new one the user is asked
    /// for (`None`).
    AddToProject {
        id: u64,
        root: Option<PathBuf>,
    },
    /// Show the group's project in the sidebar.
    ShowGroup(u64),
    /// Close the group's tabs and its project.
    CloseGroup(u64),
}

/// A project a tab can be added to, as its menu lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectChoice {
    pub name: String,
    pub root: PathBuf,
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
    /// A group being renamed from its menu, and the name typed so far.
    renaming: Option<(u64, String)>,
    /// When the pointer went down on "+", while it is still held there.
    plus_held_since: Option<f64>,
    /// The quick menu under "+" is showing (a long press, or a right-click).
    plus_menu: bool,
    /// The menu came up under a held pointer: letting go must not close it again.
    plus_menu_fresh: bool,
    /// The projects a tab's menu offers under "Add to Project" (the app keeps this current).
    pub projects: Vec<ProjectChoice>,
    /// When each tab first showed (seconds of egui time), for its entrance.
    born: std::collections::HashMap<u64, f64>,
    /// The strip has drawn once: tabs that arrive from now on animate in.
    primed: bool,
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

/// Something in the strip, left to right.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    /// A group's label (an index into the groups).
    Label(usize),
    /// A tab (an index into the items).
    Tab(usize),
}

/// The strip's slots: each group's label before its first tab, tabs of collapsed groups left
/// out, and labels of groups with no tabs at the end. `order` lists item indices left to right.
fn slots(items: &[TabItem], groups: &[GroupItem], order: &[usize]) -> Vec<Slot> {
    let mut out = Vec::with_capacity(order.len() + groups.len());
    let mut labelled = vec![false; groups.len()];
    for &i in order {
        let Some(t) = items.get(i) else { continue };
        let g = t.group.and_then(|g| groups.iter().position(|x| x.id == g));
        if let Some(gi) = g {
            if !labelled[gi] {
                labelled[gi] = true;
                out.push(Slot::Label(gi));
            }
            if groups[gi].collapsed {
                continue;
            }
        }
        out.push(Slot::Tab(i));
    }
    for (gi, done) in labelled.iter().enumerate() {
        if !done {
            out.push(Slot::Label(gi));
        }
    }
    out
}

/// A group label's text: its name, and how many tabs it hides while collapsed.
fn label_text(g: &GroupItem, count: usize) -> String {
    if g.collapsed && count > 0 { format!("{}  {count}", g.name) } else { g.name.clone() }
}

/// Draws the strip in `rect` and returns what the user did.
pub fn tab_strip(
    ui: &mut Ui,
    rect: Rect,
    p: &Palette,
    items: &[TabItem],
    groups: &[GroupItem],
    active: Option<u64>,
    st: &mut TabStripState,
) -> Vec<TabAction> {
    let mut actions = Vec::new();
    let ctx = ui.ctx().clone();
    // Tabs fill the row less a small margin (the row is sized around the traffic lights).
    // Tabs stand on the row's bottom edge, so the active one runs into the app below it.
    let th = (rect.height() - 5.0).max(22.0);
    let now = ctx.input(|i| i.time);
    // Tabs already there when the strip first draws (a restored session) don't make an entrance.
    let primed = st.primed;
    st.born.retain(|id, _| items.iter().any(|t| t.id == *id));
    for t in items {
        st.born.entry(t.id).or_insert(if primed { now } else { now - 60.0 });
    }
    st.primed = true;
    let plus_w = 26.0;
    let avail = (rect.width() - plus_w - 8.0).max(0.0);
    let font = text::tab();
    let label_font = egui::FontId::new(text::meta().size, egui::FontFamily::Name(crate::tokens::SEMIBOLD.into()));

    // Label widths, then the tabs share what's left.
    let counts: Vec<usize> = groups.iter().map(|g| items.iter().filter(|t| t.group == Some(g.id)).count()).collect();
    let label_w: Vec<f32> = groups
        .iter()
        .zip(&counts)
        .map(|(g, n)| {
            let w = ui.fonts_mut(|f| f.layout_no_wrap(label_text(g, *n), label_font.clone(), Color32::WHITE).size().x);
            (w + 22.0).clamp(28.0, 180.0)
        })
        .collect();
    let base: Vec<usize> = (0..items.len()).collect();
    let base_slots = slots(items, groups, &base);
    let n_tabs = base_slots.iter().filter(|s| matches!(s, Slot::Tab(_))).count();
    let labels_total: f32 = base_slots.iter().map(|s| if let Slot::Label(g) = s { label_w[*g] + size::TAB_GAP + 4.0 } else { 0.0 }).sum();
    let (w, _) = layout(n_tabs, (avail - labels_total).max(0.0));
    let slot_w = |s: &Slot| match s {
        Slot::Label(g) => label_w[*g] + 4.0,
        Slot::Tab(_) => w,
    };
    // Left edges (strip coordinates, before scrolling).
    let lefts = |ss: &[Slot]| -> Vec<f32> {
        let mut x = 0.0;
        ss.iter()
            .map(|s| {
                let l = x;
                x += slot_w(s) + size::TAB_GAP;
                l
            })
            .collect()
    };
    let base_lefts = lefts(&base_slots);
    let total = base_slots.iter().map(|s| slot_w(s) + size::TAB_GAP).sum::<f32>();
    let overflow = total > avail;
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
            && let Some(k) = base_slots.iter().position(|s| matches!(s, Slot::Tab(i) if Some(items[*i].id) == active))
        {
            let (l, r) = (base_lefts[k], base_lefts[k] + w);
            if l < st.scroll {
                st.scroll = l;
            } else if r > st.scroll + avail {
                st.scroll = r - avail;
            }
        }
    }
    st.scroll = st.scroll.clamp(0.0, max_scroll);
    st.last_active = active;

    // The shown tabs' centres, for working out where a dragged tab or dropped file lands.
    let shown_tabs: Vec<(usize, f32)> =
        base_slots.iter().zip(&base_lefts).filter_map(|(s, l)| if let Slot::Tab(i) = s { Some((*i, *l + w / 2.0)) } else { None }).collect();
    // The item index a tab whose centre is at `x` (strip coordinates) goes before.
    let land = |x: f32, skip: Option<u64>| -> usize {
        let rest: Vec<usize> = (0..items.len()).filter(|i| Some(items[*i].id) != skip).collect();
        let before = shown_tabs.iter().filter(|(i, _)| Some(items[*i].id) != skip).find(|(_, c)| *c > x).map(|(i, _)| *i);
        before.and_then(|b| rest.iter().position(|r| *r == b)).unwrap_or(rest.len())
    };

    // The dragged tab is lifted out and the others make room where it would land.
    let pointer = ctx.input(|i| i.pointer.interact_pos());
    let dragged = st.dragging.filter(|(id, _)| items.iter().any(|t| t.id == *id));
    let mut order: Vec<usize> = (0..items.len()).collect();
    let mut drag_target = None;
    if let (Some((id, grab)), Some(pos)) = (dragged, pointer)
        && let Some(from) = items.iter().position(|t| t.id == id)
    {
        let centre = pos.x - grab - rect.left() + st.scroll + w / 2.0;
        let to = land(centre, Some(id));
        let moved = order.remove(from);
        order.insert(to.min(order.len()), moved);
        drag_target = Some((id, to, pos.x - grab));
    }
    let cur_slots = slots(items, groups, &order);
    let cur_lefts = lefts(&cur_slots);
    let y = rect.bottom() - th;
    let painter = ui.painter().with_clip_rect(tabs_area);

    // Each group's span (first label left, last tab right), for its underline.
    let mut spans: Vec<Option<(f32, f32)>> = vec![None; groups.len()];

    for (k, slot) in cur_slots.iter().enumerate() {
        let target_x = rect.left() + cur_lefts[k] - st.scroll;
        match *slot {
            Slot::Label(gi) => {
                let g = &groups[gi];
                let x = motion::glide(&ctx, Id::unique(("cw_group_x", g.id)), target_x, motion::MOVE);
                let r = Rect::from_min_size(pos2(x, y + 3.0), vec2(label_w[gi], th - 8.0));
                spans[gi] = Some((r.left(), r.right()));
                let visible = r.intersect(tabs_area);
                if visible.width() <= 0.0 {
                    continue;
                }
                let resp = ui.interact(visible, Id::unique(("cw_group", g.id)), Sense::click());
                let (fill, ink) = p.group(g.color);
                let (h, down) = widgets::hover(ui, &resp);
                let fill = Palette::mix(fill, ink, 0.10 * h + if down { 0.08 } else { 0.0 });
                painter.rect_filled(r, CornerRadius::same(255), fill);
                if resp.has_focus() {
                    widgets::focus_ring(&painter, r, 10, p);
                }
                let galley = ui.fonts_mut(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(label_text(g, counts[gi]), label_font.clone(), ink);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(r.width() - 14.0);
                    f.layout_job(job)
                });
                painter.galley(pos2(r.center().x - galley.size().x / 2.0, r.center().y - galley.size().y / 2.0), galley, ink);
                if resp.clicked() {
                    actions.push(TabAction::ToggleGroup(g.id));
                }
                let tip = if g.collapsed { "Show this project's tabs" } else { "Hide this project's tabs" };
                let resp = resp.on_hover_text(format!("{}\n{tip}", g.name));
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Project {}{}", g.name, if g.collapsed { ", collapsed" } else { "" }))
                });
                group_menu(&resp, g, st, &mut actions);
            }
            Slot::Tab(i) => {
                let tab = &items[i];
                let is_dragged = drag_target.is_some_and(|(id, _, _)| id == tab.id);
                let x = match drag_target {
                    Some((id, _, px)) if id == tab.id => px.clamp(rect.left() - w / 2.0, rect.left() + avail - w / 2.0),
                    _ => motion::glide(&ctx, Id::unique(("cw_tab_x", tab.id)), target_x, motion::MOVE),
                };
                let r = Rect::from_min_size(pos2(x, y), vec2(w, th));
                if let Some(gi) = tab.group.and_then(|g| groups.iter().position(|x| x.id == g))
                    && !is_dragged
                {
                    let s = spans[gi].get_or_insert((r.left(), r.right()));
                    s.1 = s.1.max(r.right());
                }
                // A hairline parts two resting tabs; it steps aside for the active one.
                let next_active = matches!(cur_slots.get(k + 1), Some(Slot::Tab(j)) if Some(items[*j].id) == active);
                let sep = !is_dragged && !next_active && matches!(cur_slots.get(k + 1), Some(Slot::Tab(_)));
                draw_tab(ui, &painter, p, tab, r, tabs_area, active, is_dragged, sep, pointer, drag_target, items, st, &mut actions, &font);
            }
        }
    }
    // Each group's line under its tabs.
    for (gi, span) in spans.iter().enumerate() {
        if let Some((l, r)) = span
            && !groups[gi].collapsed
            && counts[gi] > 0
        {
            let (fill, _) = p.group(groups[gi].color);
            let line = Rect::from_min_max(pos2(*l + 3.0, rect.bottom() - 2.0), pos2(*r - 3.0, rect.bottom()));
            painter.rect_filled(line, CornerRadius::same(1), fill);
        }
    }
    if st.dragging.is_some() && !ctx.input(|i| i.pointer.any_down()) {
        st.dragging = None;
    }

    // Edge fades while scrolled: the strip dissolves into the title row, eased in as it scrolls.
    if overflow {
        let fade = |left: bool, strength: f32| {
            if strength <= 0.0 {
                return;
            }
            let (solid, clear) = (Palette::tint(p.band, strength), Palette::tint(p.band, 0.0));
            let (x0, x1) = if left { (tabs_area.left(), tabs_area.left() + size::EDGE_FADE) } else { (tabs_area.right(), tabs_area.right() - size::EDGE_FADE) };
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(pos2(x0, rect.top()), solid);
            mesh.colored_vertex(pos2(x1, rect.top()), clear);
            mesh.colored_vertex(pos2(x1, rect.bottom() - 1.0), clear);
            mesh.colored_vertex(pos2(x0, rect.bottom() - 1.0), solid);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(mesh);
        };
        fade(true, (st.scroll / size::EDGE_FADE).clamp(0.0, 1.0));
        fade(false, ((max_scroll - st.scroll) / size::EDGE_FADE).clamp(0.0, 1.0));
    }

    // Files dragged from Finder: a caret where they'll land, and a drop opens them there.
    let (hovering_files, dropped) =
        ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect::<Vec<_>>()));
    let over = pointer.is_some_and(|pt| rect.contains(pt));
    let at = pointer.map_or(items.len(), |pt| land(pt.x - rect.left() + st.scroll, None));
    if hovering_files && over {
        let x = shown_tabs.iter().find(|(i, _)| *i >= at).map_or(total, |(_, c)| c - w / 2.0);
        let x = rect.left() + x - st.scroll - size::TAB_GAP / 2.0;
        ui.painter().rect_filled(Rect::from_min_max(pos2(x - 1.0, y + 3.0), pos2(x + 1.0, y + th - 3.0)), CornerRadius::same(1), p.accent);
    }
    if !dropped.is_empty() && over {
        actions.push(TabAction::Drop { at, paths: dropped });
    }

    // "+" after the last tab (pinned at the end while the strip scrolls).
    let plus_x = motion::glide(&ctx, Id::unique("cw_plus_x"), (rect.left() + total - st.scroll).clamp(rect.left(), rect.left() + avail) + 4.0, motion::MOVE);
    let plus = Rect::from_center_size(pos2(plus_x + plus_w / 2.0 + 2.0, y + th / 2.0 - 1.0), vec2(plus_w, plus_w));
    let pr = widgets::icon_button(ui, p, plus, Id::unique("cw_plus"), "New tab", st.plus_menu, icons::plus);
    // A click opens the new tab. Holding "+" (or right-clicking it) drops a quick menu of
    // blank files instead, as a browser's toolbar buttons do.
    if pr.is_pointer_button_down_on() && ctx.input(|i| i.pointer.primary_down()) {
        let since = *st.plus_held_since.get_or_insert(now);
        if now - since >= LONG_PRESS && !st.plus_menu {
            st.plus_menu = true;
            st.plus_menu_fresh = true;
        } else if !st.plus_menu {
            ctx.request_repaint();
        }
    } else {
        st.plus_held_since = None;
    }
    if pr.secondary_clicked() {
        st.plus_menu = true;
    }
    if pr.clicked() && !st.plus_menu {
        actions.push(TabAction::NewTab);
    }
    if st.plus_menu {
        let mut open = true;
        let mut chosen = None;
        egui::Popup::from_response(&pr).open_bool(&mut open).gap(6.0).show(|ui| {
            ui.set_min_width(210.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            for (kind, label) in QUICK_NEW {
                if quick_row(ui, p, kind, label).clicked() {
                    chosen = Some(kind);
                }
            }
        });
        // Letting go of the press that opened the menu counts as a click outside it: keep it.
        let released = ctx.input(|i| i.pointer.any_released());
        if st.plus_menu_fresh && (released || !open) {
            open = true;
            st.plus_menu_fresh = !released;
        }
        if let Some(kind) = chosen {
            actions.push(TabAction::NewBlank(kind));
            open = false;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        st.plus_menu = open;
        if !open {
            st.plus_menu_fresh = false;
        }
    } else if !pr.is_pointer_button_down_on() {
        let _ = pr.clone().on_hover_text("New tab (⌘T)\nHold for a blank document, sheet, presentation or PDF");
    }
    st.plus_rect = Some(plus);
    actions
}

/// How long "+" is held before its quick menu drops, in seconds.
pub const LONG_PRESS: f64 = 0.4;

/// The quick menu under "+": a blank file of each kind.
const QUICK_NEW: [(Kind, &str); 4] =
    [(Kind::Document, "New Document"), (Kind::Sheet, "New Sheet"), (Kind::Presentation, "New Presentation"), (Kind::Pdf, "New PDF")];

/// One line of the quick menu: the kind's glass icon and its name.
fn quick_row(ui: &mut Ui, p: &Palette, kind: Kind, label: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(210.0), 32.0), Sense::click());
    widgets::hover_fill(ui, ui.painter(), p, rect, radius::CONTROL, &resp);
    icons::kind(ui.painter(), Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(20.0, 20.0)), kind, Color32::WHITE);
    ui.painter().text(pos2(rect.left() + 38.0, rect.center().y), egui::Align2::LEFT_CENTER, label, text::body(), p.text);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp
}

/// The Home tab, pinned before the others: the app's mark. It looks and behaves as a tab does
/// (chosen, it is a piece of the page below), and it never closes.
pub fn home_tab(ui: &mut Ui, p: &Palette, r: Rect, selected: bool) -> egui::Response {
    let resp = ui.interact(r, Id::unique("cw_home_tab"), Sense::click());
    let (h, _) = widgets::hover(ui, &resp);
    let sel = motion::fade(ui.ctx(), Id::unique("cw_home_tab_sel"), selected, motion::PANEL);
    let painter = ui.painter();
    let top = CornerRadius { nw: radius::TAB, ne: radius::TAB, sw: 0, se: 0 };
    if h > 0.0 && sel < 1.0 {
        let pill = Rect::from_min_max(pos2(r.left() + 3.0, r.top() + 1.0), pos2(r.right() - 3.0, r.bottom() - 4.0));
        painter.rect_filled(pill, CornerRadius::same(radius::CONTROL), Palette::tint(Palette::mix(p.band, p.surface, 0.6), h * (1.0 - sel)));
    }
    if sel > 0.0 {
        painter.rect_filled(r, top, Palette::tint(p.surface, sel));
        let outline = Rect::from_min_max(r.min, pos2(r.right(), r.bottom() + 4.0));
        painter.with_clip_rect(r).rect_stroke(outline, top, Stroke::new(1.0, Palette::tint(p.border, sel)), egui::StrokeKind::Inside);
    }
    if resp.has_focus() {
        widgets::focus_ring(painter, r.shrink(3.0), radius::CONTROL, p);
    }
    icons::mark(painter, Rect::from_center_size(pos2(r.center().x, r.center().y - 1.0), vec2(20.0, 20.0)), p);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, selected, "Home"));
    resp.on_hover_text("Home: new files and recent ones")
}

/// A group label's menu: rename, colour, collapse, show in the sidebar, close.
fn group_menu(resp: &egui::Response, g: &GroupItem, st: &mut TabStripState, actions: &mut Vec<TabAction>) {
    resp.context_menu(|ui| {
        ui.set_min_width(200.0);
        let name = match &mut st.renaming {
            Some((id, n)) if *id == g.id => n,
            _ => {
                st.renaming = Some((g.id, g.name.clone()));
                match &mut st.renaming {
                    Some((_, n)) => n,
                    None => return,
                }
            }
        };
        let field = ui.add(egui::TextEdit::singleline(name).hint_text("Project name").desired_width(f32::INFINITY));
        let commit = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if commit && !name.trim().is_empty() && name.trim() != g.name {
            actions.push(TabAction::RenameGroup { id: g.id, name: name.trim().to_string() });
            ui.close();
        }
        ui.horizontal_wrapped(|ui| {
            for (i, cname) in GROUP_NAMES.iter().enumerate() {
                let i = i as u8;
                let (fill, _) = crate::tokens::Palette::for_dark(ui.visuals().dark_mode).group(i);
                let (r, c) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                ui.painter().circle_filled(r.center(), 7.0, fill);
                if i == g.color {
                    ui.painter().circle_stroke(r.center(), 8.5, Stroke::new(1.5, ui.visuals().text_color()));
                }
                let c = c.on_hover_text(*cname);
                c.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, i == g.color, *cname));
                if c.clicked() {
                    actions.push(TabAction::SetGroupColor { id: g.id, color: i });
                    ui.close();
                }
            }
        });
        ui.separator();
        if ui.button(if g.collapsed { "Expand" } else { "Collapse" }).clicked() {
            actions.push(TabAction::ToggleGroup(g.id));
            ui.close();
        }
        if ui.button("Show in Sidebar").clicked() {
            actions.push(TabAction::ShowGroup(g.id));
            ui.close();
        }
        ui.separator();
        if ui.button("Close Project").clicked() {
            actions.push(TabAction::CloseGroup(g.id));
            ui.close();
        }
    });
    if !resp.context_menu_opened() && st.renaming.as_ref().is_some_and(|(id, _)| *id == g.id) {
        st.renaming = None;
    }
}

/// One tab: its interactions and paint.
#[allow(clippy::too_many_arguments)]
fn draw_tab(
    ui: &mut Ui,
    painter: &egui::Painter,
    p: &Palette,
    tab: &TabItem,
    r: Rect,
    tabs_area: Rect,
    active: Option<u64>,
    is_dragged: bool,
    sep: bool,
    pointer: Option<egui::Pos2>,
    drag_target: Option<(u64, usize, f32)>,
    items: &[TabItem],
    st: &mut TabStripState,
    actions: &mut Vec<TabAction>,
    font: &egui::FontId,
) {
    let ctx = ui.ctx().clone();
    let visible = r.intersect(tabs_area);
    if visible.width() <= 0.0 {
        return;
    }
    let resp = ui.interact(visible, Id::unique(("cw_tab", tab.id)), Sense::click_and_drag());
    let is_active = Some(tab.id) == active;
    let hovered = resp.hovered() || is_dragged;

    // Close button (or dirty dot / spinner) in the slot at the right.
    let slot_r = Rect::from_center_size(pos2(r.right() - 17.0, r.center().y - 1.0), vec2(18.0, 18.0));
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
        // Add to Project ▸ New Project…, then every project. A project is a folder, so adding
        // a file puts a copy of it there; the one the file is already in is ticked.
        ui.menu_button("Add to Project", |ui| {
            ui.set_min_width(200.0);
            if ui.button("New Project…").clicked() {
                actions.push(TabAction::AddToProject { id: tab.id, root: None });
                ui.close();
            }
            if !st.projects.is_empty() {
                ui.separator();
            }
            for choice in &st.projects {
                let inside = tab.path.as_deref().is_some_and(|f| f.starts_with(&choice.root));
                let label = if inside { format!("✓ {}", choice.name) } else { choice.name.clone() };
                let item = ui.add_enabled(!inside, egui::Button::new(label)).on_hover_text(choice.root.display().to_string());
                if item.clicked() {
                    actions.push(TabAction::AddToProject { id: tab.id, root: Some(choice.root.clone()) });
                    ui.close();
                }
            }
        });
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

    // Paint. Three eased states drive everything: selected, hovered, and just arrived.
    let sel = motion::fade(&ctx, Id::unique(("cw_tab_active", tab.id)), is_active, motion::PANEL);
    let hov = motion::fade(&ctx, Id::unique(("cw_tab_hover", tab.id)), hovered, motion::HOVER);
    let born = st.born.get(&tab.id).copied().unwrap_or(f64::NEG_INFINITY);
    let arrive = motion::enter(&ctx, born, 0.0, motion::MOVE);
    let mut pr = if is_dragged { ui.painter().clone() } else { painter.clone() };
    if arrive < 1.0 {
        pr.multiply_opacity(arrive);
    }
    // A new tab grows out of its left edge.
    let r = Rect::from_min_size(r.min, vec2(r.width() * (0.72 + 0.28 * arrive), r.height()));
    // The active tab is a piece of the app below, pulled up into the title row: the app's
    // surface colour, rounded at the top, open at the bottom.
    let top = CornerRadius { nw: radius::TAB, ne: radius::TAB, sw: 0, se: 0 };
    if is_dragged {
        widgets::shadow(&pr, r, radius::TAB, p, 1.0);
    }
    if hov > 0.0 && sel < 1.0 {
        let pill = Rect::from_min_max(pos2(r.left() + 3.0, r.top() + 1.0), pos2(r.right() - 3.0, r.bottom() - 4.0));
        pr.rect_filled(pill, CornerRadius::same(radius::CONTROL), Palette::tint(Palette::mix(p.band, p.surface, 0.6), hov * (1.0 - sel)));
    }
    if sel > 0.0 {
        pr.rect_filled(r, top, Palette::tint(p.surface, sel));
        // Its outline runs up the sides and over the top; the open bottom is clipped away.
        let outline = Rect::from_min_max(r.min, pos2(r.right(), r.bottom() + 4.0));
        pr.with_clip_rect(r.intersect(pr.clip_rect())).rect_stroke(outline, top, Stroke::new(1.0, Palette::tint(p.border, sel)), egui::StrokeKind::Inside);
    }
    if sep && sel < 0.5 {
        let x = r.right() - 0.5;
        pr.line_segment([pos2(x, r.center().y - 8.0), pos2(x, r.center().y + 6.0)], Stroke::new(1.0, Palette::tint(p.border, (1.0 - hov) * (1.0 - sel))));
    }
    if resp.has_focus() {
        widgets::focus_ring(&pr, r.shrink(3.0), radius::CONTROL, p);
    }
    let pr = pr.with_clip_rect(r.intersect(pr.clip_rect()));
    let mid = r.center().y - 1.0;
    let icon_r = Rect::from_center_size(pos2(r.left() + 18.0, mid), vec2(size::ICON, size::ICON));
    icons::kind(&pr, icon_r, tab.kind, Color32::WHITE);
    let text_left = icon_r.right() + 8.0;
    let text_w = (slot_r.left() - 4.0 - text_left).max(0.0);
    let color = Palette::mix(p.text_muted, p.text, sel.max(hov * 0.6));
    // The chosen tab's title is set heavier, as the chosen ribbon tab is in the apps.
    let font = &if is_active { egui::FontId::new(font.size, egui::FontFamily::Name(crate::tokens::SEMIBOLD.into())) } else { font.clone() };
    let label = truncate_middle(&tab.label, text_w, |s| ui.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font.clone(), color).size().x));
    pr.text(pos2(text_left, mid), egui::Align2::LEFT_CENTER, label, font.clone(), color);
    if tab.busy {
        icons::spinner(&pr, slot_r.shrink(2.0), ctx.input(|i| i.time), p.text_muted);
        ctx.request_repaint();
    } else if hovered || is_active {
        let (ch, down) = widgets::hover(ui, &close);
        if ch > 0.0 {
            let fill = if down { Palette::mix(p.surface_hover, p.text, 0.16) } else { Palette::mix(p.surface_hover, p.text, 0.08) };
            pr.circle_filled(slot_r.center(), 9.0, Palette::tint(fill, ch));
        }
        icons::close(&pr, slot_r.shrink(3.5), Palette::mix(p.text_muted, p.text, ch));
    } else if tab.dirty {
        pr.circle_filled(slot_r.center(), 3.5, p.kind(tab.kind));
    }
    if tab.dirty && (hovered || is_active) && !close.hovered() {
        // Unsaved and showing ×: a small dot beside the title keeps the state visible.
        pr.circle_filled(pos2(slot_r.left() - 6.0, mid), 2.5, p.kind(tab.kind));
    }
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
    fn group_labels_come_before_their_tabs_and_collapsed_tabs_hide() {
        let t = |id: u64, group: Option<u64>| TabItem { id, kind: Kind::Sheet, label: String::new(), path: None, dirty: false, busy: false, group };
        let items = [t(1, None), t(2, Some(7)), t(3, Some(7)), t(4, None)];
        let g = |id: u64, collapsed: bool| GroupItem { id, name: "P".into(), color: 0, collapsed };
        let order = [0, 1, 2, 3];
        assert_eq!(
            slots(&items, &[g(7, false), g(8, false)], &order),
            [Slot::Tab(0), Slot::Label(0), Slot::Tab(1), Slot::Tab(2), Slot::Tab(3), Slot::Label(1)]
        );
        assert_eq!(slots(&items, &[g(7, true)], &order), [Slot::Tab(0), Slot::Label(0), Slot::Tab(3)]);
        assert_eq!(label_text(&g(7, true), 2), "P  2");
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
            let items: Vec<TabItem> = (1..=n)
                .map(|i| TabItem { id: i, kind: Kind::Sheet, label: format!("Book {i}"), path: None, dirty: false, busy: false, group: None })
                .collect();
            let mut st = TabStripState::default();
            let rect = Rect::from_min_size(pos2(80.0, 0.0), vec2(1300.0, 32.0));
            let mut t = 0.0;
            let mut last_requested = true;
            for _ in 0..120 {
                t += 1.0 / 60.0;
                let mut out = ctx.run_ui(egui::RawInput { time: Some(t), ..Default::default() }, |ui| {
                    let _ = tab_strip(ui, rect, &Palette::LIGHT, &items, &[], Some(n), &mut st);
                });
                out.textures_delta.clear();
                last_requested =
                    out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay < std::time::Duration::from_secs(1));
            }
            assert!(!last_requested, "{n} tabs: still asking for frames after 2 s idle");
        }
    }

    /// Holding "+" drops the quick menu instead of opening a new tab, and letting go leaves
    /// the menu up.
    #[test]
    fn holding_plus_drops_the_quick_menu() {
        let ctx = egui::Context::default();
        let mut st = TabStripState::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 38.0));
        let mut time = 0.0;
        let mut run = |events: Vec<egui::Event>, st: &mut TabStripState, dt: f64| {
            time += dt;
            let mut got = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput { events, time: Some(time), ..Default::default() }, |ui| {
                got = tab_strip(ui, rect, &Palette::LIGHT, &[], &[], None, st);
            });
            out.textures_delta.clear();
            got
        };
        let _ = run(vec![], &mut st, 0.1);
        let pos = st.plus_rect.map(|r| r.center()).unwrap_or_default();
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut all = run(vec![egui::Event::PointerMoved(pos), button(true)], &mut st, 0.1);
        for _ in 0..6 {
            all.extend(run(vec![], &mut st, 0.1));
        }
        assert!(st.plus_menu, "the menu drops after {LONG_PRESS} s");
        all.extend(run(vec![button(false)], &mut st, 0.1));
        all.extend(run(vec![], &mut st, 0.1));
        assert!(st.plus_menu, "letting go keeps it");
        assert!(!all.contains(&TabAction::NewTab), "a long press is not a click: {all:?}");
    }

    #[test]
    fn the_strip_reports_clicks_on_tabs_and_plus() {
        let ctx = egui::Context::default();
        let items: Vec<TabItem> = (1..=3)
            .map(|i| TabItem { id: i, kind: Kind::Document, label: format!("Doc {i}"), path: None, dirty: i == 2, busy: false, group: None })
            .collect();
        let mut st = TabStripState::default();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 38.0));
        let run = |events: Vec<egui::Event>, st: &mut TabStripState| {
            let mut got = Vec::new();
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                got = tab_strip(ui, rect, &Palette::LIGHT, &items, &[], Some(1), st);
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
