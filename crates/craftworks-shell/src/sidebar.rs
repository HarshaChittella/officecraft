//! The project sidebar (docs/design.md "Sidebar"): a project switcher, a search field that
//! searches every file in the project, then Pinned and Files. It draws and reports what the
//! user did; the app does it (opening, creating, renaming, the index).

use crate::{icons, tokens::Palette, tokens::radius, tokens::text};
use craftworks_core::Kind;
use craftworks_core::index::{Hit, Progress};
use craftworks_core::tree::Entry;
use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const WIDTH: f32 = 248.0;
pub const MIN_WIDTH: f32 = 200.0;
pub const MAX_WIDTH: f32 = 400.0;
const ROW: f32 = 26.0;
const INDENT: f32 = 14.0;
/// Search results shown at most.
pub const MAX_HITS: usize = 200;

/// What the user did in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarAction {
    Open(PathBuf),
    OpenHit(Hit),
    NewFile {
        dir: PathBuf,
        kind: Kind,
    },
    Rename {
        from: PathBuf,
        to_name: String,
    },
    Reveal(PathBuf),
    TogglePin(PathBuf),
    SwitchProject(PathBuf),
    NewProject,
    OpenFolder,
    /// The search text changed.
    Search(String),
}

/// A project as the switcher lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectItem {
    pub name: String,
    pub root: PathBuf,
}

/// What the sidebar shows this frame.
pub struct SidebarView<'a> {
    pub project: Option<&'a ProjectItem>,
    pub projects: &'a [ProjectItem],
    pub entries: &'a [Entry],
    pub pinned: &'a [PathBuf],
    /// The file in the tab in front.
    pub active: Option<&'a Path>,
    pub hits: &'a [Hit],
    pub progress: &'a Progress,
    pub read_only: bool,
}

/// The sidebar's own state.
#[derive(Default)]
pub struct Sidebar {
    pub open: bool,
    pub query: String,
    pub show_all: bool,
    collapsed: HashSet<String>,
    renaming: Option<(PathBuf, String)>,
    focus_search: bool,
}

fn search_id() -> Id {
    Id::unique("cw_sidebar_search")
}
fn rename_id() -> Id {
    Id::unique("cw_sidebar_rename")
}

impl Sidebar {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Opens the sidebar with the keyboard in its search field (⌃⇧F).
    pub fn focus_search(&mut self) {
        self.open = true;
        self.focus_search = true;
    }

    /// Whether one of the sidebar's fields has the keyboard.
    pub fn wants_keyboard(&self, ctx: &egui::Context) -> bool {
        self.open && ctx.memory(|m| m.focused().is_some_and(|f| f == search_id() || f == rename_id()))
    }

    /// Draws the sidebar in `ui` (the panel's area).
    pub fn show(&mut self, ui: &mut Ui, p: &Palette, v: &SidebarView<'_>) -> Vec<SidebarAction> {
        let mut out = Vec::new();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, p.surface);
        ui.painter().vline(full.right() - 0.5, full.y_range(), Stroke::new(1.0, p.hairline));
        let inner = full.shrink2(vec2(10.0, 8.0));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        ui.spacing_mut().item_spacing = vec2(0.0, 2.0);

        self.header(&mut ui, p, v, &mut out);
        let Some(project) = v.project else {
            self.empty(&mut ui, p, &mut out);
            return out;
        };
        self.search_field(&mut ui, p, &mut out);
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            if self.query.trim().is_empty() {
                self.files(ui, p, v, &project.root, &mut out);
            } else {
                results(ui, p, v, &mut out);
            }
        });
        out
    }

    fn header(&mut self, ui: &mut Ui, p: &Palette, v: &SidebarView<'_>, out: &mut Vec<SidebarAction>) {
        let name = v.project.map_or("No project", |p| p.name.as_str());
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(radius::CONTROL), p.surface_hover);
        }
        icons::folder(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(16.0, 16.0)), p.text_muted);
        let galley = ui.painter().layout_no_wrap(name.to_string(), text::card(), p.text);
        let max = rect.width() - 52.0;
        ui.painter().with_clip_rect(Rect::from_min_size(pos2(rect.left() + 28.0, rect.top()), vec2(max, rect.height()))).galley(
            pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0),
            galley,
            p.text,
        );
        icons::chevron_down(ui.painter(), Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)), p.text_muted);
        let resp = resp.on_hover_text("Switch project");
        egui::Popup::menu(&resp).show(|ui| {
            ui.set_min_width(220.0);
            for item in v.projects {
                let current = v.project.is_some_and(|c| c.root == item.root);
                if ui.selectable_label(current, &item.name).on_hover_text(item.root.display().to_string()).clicked() && !current {
                    out.push(SidebarAction::SwitchProject(item.root.clone()));
                }
            }
            if !v.projects.is_empty() {
                ui.separator();
            }
            if ui.button("New Project…").clicked() {
                out.push(SidebarAction::NewProject);
            }
            if ui.button("Open Folder as Project…").clicked() {
                out.push(SidebarAction::OpenFolder);
            }
            if v.project.is_some() {
                ui.separator();
                ui.checkbox(&mut self.show_all, "Show all files");
            }
        });
        if v.read_only {
            ui.label(egui::RichText::new("Saved by a newer Craftworks: pins can't change").font(text::meta()).color(p.text_muted));
        }
    }

    fn empty(&mut self, ui: &mut Ui, p: &Palette, out: &mut Vec<SidebarAction>) {
        ui.add_space(24.0);
        ui.label(egui::RichText::new("A project is a folder of documents you work on together.").font(text::body()).color(p.text_muted));
        ui.add_space(12.0);
        if ui.button("New Project…").clicked() {
            out.push(SidebarAction::NewProject);
        }
        if ui.button("Open Folder as Project…").clicked() {
            out.push(SidebarAction::OpenFolder);
        }
    }

    fn search_field(&mut self, ui: &mut Ui, p: &Palette, out: &mut Vec<SidebarAction>) {
        let before = self.query.clone();
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id(search_id())
            .hint_text("Search project  ⌃⇧F")
            .font(text::body())
            .desired_width(f32::INFINITY)
            .margin(vec2(8.0, 5.0));
        let resp = ui.add(edit);
        if std::mem::take(&mut self.focus_search) {
            resp.request_focus();
        }
        if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.query.clear();
            resp.surrender_focus();
        }
        if self.query != before {
            out.push(SidebarAction::Search(self.query.clone()));
        }
        let _ = p;
    }

    fn files(&mut self, ui: &mut Ui, p: &Palette, v: &SidebarView<'_>, root: &Path, out: &mut Vec<SidebarAction>) {
        if !v.pinned.is_empty() {
            section(ui, p, "Pinned");
            for path in v.pinned {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let kind = Kind::for_path(path);
                let r = row(ui, p, 0, kind, &name, v.active == Some(path.as_path()), false, None);
                if r.clicked() {
                    out.push(SidebarAction::Open(path.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Unpin").clicked() {
                        out.push(SidebarAction::TogglePin(path.clone()));
                    }
                    if ui.button("Reveal in Finder").clicked() {
                        out.push(SidebarAction::Reveal(path.clone()));
                    }
                });
            }
            ui.add_space(8.0);
        }
        let files_header = section(ui, p, "Files");
        files_header.context_menu(|ui| new_here(ui, root, out));

        // Folders that hold a document (at any depth) show; others only with "Show all files".
        let mut has_docs: HashSet<&str> = HashSet::new();
        for e in v.entries.iter().filter(|e| e.kind.is_some()) {
            let mut rel = e.rel.as_str();
            while let Some(i) = rel.rfind('/') {
                rel = &rel[..i];
                has_docs.insert(rel);
            }
        }
        let mut hidden_under: Option<String> = None;
        let mut shown = 0;
        for e in v.entries {
            if let Some(h) = &hidden_under {
                if e.rel.starts_with(h.as_str()) {
                    continue;
                }
                hidden_under = None;
            }
            if !self.show_all && (if e.is_dir { !has_docs.contains(e.rel.as_str()) } else { e.kind.is_none() }) {
                if e.is_dir {
                    hidden_under = Some(format!("{}/", e.rel));
                }
                continue;
            }
            shown += 1;
            let name = e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let renaming = self.renaming.as_ref().is_some_and(|(p, _)| *p == e.path);
            if renaming {
                self.rename_row(ui, p, e, out);
                continue;
            }
            let collapsed = e.is_dir && self.collapsed.contains(&e.rel);
            let folder = e.is_dir.then_some(!collapsed);
            let r = row(ui, p, e.depth, e.kind, &name, v.active == Some(e.path.as_path()), !e.is_dir && e.kind.is_none(), folder);
            if r.clicked() {
                if e.is_dir {
                    if collapsed {
                        self.collapsed.remove(&e.rel);
                    } else {
                        self.collapsed.insert(e.rel.clone());
                    }
                } else if e.kind.is_some() {
                    out.push(SidebarAction::Open(e.path.clone()));
                }
            }
            if collapsed {
                hidden_under = Some(format!("{}/", e.rel));
            }
            r.context_menu(|ui| {
                let dir = if e.is_dir { e.path.clone() } else { e.path.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf) };
                new_here(ui, &dir, out);
                ui.separator();
                if ui.button("Rename…").clicked() {
                    self.renaming = Some((e.path.clone(), name.clone()));
                }
                if ui.button("Reveal in Finder").clicked() {
                    out.push(SidebarAction::Reveal(e.path.clone()));
                }
                if !e.is_dir && e.kind.is_some() {
                    let pinned = v.pinned.contains(&e.path);
                    if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                        out.push(SidebarAction::TogglePin(e.path.clone()));
                    }
                }
            });
        }
        if shown == 0 {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("No documents here yet. Right-click Files to make one, or add files to the folder in Finder.")
                    .font(text::meta())
                    .color(p.text_muted),
            );
        }
    }

    fn rename_row(&mut self, ui: &mut Ui, p: &Palette, e: &Entry, out: &mut Vec<SidebarAction>) {
        let Some((path, name)) = self.renaming.as_mut() else { return };
        let mut done = false;
        ui.horizontal(|ui| {
            ui.add_space(e.depth as f32 * INDENT + 24.0);
            let resp = ui.add(egui::TextEdit::singleline(name).id(rename_id()).font(text::body()).desired_width(f32::INFINITY));
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
            if resp.lost_focus() || enter || esc {
                let from = path.clone();
                let to_name = name.trim().to_string();
                let unchanged = from.file_name().is_some_and(|n| n.to_string_lossy() == to_name);
                if !esc && !to_name.is_empty() && !unchanged {
                    out.push(SidebarAction::Rename { from, to_name });
                }
                done = true;
            }
        });
        if done {
            self.renaming = None;
        }
        let _ = p;
    }
}

fn new_here(ui: &mut Ui, dir: &Path, out: &mut Vec<SidebarAction>) {
    for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
        if ui.button(format!("New {} Here", kind.label())).clicked() {
            out.push(SidebarAction::NewFile { dir: dir.to_path_buf(), kind });
        }
    }
}

fn section(ui: &mut Ui, p: &Palette, title: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    ui.painter().text(pos2(rect.left() + 6.0, rect.center().y + 2.0), Align2::LEFT_CENTER, title.to_uppercase(), text::meta(), p.text_muted);
    resp
}

/// One 26 pt row: indent, folder chevron or kind icon, name. `folder` is `Some(expanded)`.
#[allow(clippy::too_many_arguments)]
fn row(ui: &mut Ui, p: &Palette, depth: usize, kind: Option<Kind>, name: &str, selected: bool, greyed: bool, folder: Option<bool>) -> egui::Response {
    let sense = if greyed { Sense::hover() } else { Sense::click() };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), sense);
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, CornerRadius::same(radius::CONTROL), p.surface_raised);
    } else if resp.hovered() && !greyed {
        painter.rect_filled(rect, CornerRadius::same(radius::CONTROL), p.surface_hover);
    }
    let x = rect.left() + 6.0 + depth as f32 * INDENT;
    let icon = Rect::from_min_size(pos2(x, rect.center().y - 8.0), vec2(16.0, 16.0));
    let ink = if greyed { Palette::tint(p.text_muted, 0.5) } else { p.text_muted };
    match (folder, kind) {
        (Some(expanded), _) => {
            let c = Rect::from_center_size(pos2(x + 4.0, rect.center().y), vec2(10.0, 10.0));
            if expanded {
                icons::chevron_down(&painter, c, p.text_muted);
            } else {
                icons::chevron(&painter, c, false, p.text_muted);
            }
            icons::folder(&painter, icon.translate(vec2(12.0, 0.0)), p.text_muted);
        }
        (None, Some(k)) => icons::kind(&painter, icon, k, p.kind(k), ink),
        (None, None) => icons::kind(&painter, icon, Kind::Document, ink, ink),
    }
    let tx = if folder.is_some() { x + 34.0 } else { x + 22.0 };
    let color: Color32 = if greyed { Palette::tint(p.text_muted, 0.6) } else { p.text };
    // One line, ending in "…" when it doesn't fit.
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_string(), text::body(), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - tx - 4.0).max(10.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(pos2(tx, rect.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_text_at_pointer(name)
}

fn results(ui: &mut Ui, p: &Palette, v: &SidebarView<'_>, out: &mut Vec<SidebarAction>) {
    let pr = v.progress;
    let status = if pr.pending > 0 {
        format!("Reading files… {} left", pr.pending)
    } else if v.hits.is_empty() {
        "No matches".to_string()
    } else if v.hits.len() >= MAX_HITS {
        format!("First {MAX_HITS} matches")
    } else {
        format!("{} {}", v.hits.len(), if v.hits.len() == 1 { "match" } else { "matches" })
    };
    ui.label(egui::RichText::new(status).font(text::meta()).color(p.text_muted));
    ui.add_space(4.0);
    let mut last: Option<&Path> = None;
    for hit in v.hits {
        if last != Some(hit.path.as_path()) {
            let name = hit.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            ui.add_space(4.0);
            let r = row(ui, p, 0, Kind::for_path(&hit.path), &name, v.active == Some(hit.path.as_path()), false, None);
            if r.clicked() {
                out.push(SidebarAction::Open(hit.path.clone()));
            }
            last = Some(hit.path.as_path());
        }
        let label = hit.location.label();
        let width = ui.available_width();
        let text = if label.is_empty() { hit.snippet.clone() } else { format!("{label} · {}", hit.snippet) };
        let galley = ui.painter().layout(text, text::meta(), p.text_muted, width - 30.0);
        let h = galley.size().y + 8.0;
        let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(radius::CONTROL), p.surface_hover);
        }
        ui.painter().galley(pos2(rect.left() + 28.0, rect.top() + 4.0), galley, p.text_muted);
        if resp.clicked() {
            out.push(SidebarAction::OpenHit(hit.clone()));
        }
    }
    if !pr.failed.is_empty() {
        ui.add_space(8.0);
        let names: Vec<String> =
            pr.failed.iter().map(|(p, e)| format!("{}: {e}", p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())).collect();
        ui.label(egui::RichText::new(format!("{} files couldn't be read", pr.failed.len())).font(text::meta()).color(p.text_muted))
            .on_hover_text(names.join("\n"));
    }
}

/// The name for a new file of `kind` in `dir` that isn't taken: "Untitled Document.docx", then
/// "Untitled Document 2.docx"…
pub fn new_file_name(dir: &Path, kind: Kind, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let ext = match kind {
        Kind::Document => "docx",
        Kind::Sheet => "xlsx",
        Kind::Presentation => "pptx",
        Kind::Pdf => "pdf",
    };
    let base = format!("Untitled {}", kind.label());
    (1..10_000)
        .map(|n| dir.join(if n == 1 { format!("{base}.{ext}") } else { format!("{base} {n}.{ext}") }))
        .find(|p| !exists(p))
        .unwrap_or_else(|| dir.join(format!("{base} {}.{ext}", std::process::id())))
}

/// Whether `name` is a usable new name for a file or folder: one path part, not hidden.
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n != "." && n != ".." && !n.starts_with('.') && !n.contains('/') && !n.contains(':') && n.len() <= 255
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_files_get_the_first_free_name() {
        let dir = Path::new("/p");
        let taken = [PathBuf::from("/p/Untitled Sheet.xlsx"), PathBuf::from("/p/Untitled Sheet 2.xlsx")];
        assert_eq!(new_file_name(dir, Kind::Sheet, |p| taken.iter().any(|t| t == p)), PathBuf::from("/p/Untitled Sheet 3.xlsx"));
        assert_eq!(new_file_name(dir, Kind::Document, |_| false), PathBuf::from("/p/Untitled Document.docx"));
    }

    #[test]
    fn names_stay_in_their_folder() {
        assert!(valid_name("Q3 plan.docx"));
        for bad in ["", " ", "..", "a/b.docx", ".hidden", "x:y"] {
            assert!(!valid_name(bad), "{bad:?}");
        }
    }
}
