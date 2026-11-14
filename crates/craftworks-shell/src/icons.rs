//! The chrome's icons, drawn in code (docs/design.md "Icons"): 16 pt on a 1.5 pt stroke,
//! scaled to any rect. Our own shapes; nothing traced from a vendor's file icons.

use craftworks_core::Kind;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

const STROKE: f32 = 1.5;

/// Maps a point on a 16×16 grid into `r`.
fn at(r: Rect, x: f32, y: f32) -> Pos2 {
    let s = r.width().min(r.height()) / 16.0;
    let o = r.center() - vec2(8.0 * s, 8.0 * s);
    pos2(o.x + x * s, o.y + y * s)
}

fn scale(r: Rect) -> f32 {
    r.width().min(r.height()) / 16.0
}

fn line(p: &Painter, r: Rect, pts: &[(f32, f32)], color: Color32) {
    let pts: Vec<Pos2> = pts.iter().map(|&(x, y)| at(r, x, y)).collect();
    p.add(Shape::line(pts, Stroke::new(STROKE * scale(r), color)));
}

/// A sheet of paper with a folded corner (outline in `ink`), and the kind's glyph in `color`.
pub fn kind(p: &Painter, r: Rect, kind: Kind, color: Color32, ink: Color32) {
    let s = scale(r);
    // The sheet: 3..13 wide, 1.5..14.5 tall, corner folded at the top right.
    line(p, r, &[(9.5, 1.5), (3.5, 1.5), (3.0, 2.0), (3.0, 14.0), (3.5, 14.5), (12.5, 14.5), (13.0, 14.0), (13.0, 5.0), (9.5, 1.5)], ink);
    line(p, r, &[(9.5, 1.5), (9.5, 5.0), (13.0, 5.0)], ink);
    let st = Stroke::new(STROKE * s, color);
    match kind {
        Kind::Document => {
            for (y, w) in [(7.5, 6.0), (9.75, 6.0), (12.0, 3.75)] {
                p.line_segment([at(r, 5.25, y), at(r, 5.25 + w, y)], st);
            }
        }
        Kind::Sheet => {
            let g = Rect::from_min_max(at(r, 5.0, 7.0), at(r, 11.0, 12.5));
            p.rect_stroke(g, 0.5 * s, st, egui::StrokeKind::Middle);
            p.line_segment([pos2(g.center().x, g.top()), pos2(g.center().x, g.bottom())], st);
            p.line_segment([pos2(g.left(), g.center().y), pos2(g.right(), g.center().y)], st);
        }
        Kind::Presentation => {
            let base = 12.5;
            for (x, h) in [(5.75, 2.0), (8.0, 4.0), (10.25, 3.0)] {
                p.line_segment([at(r, x, base), at(r, x, base - h)], Stroke::new(1.75 * s, color));
            }
        }
        Kind::Pdf => {
            // A round stamp with a tick: "signed and settled".
            p.circle_stroke(at(r, 8.0, 9.75), 2.75 * s, st);
            line(p, r, &[(6.75, 9.75), (7.6, 10.6), (9.25, 8.9)], color);
        }
    }
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
    line(p, r, &[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)], color);
}

pub fn chevron(p: &Painter, r: Rect, left: bool, color: Color32) {
    if left { line(p, r, &[(10.0, 4.5), (6.5, 8.0), (10.0, 11.5)], color) } else { line(p, r, &[(6.0, 4.5), (9.5, 8.0), (6.0, 11.5)], color) }
}

pub fn folder(p: &Painter, r: Rect, color: Color32) {
    line(p, r, &[(2.0, 4.0), (2.0, 12.5), (14.0, 12.5), (14.0, 5.5), (8.0, 5.5), (6.5, 3.5), (2.5, 3.5), (2.0, 4.0)], color);
}

/// A four-pointed sparkle (the AI pill).
pub fn sparkle(p: &Painter, r: Rect, color: Color32) {
    let pts = [(8.0, 2.0), (9.3, 6.7), (14.0, 8.0), (9.3, 9.3), (8.0, 14.0), (6.7, 9.3), (2.0, 8.0), (6.7, 6.7)];
    let poly: Vec<Pos2> = pts.iter().map(|&(x, y)| at(r, x, y)).collect();
    p.add(Shape::convex_polygon(poly, Color32::TRANSPARENT, Stroke::new(STROKE * scale(r), color)));
}

/// The sidebar toggle: a window with its left pane marked.
pub fn sidebar(p: &Painter, r: Rect, color: Color32) {
    let s = scale(r);
    let w = Rect::from_min_max(at(r, 2.0, 3.0), at(r, 14.0, 13.0));
    p.rect_stroke(w, 2.0 * s, Stroke::new(STROKE * s, color), egui::StrokeKind::Middle);
    p.line_segment([at(r, 6.0, 3.0), at(r, 6.0, 13.0)], Stroke::new(STROKE * s, color));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_paint_inside_their_rect() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let r = Rect::from_min_size(pos2(10.0, 10.0), vec2(16.0, 16.0));
            let p = ui.painter().clone();
            for k in Kind::ALL {
                kind(&p, r, k, Color32::RED, Color32::GRAY);
            }
            plus(&p, r, Color32::GRAY);
            close(&p, r, Color32::GRAY);
            chevron_down(&p, r, Color32::GRAY);
            chevron(&p, r, true, Color32::GRAY);
            folder(&p, r, Color32::GRAY);
            sparkle(&p, r, Color32::GRAY);
            sidebar(&p, r, Color32::GRAY);
        });
        out.textures_delta.clear();
        // Drawing must not panic; the grid maps to the rect's square.
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(32.0, 32.0));
        assert_eq!(at(r, 0.0, 0.0), pos2(0.0, 0.0));
        assert_eq!(at(r, 16.0, 16.0), pos2(32.0, 32.0));
    }
}
