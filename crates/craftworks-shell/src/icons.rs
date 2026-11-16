//! The chrome's icons (docs/design.md "Icons"), all our own. Things (a kind of file, a
//! folder, a project, Home, the AI, a result) are Liquid Glass tiles: artwork drawn by
//! `assets/make_icons.py` and embedded. Controls (plus, close, chevron, search, pin, the
//! sidebar toggle) are line glyphs on a 16 pt grid with a 1.5 pt round stroke, drawn in code
//! in the colour they are given, as symbols sit on glass.

use crate::tokens::Palette;
use craftworks_core::Kind;
use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use std::f32::consts::{PI, TAU};

const STROKE: f32 = 1.5;

/// Maps a point on a 16×16 grid into `r`.
fn at(r: Rect, x: f32, y: f32) -> Pos2 {
    let s = scale(r);
    let o = r.center() - vec2(8.0 * s, 8.0 * s);
    pos2(o.x + x * s, o.y + y * s)
}

fn scale(r: Rect) -> f32 {
    r.width().min(r.height()) / 16.0
}

/// A corner radius of `pt` grid points in `r`.
fn corner(r: Rect, pt: f32) -> u8 {
    (pt * scale(r)).round().clamp(1.0, 64.0) as u8
}

/// A polyline through screen points with round ends and joins, `w` wide.
fn round_line(p: &Painter, pts: Vec<Pos2>, w: f32, color: Color32) {
    // Dots under the line round its ends and corners (only where an overlap can't show).
    if w >= 1.0 && color.a() == 255 {
        for q in &pts {
            p.circle_filled(*q, w / 2.0, color);
        }
    }
    p.add(Shape::line(pts, Stroke::new(w, color)));
}

/// A polyline on the grid, at the standard stroke.
fn line(p: &Painter, r: Rect, pts: &[(f32, f32)], color: Color32) {
    line_w(p, r, pts, STROKE, color);
}

fn line_w(p: &Painter, r: Rect, pts: &[(f32, f32)], w: f32, color: Color32) {
    round_line(p, pts.iter().map(|&(x, y)| at(r, x, y)).collect(), w * scale(r), color);
}

fn ring(p: &Painter, r: Rect, c: (f32, f32), rad: f32, w: f32, color: Color32) {
    p.circle_stroke(at(r, c.0, c.1), rad * scale(r), Stroke::new(w * scale(r), color));
}

/// The Liquid Glass icons: artwork drawn by `assets/make_icons.py` (tinted glass tiles with a
/// frosted white glyph) and embedded here. Small sizes use the 64 px drawing, large ones the
/// 256 px one, so neither is stretched nor crushed.
mod art {
    macro_rules! png {
        ($name:literal) => {
            include_bytes!(concat!("../assets/icons/", $name, ".png")).as_slice()
        };
    }
    pub const DOC: [&[u8]; 2] = [png!("doc-64"), png!("doc-256")];
    pub const SHEET: [&[u8]; 2] = [png!("sheet-64"), png!("sheet-256")];
    pub const DECK: [&[u8]; 2] = [png!("deck-64"), png!("deck-256")];
    pub const PDF: [&[u8]; 2] = [png!("pdf-64"), png!("pdf-256")];
    pub const MARK: [&[u8]; 2] = [png!("mark-64"), png!("mark-256")];
    pub const HOME: [&[u8]; 2] = [png!("home-64"), png!("home-64")];
    pub const FOLDER: [&[u8]; 2] = [png!("folder-64"), png!("folder-64")];
    pub const PROJECT: [&[u8]; 2] = [png!("project-64"), png!("project-64")];
    pub const CHECK: [&[u8]; 2] = [png!("check-64"), png!("check-64")];
    pub const ALERT: [&[u8]; 2] = [png!("alert-64"), png!("alert-64")];
    pub const SPARKLE: [&[u8]; 2] = [png!("sparkle-64"), png!("sparkle-64")];
}

/// The texture for one drawing, decoded once and kept by the context.
fn texture(ctx: &egui::Context, png: &'static [u8]) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique(("cw_glass_icon", png.as_ptr() as usize));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
    let t = ctx.load_texture(format!("cw-icon-{}", png.as_ptr() as usize), egui::ColorImage::from_rgba_unmultiplied(size, &img), options);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// Paints a glass icon in `r`. Its glass has its own colour: of `color` only the opacity is
/// used (a faded icon for a missing file, a breathing one for the AI at work).
fn glass(p: &Painter, r: Rect, art: [&'static [u8]; 2], color: Color32) {
    let ctx = p.ctx();
    let px = r.width().min(r.height()) * ctx.pixels_per_point();
    let Some(t) = texture(ctx, art[usize::from(px > 96.0)]) else { return };
    // The drawing keeps a margin for its shadow: let the tile itself fill the icon's box.
    let side = r.width().min(r.height()) * 1.14;
    let rect = Rect::from_center_size(r.center() + vec2(0.0, side * 0.012), vec2(side, side));
    let ppp = ctx.pixels_per_point();
    let rect = Rect::from_min_size(pos2((rect.left() * ppp).round() / ppp, (rect.top() * ppp).round() / ppp), rect.size());
    let a = color.a();
    p.image(t.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_rgba_premultiplied(a, a, a, a));
}

/// A kind of file: its glass tile (blue page, green table, orange slide, red PDF).
pub fn kind(p: &Painter, r: Rect, kind: Kind, color: Color32) {
    let art = match kind {
        Kind::Document => art::DOC,
        Kind::Sheet => art::SHEET,
        Kind::Presentation => art::DECK,
        Kind::Pdf => art::PDF,
    };
    glass(p, r, art, color);
}

/// A file Craftworks doesn't open: an outlined page with a folded corner.
pub fn file(p: &Painter, r: Rect, color: Color32) {
    line(p, r, &[(9.25, 1.75), (4.5, 1.75), (3.5, 2.75), (3.5, 13.25), (4.5, 14.25), (11.5, 14.25), (12.5, 13.25), (12.5, 5.0), (9.25, 1.75)], color);
    line(p, r, &[(9.25, 1.75), (9.25, 5.0), (12.5, 5.0)], color);
}

/// Craftworks' mark: the app icon, a fan of four sheets on blue glass (a page of text in
/// front, and behind it a green, an orange and a red sheet, one for each kind of file).
pub fn mark(p: &Painter, r: Rect, _pal: &Palette) {
    glass(p, r, art::MARK, Color32::WHITE);
}

pub fn home(p: &Painter, r: Rect, color: Color32) {
    glass(p, r, art::HOME, Color32::WHITE.gamma_multiply(f32::from(color.a()) / 255.0));
}

/// A project: a folder with a document showing in it.
pub fn project(p: &Painter, r: Rect, color: Color32) {
    glass(p, r, art::PROJECT, Color32::WHITE.gamma_multiply(f32::from(color.a()) / 255.0));
}

pub fn plus(p: &Painter, r: Rect, color: Color32) {
    line(p, r, &[(8.0, 3.5), (8.0, 12.5)], color);
    line(p, r, &[(3.5, 8.0), (12.5, 8.0)], color);
}

pub fn close(p: &Painter, r: Rect, color: Color32) {
    line(p, r, &[(4.5, 4.5), (11.5, 11.5)], color);
    line(p, r, &[(11.5, 4.5), (4.5, 11.5)], color);
}

pub fn chevron_down(p: &Painter, r: Rect, color: Color32) {
    chevron_turn(p, r, 1.0, color);
}

/// A chevron pointing right at `t` = 0 and down at `t` = 1, turning in between (a folder
/// opening).
pub fn chevron_turn(p: &Painter, r: Rect, t: f32, color: Color32) {
    let (sin, cos) = (t.clamp(0.0, 1.0) * PI / 2.0).sin_cos();
    let pts = [(-1.75, -3.5), (1.75, 0.0), (-1.75, 3.5)].iter().map(|&(x, y)| at(r, 8.0 + x * cos - y * sin, 8.0 + x * sin + y * cos)).collect();
    round_line(p, pts, STROKE * scale(r), color);
}

pub fn folder(p: &Painter, r: Rect, color: Color32) {
    glass(p, r, art::FOLDER, Color32::WHITE.gamma_multiply(f32::from(color.a()) / 255.0));
}

pub fn search(p: &Painter, r: Rect, color: Color32) {
    ring(p, r, (7.0, 7.0), 4.25, STROKE, color);
    line(p, r, &[(10.2, 10.2), (13.25, 13.25)], color);
}

/// A pushpin.
pub fn pin(p: &Painter, r: Rect, color: Color32) {
    line(p, r, &[(5.75, 2.5), (10.25, 2.5)], color);
    line(p, r, &[(6.5, 2.5), (6.5, 7.0), (4.5, 9.5), (11.5, 9.5), (9.5, 7.0), (9.5, 2.5)], color);
    line(p, r, &[(8.0, 9.5), (8.0, 13.75)], color);
}

/// A tick on green glass: done.
pub fn check_circle(p: &Painter, r: Rect, _color: Color32) {
    glass(p, r, art::CHECK, Color32::WHITE);
}

/// An exclamation mark on red glass: something went wrong.
pub fn alert(p: &Painter, r: Rect, _color: Color32) {
    glass(p, r, art::ALERT, Color32::WHITE);
}

/// The AI's sparkle on violet glass; `color`'s opacity fades it.
pub fn sparkle(p: &Painter, r: Rect, color: Color32) {
    glass(p, r, art::SPARKLE, Color32::WHITE.gamma_multiply(f32::from(color.a()) / 255.0));
}

/// The sidebar toggle: a window with its left pane, filled in while the sidebar is `open`.
pub fn sidebar(p: &Painter, r: Rect, open: bool, color: Color32) {
    let s = scale(r);
    let w = Rect::from_min_max(at(r, 2.0, 3.0), at(r, 14.0, 13.0));
    let rad = corner(r, 2.5);
    if open {
        let pane = Rect::from_min_max(w.min, at(r, 6.25, 13.0));
        p.rect_filled(pane, CornerRadius { nw: rad, sw: rad, ne: 0, se: 0 }, Palette::tint(color, 0.3));
    }
    p.rect_stroke(w, rad, Stroke::new(STROKE * s, color), egui::StrokeKind::Middle);
    p.line_segment([at(r, 6.25, 3.0), at(r, 6.25, 13.0)], Stroke::new(STROKE * s, color));
}

/// A busy ring: a faint track with a bright arc going round. `time` is in seconds.
pub fn spinner(p: &Painter, r: Rect, time: f64, color: Color32) {
    ring(p, r, (8.0, 8.0), 5.25, STROKE, Palette::tint(color, 0.22));
    let a0 = (time * 5.5 % f64::from(TAU)) as f32;
    let pts = (0..=12).map(|i| {
        let a = a0 + TAU * 0.3 * i as f32 / 12.0;
        at(r, 8.0 + 5.25 * a.cos(), 8.0 + 5.25 * a.sin())
    });
    round_line(p, pts.collect(), STROKE * scale(r), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_paint_at_every_size() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.painter().clone();
            for size in [10.0, 16.0, 40.0] {
                let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(size, size));
                for k in Kind::ALL {
                    kind(&p, r, k, Color32::RED);
                }
                file(&p, r, Color32::GRAY);
                mark(&p, r, &Palette::LIGHT);
                mark(&p, r, &Palette::DARK);
                home(&p, r, Color32::GRAY);
                project(&p, r, Color32::GRAY);
                plus(&p, r, Color32::GRAY);
                close(&p, r, Color32::GRAY);
                chevron_down(&p, r, Color32::GRAY);
                chevron_turn(&p, r, 0.4, Color32::GRAY);
                folder(&p, r, Color32::GRAY);
                search(&p, r, Color32::GRAY);
                pin(&p, r, Color32::GRAY);
                check_circle(&p, r, Color32::GRAY);
                alert(&p, r, Color32::GRAY);
                sparkle(&p, r, Color32::GRAY);
                sidebar(&p, r, true, Color32::GRAY);
                spinner(&p, r, 1.3, Color32::GRAY);
            }
        });
        out.textures_delta.clear();
        // Drawing must not panic; the grid maps to the rect's square.
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(32.0, 32.0));
        assert_eq!(at(r, 0.0, 0.0), pos2(0.0, 0.0));
        assert_eq!(at(r, 16.0, 16.0), pos2(32.0, 32.0));
    }
}
