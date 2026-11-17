//! The Ask AI panel's drawing: a header with the chat menu, the conversation (your questions,
//! Claude's answers with their sources as chips, what the tools did), the question that allows
//! edits, the changes waiting to be kept or undone, and the box to ask in. Before a credential is
//! set up, a card that sets one up.

use super::{ChatPanel, PanelAction, Seg, cite_label, engine, key, memory, segments};
use egui::{Align, CornerRadius, Id, Key, Layout, Modifiers, Rect, RichText, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use officecraft_shell::ai::{Changed, ReviewAction};
use officecraft_shell::icons;
use officecraft_shell::tokens::{Palette, radius, size, space, text};
use officecraft_shell::widgets::{self, ButtonKind};
use serde_json::Value;

pub fn input_id() -> Id {
    Id::unique("cw_chat_input")
}

pub fn key_id() -> Id {
    Id::unique("cw_chat_key")
}

/// Questions offered in an empty chat.
const STARTERS: [&str; 3] = ["What's in this project?", "Summarise the document in front", "Make a one-page summary of this project as a new document"];

impl ChatPanel {
    /// Draws the panel into `ui` (the side panel's area).
    /// `changed`: the project's files with AI changes waiting to be kept or undone.
    pub fn view(&mut self, ui: &mut Ui, p: &Palette, project_name: Option<&str>, changed: &[Changed]) -> Vec<PanelAction> {
        let mut out = Vec::new();
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, p.surface);
        ui.painter().vline(full.left() + 0.5, full.y_range(), Stroke::new(1.0, p.hairline));
        let inner = full.shrink2(vec2(space::M, space::S));
        let header = Rect::from_min_size(inner.min, vec2(inner.width(), 32.0));
        self.header(ui, p, header, project_name);
        let body = Rect::from_min_max(pos2(inner.left(), header.bottom() + space::S), inner.max);
        let mut ui = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
        ui.spacing_mut().item_spacing.y = space::XS;
        if self.root.is_none() {
            ui.add_space(space::XL);
            ui.label(RichText::new("Ask AI works with a project: a folder of your files.").font(text::card()).color(p.text));
            ui.label(RichText::new("Open one and ask about everything in it.").font(text::meta()).color(p.text_muted));
            ui.add_space(space::S);
            if widgets::button(&mut ui, p, "Open Project…", ButtonKind::Primary, true).clicked() {
                out.push(PanelAction::OpenProject);
            }
            return out;
        }
        if !self.is_configured() {
            self.setup(&mut ui, p);
            return out;
        }
        if self.memory_open {
            self.memory_page(&mut ui, p);
            return out;
        }
        let composer_h = 112.0;
        let list = Rect::from_min_max(body.min, pos2(body.right(), body.bottom() - composer_h - space::S));
        let composer = Rect::from_min_max(pos2(body.left(), body.bottom() - composer_h), body.max);
        let mut lui = ui.new_child(UiBuilder::new().max_rect(list).layout(Layout::top_down(Align::Min)));
        self.conversation(&mut lui, p, changed, &mut out);
        let mut cui = ui.new_child(UiBuilder::new().max_rect(composer).layout(Layout::top_down(Align::Min)));
        self.composer(&mut cui, p, &mut out);
        out
    }

    fn header(&mut self, ui: &mut Ui, p: &Palette, r: Rect, project_name: Option<&str>) {
        let painter = ui.painter();
        icons::sparkle(painter, Rect::from_center_size(pos2(r.left() + 9.0, r.center().y), vec2(18.0, 18.0)), egui::Color32::WHITE);
        let title = painter.layout_no_wrap("Ask AI".into(), text::heading(), p.text);
        let tw = title.size().x;
        painter.galley(pos2(r.left() + 26.0, r.center().y - title.size().y / 2.0), title, p.text);
        if let Some(name) = project_name {
            let mut job = egui::text::LayoutJob::simple_singleline(name.to_string(), text::meta(), p.text_muted);
            job.wrap = egui::text::TextWrapping::truncate_at_width((r.width() - tw - 110.0).max(20.0));
            let g = painter.layout_job(job);
            painter.galley(pos2(r.left() + 34.0 + tw, r.center().y - g.size().y / 2.0), g, p.text_muted);
        }
        // New chat, and the chats menu.
        let menu = Rect::from_center_size(pos2(r.right() - 13.0, r.center().y), vec2(26.0, 26.0));
        let plus = menu.translate(vec2(-30.0, 0.0));
        let new = widgets::icon_button(ui, p, plus, Id::unique("cw_chat_new"), "New chat", false, icons::plus);
        if new.on_hover_text("New chat").clicked() {
            self.new_chat();
        }
        let m = widgets::icon_button(ui, p, menu, Id::unique("cw_chat_menu"), "Chats", false, icons::chevron_down);
        let m = m.on_hover_text("Chats and settings");
        let mut pick = None;
        let mut forget = false;
        let mut open_memory = false;
        let mut delete = false;
        egui::Popup::menu(&m).show(|ui| {
            ui.set_min_width(240.0);
            if self.chats.is_empty() {
                ui.label(RichText::new("No saved chats yet").color(p.text_muted));
            }
            for (id, title, _) in self.chats.iter().take(20) {
                let current = self.chat.as_ref().is_some_and(|c| &c.id == id);
                let label = if title.is_empty() { "Untitled chat" } else { title.as_str() };
                if ui.selectable_label(current, label).clicked() {
                    pick = Some(id.clone());
                }
            }
            ui.separator();
            if ui.button("Project Memory…").on_hover_text("What Claude remembers about this project").clicked() {
                open_memory = true;
            }
            let can_delete = self.chat.as_ref().is_some_and(|c| !c.messages.is_empty()) && !self.busy();
            if ui.add_enabled(can_delete, egui::Button::new("Delete This Chat")).clicked() {
                delete = true;
            }
            ui.separator();
            if (key::has_saved_key() || key::use_ant()) && ui.button("Remove API Key and Sign-In").clicked() {
                forget = true;
            }
        });
        if let Some(id) = pick {
            self.memory_open = false;
            self.switch_to(&id);
        }
        if open_memory {
            self.open_memory();
        }
        if delete {
            self.memory_open = false;
            self.delete_chat();
        }
        if forget {
            key::forget();
            self.configured = None;
        }
    }

    /// The card that sets up a credential.
    fn setup(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add_space(space::M);
        ui.label(RichText::new("Connect Ask AI to Claude").font(text::card()).color(p.text));
        ui.label(
            RichText::new(
                "Ask AI uses Claude through your own Anthropic API account, billed per question there (a typical question costs a few cents). Your questions, and what Claude reads from this project to answer them, are sent to Anthropic.",
            )
            .font(text::meta())
            .color(p.text_muted),
        );
        ui.add_space(space::S);
        let w = ui.available_width();
        let field = ui.add(
            egui::TextEdit::singleline(&mut self.key_input)
                .id(key_id())
                .password(true)
                .hint_text(RichText::new("Paste your API key").color(p.text_faint))
                .desired_width(w)
                .margin(vec2(space::S, 6.0)),
        );
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(space::XS);
        let mut save = enter;
        ui.allocate_ui_with_layout(vec2(w, 28.0), Layout::right_to_left(Align::Center), |ui| {
            save |= widgets::button(ui, p, "Save Key", ButtonKind::Primary, !self.key_input.trim().is_empty()).clicked();
            if ui.link(RichText::new("Get an API key").font(text::meta())).clicked() {
                officecraft_platform::files::open_url("https://platform.claude.com/settings/keys");
            }
        });
        if save && !self.key_input.trim().is_empty() {
            match key::save_key(&self.key_input) {
                Ok(()) => {
                    self.key_input.clear();
                    self.configured = None;
                    self.notice = None;
                    self.focus_input = true;
                }
                Err(e) => self.notice = Some(e),
            }
        }
        if key::ant_path().is_some() {
            ui.add_space(space::S);
            ui.label(RichText::new("Or use the sign-in of Anthropic's ant command, billed to your Console organization.").font(text::meta()).color(p.text_muted));
            ui.allocate_ui_with_layout(vec2(w, 28.0), Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, p, "Use My Anthropic Sign-In", ButtonKind::Secondary, true).clicked() {
                    if key::ant_token().is_some() {
                        key::set_use_ant(true);
                        self.configured = None;
                        self.notice = None;
                    } else {
                        self.notice = Some("Not signed in yet: run `ant auth login` in Terminal, then try again.".into());
                    }
                }
            });
        }
        ui.add_space(space::S);
        ui.label(
            RichText::new("A Claude Pro or Max plan can't be used here. It works with OfficeCraft through Connect AI, in Claude Desktop or Claude Code.")
                .font(text::meta())
                .color(p.text_muted),
        );
        if let Some(n) = &self.notice {
            ui.add_space(space::S);
            ui.label(RichText::new(n).font(text::meta()).color(p.danger));
        }
    }

    fn conversation(&mut self, ui: &mut Ui, p: &Palette, changed: &[Changed], out: &mut Vec<PanelAction>) {
        let width = ui.available_width();
        let empty = self.chat.as_ref().is_none_or(|c| c.messages.is_empty()) && self.running.is_none() && changed.is_empty();
        if empty {
            ui.add_space(space::XL);
            ui.label(RichText::new("Ask about the files in this project").font(text::card()).color(p.text));
            ui.label(
                RichText::new("Claude reads and searches them to answer, and says where each answer came from. It can change them and make new ones: you watch each change in its tab, then keep or undo it.")
                    .font(text::meta())
                    .color(p.text_muted),
            );
            ui.add_space(space::S);
            for s in STARTERS {
                let b = ui.add(egui::Button::new(RichText::new(s).font(text::meta()).color(p.text)).fill(p.surface_raised).stroke(Stroke::new(1.0, p.hairline)).corner_radius(CornerRadius::same(radius::CONTROL)));
                if b.clicked() {
                    self.input = s.to_string();
                    out.push(PanelAction::Send);
                }
            }
            self.show_notice(ui, p);
            return;
        }
        ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = space::XS;
            let messages: Vec<Value> = self.chat.as_ref().map(|c| c.messages.clone()).unwrap_or_default();
            for m in &messages {
                match m["role"].as_str() {
                    Some("user") => {
                        let said = super::store::user_text(m);
                        if !said.trim().is_empty() {
                            ui.add_space(space::S);
                            user_bubble(ui, p, &said, width);
                            ui.add_space(space::XS);
                        }
                    }
                    Some("assistant") => {
                        for b in m["content"].as_array().into_iter().flatten() {
                            match b["type"].as_str() {
                                Some("text") => answer_text(ui, p, b["text"].as_str().unwrap_or_default(), out),
                                Some("tool_use") => {
                                    let (name, input) = (b["name"].as_str().unwrap_or_default(), &b["input"]);
                                    step_row(ui, p, &engine::step_label(name, input), false, engine::is_change(name, input));
                                }
                                Some("compaction") => {
                                    ui.label(
                                        RichText::new("The earlier part of this chat was summarised to make room; Claude works from the summary.")
                                            .font(text::meta())
                                            .italics()
                                            .color(p.text_muted),
                                    );
                                }
                                Some("fallback") => {
                                    ui.label(RichText::new("Another Claude model finished this answer.").font(text::meta()).italics().color(p.text_muted));
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            if self.running.is_some() {
                if !self.live.is_empty() {
                    answer_text(ui, p, &self.live, out);
                }
                let doing = self.step.clone().unwrap_or_else(|| if self.live.is_empty() { "Thinking…".into() } else { String::new() });
                if !doing.is_empty() && self.asking.is_none() {
                    step_row(ui, p, &doing, true, false);
                }
            }
            if let Some(what) = self.asking.clone() {
                ui.add_space(space::S);
                self.allow_card(ui, p, &what, project_label(self.root.as_deref()), out);
            }
            if !changed.is_empty() {
                ui.add_space(space::S);
                review_card(ui, p, self.root.as_deref(), changed, out);
            }
            self.show_notice(ui, p);
            ui.add_space(space::S);
        });
    }

    /// "Allow Ask AI to change files?": the first change in a project that lets the AI only read.
    fn allow_card(&mut self, ui: &mut Ui, p: &Palette, what: &str, project: String, out: &mut Vec<PanelAction>) {
        card(ui, p, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                icons::sparkle(ui.painter(), r, egui::Color32::WHITE);
                ui.label(RichText::new("Allow changes to files?").font(text::card()).color(p.text));
            });
            ui.label(RichText::new(format!("Claude wants to: {what}.")).font(text::body()).color(p.text));
            ui.label(
                RichText::new(format!(
                    "Allowing lets Ask AI edit and make files in {project}. Each change shows in its tab and isn't saved until you keep it."
                ))
                .font(text::meta())
                .color(p.text_muted),
            );
            ui.add_space(space::XS);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), size::CONTROL_H), Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, p, "Allow Edits", ButtonKind::Primary, true).clicked() {
                    out.push(PanelAction::AllowEdits);
                }
                if widgets::button(ui, p, "Not Now", ButtonKind::Secondary, true).clicked() {
                    self.answer(false);
                }
            });
        });
    }

    /// The project's memory: each note, editable, with Save and Delete; and Clear Memory.
    fn memory_page(&mut self, ui: &mut Ui, p: &Palette) {
        if ui.link(RichText::new("‹ Back to Chat").font(text::meta())).clicked() {
            self.memory_open = false;
            return;
        }
        ui.add_space(space::XS);
        ui.label(RichText::new("Project memory").font(text::card()).color(p.text));
        ui.label(
            RichText::new(
                "What Claude remembers about this project; every chat here reads it. It's kept in the project folder (.officecraft/memory), so it goes with the project. Change or delete anything.",
            )
            .font(text::meta())
            .color(p.text_muted),
        );
        ui.add_space(space::S);
        let Some(root) = self.root.clone() else { return };
        let mem = memory::Memory::new(&root);
        let (mut save, mut delete, mut clear) = (None, None, false);
        let width = ui.available_width();
        let bottom_h = if self.notes.is_empty() { 0.0 } else { size::CONTROL_H + space::S };
        let list_h = (ui.available_height() - bottom_h).max(80.0);
        ScrollArea::vertical().max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(width);
            if self.notes.is_empty() {
                ui.label(
                    RichText::new("Nothing remembered yet. Claude notes lasting facts here as you work together, or when you ask it to remember something.")
                        .font(text::body())
                        .color(p.text_muted),
                );
            }
            for (i, (path, saved, text_)) in self.notes.iter_mut().enumerate() {
                card(ui, p, |ui| {
                    let name = path.strip_prefix("/memories/").unwrap_or(path);
                    ui.label(RichText::new(name).font(text::card()).color(p.text));
                    ui.add(
                        egui::TextEdit::multiline(text_)
                            .id(ui.scope_id().with(("cw_memory_note", i)))
                            .font(text::body())
                            .desired_width(ui.available_width())
                            .desired_rows(3),
                    );
                    let changed = text_ != saved;
                    ui.allocate_ui_with_layout(vec2(ui.available_width(), size::CONTROL_H), Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, p, "Save", ButtonKind::Primary, changed).clicked() {
                            save = Some(i);
                        }
                        if widgets::button(ui, p, "Delete", ButtonKind::Secondary, true).clicked() {
                            delete = Some(i);
                        }
                    });
                });
                ui.add_space(space::XS);
            }
        });
        if !self.notes.is_empty() {
            ui.add_space(space::S);
            ui.allocate_ui_with_layout(vec2(width, size::CONTROL_H), Layout::right_to_left(Align::Center), |ui| {
                let label = if self.clear_armed { "Click Again to Clear" } else { "Clear Memory" };
                let b = widgets::button(ui, p, label, ButtonKind::Secondary, true);
                if b.on_hover_text("Forget everything Claude noted about this project").clicked() {
                    if self.clear_armed {
                        clear = true;
                    }
                    self.clear_armed = !self.clear_armed;
                }
            });
        }
        let result = if let Some(i) = save {
            let (path, _, text_) = &self.notes[i];
            mem.save(path, text_).map(|()| self.notes[i].1 = self.notes[i].2.clone())
        } else if let Some(i) = delete {
            mem.delete(&self.notes[i].0).map(|()| {
                self.notes.remove(i);
            })
        } else if clear {
            mem.clear().map(|()| self.notes.clear())
        } else {
            Ok(())
        };
        if let Err(e) = result {
            self.notice = Some(format!("Couldn't change the memory: {e}"));
        }
        self.show_notice(ui, p);
    }

    fn show_notice(&self, ui: &mut Ui, p: &Palette) {
        if let Some(n) = &self.notice {
            ui.add_space(space::S);
            ui.horizontal_wrapped(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                icons::alert(ui.painter(), r, egui::Color32::WHITE);
                ui.label(RichText::new(n).font(text::meta()).color(p.text));
            });
        }
    }

    fn composer(&mut self, ui: &mut Ui, p: &Palette, out: &mut Vec<PanelAction>) {
        let r = ui.max_rect();
        let box_r = Rect::from_min_max(r.min, pos2(r.right(), r.bottom() - 18.0));
        let focused = ui.memory(|m| m.has_focus(input_id()));
        let corner = CornerRadius::same(radius::CARD);
        ui.painter().rect_filled(box_r, corner, p.surface_raised);
        ui.painter().rect_stroke(box_r, corner, Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { p.accent } else { p.border }), egui::StrokeKind::Inside);
        // Enter sends; Shift-Enter starts a new line.
        if focused && !self.busy() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            out.push(PanelAction::Send);
        }
        let field_r = Rect::from_min_max(box_r.min + vec2(space::S, space::S), box_r.max - vec2(40.0, space::S));
        let mut fui = ui.new_child(UiBuilder::new().max_rect(field_r).layout(Layout::top_down(Align::Min)));
        let hint = if self.busy() { "Claude is working…" } else { "Ask, or say what to change…" };
        let field = fui.add(
            egui::TextEdit::multiline(&mut self.input)
                .id(input_id())
                .frame(egui::Frame::NONE)
                .hint_text(RichText::new(hint).color(p.text_faint))
                .font(text::body())
                .desired_width(field_r.width())
                .desired_rows(3),
        );
        if std::mem::take(&mut self.focus_input) {
            field.request_focus();
        }
        let btn = Rect::from_center_size(pos2(box_r.right() - 20.0, box_r.bottom() - 20.0), vec2(28.0, 28.0));
        if self.busy() {
            let s = widgets::icon_button(ui, p, btn, Id::unique("cw_chat_stop"), "Stop", false, icons::stop);
            if s.on_hover_text("Stop").clicked() {
                self.stop();
            }
        } else {
            let can = !self.input.trim().is_empty();
            let resp = ui.interact(btn, Id::unique("cw_chat_send"), if can { Sense::click() } else { Sense::hover() });
            let fill = if can { p.accent } else { Palette::tint(p.text_muted, 0.25) };
            ui.painter().circle_filled(btn.center(), 13.0, fill);
            icons::send(ui.painter(), Rect::from_center_size(btn.center(), vec2(14.0, 14.0)), p.accent_ink);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, can, "Send"));
            if resp.on_hover_text("Send (Return)").clicked() {
                out.push(PanelAction::Send);
            }
        }
        ui.painter().text(
            pos2(r.left() + 2.0, r.bottom() - 7.0),
            egui::Align2::LEFT_CENTER,
            "Claude Opus 5.5 · Check the sources.",
            text::meta(),
            p.text_muted,
        );
    }
}

/// The person's question, on the right.
fn user_bubble(ui: &mut Ui, p: &Palette, said: &str, width: f32) {
    ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::right_to_left(Align::Min), |ui| {
        egui::Frame::NONE
            .fill(Palette::tint(p.accent, if p.dark { 0.24 } else { 0.12 }))
            .corner_radius(CornerRadius::same(radius::CARD))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_max_width(width * 0.82);
                // Wrapped and left-aligned inside the right-aligned bubble.
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    ui.add(egui::Label::new(RichText::new(said).font(text::body()).color(p.text)).wrap());
                });
            });
    });
}

/// A raised card in the conversation.
fn card(ui: &mut Ui, p: &Palette, add: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::XS;
            add(ui);
        });
}

/// The project's name for a sentence ("“Q3 Planning”"), or "this project".
fn project_label(root: Option<&std::path::Path>) -> String {
    root.and_then(|r| r.file_name()).map_or_else(|| "this project".into(), |n| format!("“{}”", n.to_string_lossy()))
}

/// The files Claude changed, each kept (saved) or undone here.
fn review_card(ui: &mut Ui, p: &Palette, root: Option<&std::path::Path>, changed: &[Changed], out: &mut Vec<PanelAction>) {
    card(ui, p, |ui| {
        let n: u32 = changed.iter().map(|c| c.edits).sum();
        ui.label(RichText::new("Changes to review").font(text::card()).color(p.text));
        ui.label(
            RichText::new(format!(
                "{n} change{} in {} file{}. Keep saves a file; Undo takes its changes back.",
                if n == 1 { "" } else { "s" },
                changed.len(),
                if changed.len() == 1 { "" } else { "s" }
            ))
            .font(text::meta())
            .color(p.text_muted),
        );
        ui.add_space(space::XS);
        for c in changed {
            let name = root
                .and_then(|r| c.path.strip_prefix(r.canonicalize().unwrap_or_else(|_| r.to_path_buf())).ok().or_else(|| c.path.strip_prefix(r).ok()))
                .map_or_else(|| c.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), |r| r.to_string_lossy().into_owned());
            // Laid out by hand: the icon, the name cut to fit, then Undo and Keep at the right.
            let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), size::CONTROL_H), Sense::hover());
            icons::kind(ui.painter(), Rect::from_center_size(pos2(row.left() + 9.0, row.center().y), vec2(18.0, 18.0)), c.kind, egui::Color32::WHITE);
            let kw = widgets::button_width(ui, "Keep");
            let uw = widgets::button_width(ui, "Undo");
            let keep = Rect::from_min_size(pos2(row.right() - kw, row.top()), vec2(kw, size::CONTROL_H));
            let undo = Rect::from_min_size(pos2(keep.left() - space::S - uw, row.top()), vec2(uw, size::CONTROL_H));
            if widgets::button_at(ui, p, keep, "Keep", ButtonKind::Primary, true).on_hover_text(format!("Save {name}")).clicked() {
                out.push(PanelAction::Review(ReviewAction::Keep(c.path.clone())));
            }
            if widgets::button_at(ui, p, undo, "Undo", ButtonKind::Secondary, true).on_hover_text("Take Claude's changes back").clicked() {
                out.push(PanelAction::Review(ReviewAction::Undo(c.path.clone())));
            }
            let name_left = row.left() + 18.0 + space::S;
            let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), text::body(), p.text);
            job.wrap = egui::text::TextWrapping::truncate_at_width((undo.left() - space::S - name_left).max(20.0));
            let g = ui.painter().layout_job(job);
            let name_r = Rect::from_min_size(pos2(name_left, row.center().y - g.size().y / 2.0), g.size());
            ui.painter().galley(name_r.min, g, p.text);
            ui.interact(name_r, ui.scope_id().with(("cw_chat_changed", &c.path)), Sense::hover())
                .on_hover_text(format!("{name}: {} change{}", c.edits, if c.edits == 1 { "" } else { "s" }));
        }
        if changed.len() > 1 {
            ui.allocate_ui_with_layout(vec2(ui.available_width(), size::CONTROL_H), Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, p, "Keep All", ButtonKind::Secondary, true).clicked() {
                    out.push(PanelAction::Review(ReviewAction::KeepAll));
                }
            });
        }
    });
}

/// A step the tools took, small and muted; `busy` with a spinner; `change` when it changed a file.
fn step_row(ui: &mut Ui, p: &Palette, label: &str, busy: bool, change: bool) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        if busy {
            icons::spinner(ui.painter(), r, ui.input(|i| i.time), p.text_muted);
            ui.ctx().request_repaint();
        } else if change {
            icons::file(ui.painter(), r, p.accent);
        } else {
            icons::search(ui.painter(), r, p.text_faint);
        }
        ui.label(RichText::new(label).font(text::meta()).color(p.text_muted));
    });
}

/// An answer: paragraphs, "- " lists, **bold**, and sources as chips that open the place cited.
fn answer_text(ui: &mut Ui, p: &Palette, text_: &str, out: &mut Vec<PanelAction>) {
    let mut code = false;
    for line in text_.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            code = !code;
            continue;
        }
        if code {
            ui.label(RichText::new(line).monospace().color(p.text));
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(space::XS);
            continue;
        }
        let (bullet, content, heading) = if let Some(rest) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            (true, rest, false)
        } else if let Some(rest) = trimmed.strip_prefix("#") {
            (false, rest.trim_start_matches('#').trim_start(), true)
        } else {
            (false, trimmed, false)
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            if bullet {
                ui.add_space(4.0);
                ui.label(RichText::new("•  ").font(text::body()).color(p.text_muted));
            }
            for seg in segments(content) {
                match seg {
                    Seg::Text(t) => {
                        let rt = RichText::new(t).color(p.text);
                        ui.label(if heading { rt.font(text::card()) } else { rt.font(text::body()) });
                    }
                    Seg::Bold(t) => {
                        ui.label(RichText::new(t).font(text::card()).color(p.text));
                    }
                    Seg::Cite { file, at } => {
                        ui.add_space(3.0);
                        let label = cite_label(&file, at.as_deref());
                        let chip = egui::Button::new(RichText::new(label).font(text::meta()).color(p.accent))
                            .fill(Palette::tint(p.accent, if p.dark { 0.18 } else { 0.09 }))
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(255))
                            .min_size(vec2(0.0, 18.0));
                        let r = ui.add(chip).on_hover_text(match &at {
                            Some(w) => format!("Open {file} at {w}"),
                            None => format!("Open {file}"),
                        });
                        if r.clicked() {
                            out.push(PanelAction::OpenSource { file, at });
                        }
                        ui.add_space(3.0);
                    }
                }
            }
        });
    }
}
