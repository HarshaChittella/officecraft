//! Each app's commands as its menus would list them: id, label, where it lives in the app
//! (ribbon tab and group, `["Home", "Font"]`), its shortcut as the app writes it, and whether
//! it needs arguments (a menu can't give any). WordCraft, GridCraft and DeckCraft have no Mac
//! menu bar of their own that we can link, so Craftworks builds theirs from this (D42).
//! PdfCraft has one (`Editor::menu_bar`).

use crate::Kind;

/// One command, as a menu shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
    /// Ribbon tab, then group (either may be missing).
    pub place: Vec<String>,
    /// The app's own notation (`Mod+B`, `Cmd+Shift+K`, `⌘B`); the first of any alternatives.
    pub shortcut: Option<String>,
    /// The command needs arguments, so a plain menu click can't run it.
    pub needs_args: bool,
}

/// Whether a parameter description names a key that isn't optional: `{}` and `{on?: bool}`
/// don't; `{"size": number}` does. Alternatives (` | `) are fine if any one has none.
pub fn needs_args(params: &str) -> bool {
    let params = params.split('→').next().unwrap_or("").trim();
    if params.is_empty() || params == "{}" {
        return false;
    }
    params.split(" | ").all(|alt| {
        let inner = alt.trim().trim_start_matches('{').trim_end_matches('}');
        let mut depth = 0i32;
        let mut parts = Vec::new();
        let mut cur = String::new();
        for c in inner.chars() {
            match c {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth -= 1,
                ',' if depth == 0 => {
                    parts.push(std::mem::take(&mut cur));
                    continue;
                }
                _ => {}
            }
            cur.push(c);
        }
        parts.push(cur);
        parts.iter().any(|p| {
            let key = p.split(':').next().unwrap_or("").trim().trim_matches('"');
            !key.is_empty() && !key.ends_with('?') && !key.starts_with('(')
        })
    })
}

fn first_alternative(s: &str) -> Option<String> {
    s.split(" / ").map(str::trim).find(|x| !x.is_empty()).map(str::to_string)
}

/// The app's commands, in its own order.
pub fn entries(kind: Kind) -> Vec<MenuEntry> {
    match kind {
        Kind::Document => wordcraft_engine::cmd::registry()
            .all()
            .iter()
            .map(|c| MenuEntry {
                id: c.id.to_string(),
                label: c.label.to_string(),
                place: c.location.split('›').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
                shortcut: first_alternative(c.shortcut),
                needs_args: needs_args(c.params),
            })
            .collect(),
        Kind::Sheet => gridcraft_engine::command_specs()
            .iter()
            .map(|c| MenuEntry {
                id: c.id.to_string(),
                label: c.label.to_string(),
                place: c.menu.iter().map(|p| p.to_string()).collect(),
                shortcut: c.shortcut_mac.or(c.shortcut).and_then(first_alternative),
                needs_args: needs_args(c.params),
            })
            .collect(),
        Kind::Presentation => {
            let mut out: Vec<MenuEntry> = deckcraft_engine::command_specs()
                .iter()
                .map(|c| MenuEntry {
                    id: c.id.to_string(),
                    label: c.label.to_string(),
                    place: c.menu.iter().map(|p| p.to_string()).collect(),
                    shortcut: c.shortcut.and_then(first_alternative),
                    needs_args: needs_args(c.params),
                })
                .collect();
            // The app's own views (not engine commands) belong in View.
            for (id, label, shortcut, params) in deckcraft_ui_egui::UI_COMMANDS {
                if id.starts_with("view.") {
                    out.push(MenuEntry {
                        id: id.to_string(),
                        label: label.to_string(),
                        place: vec!["View".into()],
                        shortcut: shortcut.and_then(first_alternative),
                        needs_args: needs_args(params),
                    });
                }
            }
            out
        }
        Kind::Pdf => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_a_menu_cant_give_are_spotted() {
        assert!(!needs_args(""));
        assert!(!needs_args("{}"));
        assert!(!needs_args("{on?: bool}"));
        assert!(!needs_args("{rect?: [x,y,w,h], text?: string}"));
        assert!(needs_args(r#"{"size": number}"#));
        assert!(needs_args("{index: number, at?: [x, y]}"));
        assert!(!needs_args("{cell: string} | {}"), "one alternative needs nothing");
        assert!(!needs_args("{} → {ok: bool}"), "the answer isn't an argument");
    }

    #[test]
    fn the_three_apps_without_a_menu_bar_have_commands_with_places() {
        for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
            let e = entries(kind);
            let placed = e.iter().filter(|x| !x.place.is_empty() && !x.needs_args).count();
            assert!(placed > 50, "{kind:?}: only {placed} menu-ready commands");
            assert!(e.iter().any(|x| x.shortcut.is_some()), "{kind:?}: no shortcuts");
        }
    }
}
