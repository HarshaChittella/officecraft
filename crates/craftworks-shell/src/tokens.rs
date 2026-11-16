//! Design tokens (docs/design.md "Tokens"): colours for light and dark, sizes, radii, motion,
//! and the chrome's fonts and egui style.

use craftworks_core::Kind;
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke};
use std::sync::Arc;

/// The chrome's colours in one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    /// The title row behind the tabs: the darkest band of the window, as in Office and Acrobat.
    pub band: Color32,
    /// The top of whatever a tab shows (every app's first row has this colour, so the active
    /// tab runs into it), the sidebar and the Home page.
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_hover: Color32,
    /// Recessed wells: text fields, icon tiles, keycaps.
    pub sunken: Color32,
    pub hairline: Color32,
    /// A firmer line than the hairline: outlines of raised things (active tab, cards, popovers).
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    /// Decoration only (placeholder glyphs, separators): not for text that must be read.
    pub text_faint: Color32,
    pub accent: Color32,
    /// Text and icons on an accent fill.
    pub accent_ink: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub doc: Color32,
    pub sheet: Color32,
    pub deck: Color32,
    pub pdf: Color32,
    /// Popover shadow colour.
    pub shadow: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        dark: false,
        band: rgb(0xE2E3E9),
        surface: rgb(0xF6F6F8),
        surface_raised: rgb(0xFFFFFF),
        surface_hover: rgb(0xEAEAEF),
        sunken: rgb(0xEDEDF1),
        hairline: Color32::from_black_alpha(0x16),
        border: Color32::from_black_alpha(0x26),
        text: rgb(0x1A1A1F),
        text_muted: rgb(0x5E5E6A),
        text_faint: rgb(0x9A9AA6),
        accent: rgb(0x3B5BDB),
        accent_ink: rgb(0xFFFFFF),
        danger: rgb(0xC42B1C),
        success: rgb(0x107C41),
        doc: rgb(0x3B5BDB),
        sheet: rgb(0x107C41),
        deck: rgb(0xD95A1A),
        pdf: rgb(0xD93A3A),
        shadow: Color32::from_black_alpha(28),
    };

    pub const DARK: Palette = Palette {
        dark: true,
        band: rgb(0x141416),
        surface: rgb(0x26262A),
        surface_raised: rgb(0x2E2E33),
        surface_hover: rgb(0x37373D),
        sunken: rgb(0x1B1B1E),
        hairline: Color32::from_rgba_premultiplied(0x16, 0x16, 0x16, 0x16),
        border: Color32::from_rgba_premultiplied(0x28, 0x28, 0x28, 0x28),
        text: rgb(0xEDEDF0),
        text_muted: rgb(0xAEAEB8),
        text_faint: rgb(0x6E6E79),
        accent: rgb(0x7A93F0),
        accent_ink: rgb(0x11162E),
        danger: rgb(0xF1705F),
        success: rgb(0x5CC587),
        doc: rgb(0x7A93F0),
        sheet: rgb(0x5CC587),
        deck: rgb(0xF58A4B),
        pdf: rgb(0xF27C7C),
        shadow: Color32::from_black_alpha(120),
    };

    pub fn for_dark(dark: bool) -> Palette {
        if dark { Palette::DARK } else { Palette::LIGHT }
    }

    /// The colour that marks a kind of file.
    pub fn kind(&self, kind: Kind) -> Color32 {
        match kind {
            Kind::Document => self.doc,
            Kind::Sheet => self.sheet,
            Kind::Presentation => self.deck,
            Kind::Pdf => self.pdf,
        }
    }

    /// For Increase contrast: firmer hairlines and hover fills, and secondary text closer to
    /// the primary text.
    pub fn with_contrast(self) -> Palette {
        if self.dark {
            Palette {
                hairline: Color32::from_white_alpha(0x55),
                border: Color32::from_white_alpha(0x80),
                text_muted: rgb(0xD6D6DD),
                text_faint: rgb(0xB4B4BE),
                surface_hover: rgb(0x40404A),
                ..self
            }
        } else {
            Palette {
                hairline: Color32::from_black_alpha(0x4D),
                border: Color32::from_black_alpha(0x80),
                text_muted: rgb(0x3F3F46),
                text_faint: rgb(0x5E5E6A),
                surface_hover: rgb(0xDCDCE2),
                ..self
            }
        }
    }

    /// Tab group colour `i` (wrapping), and the colour of a label written on it.
    pub fn group(&self, i: u8) -> (Color32, Color32) {
        let (fill, light_text) = GROUP_COLORS[usize::from(i) % GROUP_COLORS.len()];
        let fill = if self.dark { rgb(fill.1) } else { rgb(fill.0) };
        let ink = if self.dark || !light_text { rgb(0x1F1F23) } else { rgb(0xFFFFFF) };
        (fill, ink)
    }

    /// `a` blended towards `b` by `t` (0–1), in gamma space: for cross-fading two fills.
    pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
        let t = t.clamp(0.0, 1.0);
        let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
        Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
    }

    /// The ink that reads on a fill of `c`: near-black on light colours, white on dark ones.
    pub fn ink_on(c: Color32) -> Color32 {
        let lum = 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
        if lum > 140.0 { rgb(0x14141A) } else { rgb(0xFFFFFF) }
    }

    /// `c` at `alpha` (0–1) of its own opacity: a soft tint for hover states, or a line fading in.
    pub fn tint(c: Color32, alpha: f32) -> Color32 {
        c.gamma_multiply(alpha.clamp(0.0, 1.0))
    }
}

/// Tab group colours: name, (light, dark) fill, and whether light mode writes white on it.
pub const GROUP_NAMES: [&str; 9] = ["Grey", "Blue", "Red", "Yellow", "Green", "Pink", "Purple", "Cyan", "Orange"];
const GROUP_COLORS: [((u32, u32), bool); 9] = [
    ((0x5F6368, 0xC4C7CC), true),
    ((0x1A66D2, 0x8AB4F8), true),
    ((0xC5221F, 0xF28B82), true),
    ((0xF2B200, 0xFDD663), false),
    ((0x137333, 0x81C995), true),
    ((0xB80672, 0xFF8BCB), true),
    ((0x8430CE, 0xC58AF9), true),
    ((0x00707A, 0x78D9EC), true),
    ((0xF29233, 0xFCAD70), false),
];

/// Sizes in points (docs/design.md).
pub mod size {
    /// The title row is never shorter than this; it grows to centre the traffic lights.
    pub const TITLE_ROW_MIN: f32 = 38.0;
    pub const TAB_HEIGHT: f32 = 28.0;
    pub const TAB_MIN: f32 = 120.0;
    pub const TAB_MAX: f32 = 220.0;
    pub const TAB_GAP: f32 = 0.0;
    pub const TAB_ACCENT: f32 = 2.0;
    pub const ICON: f32 = 16.0;
    pub const CARD_W: f32 = 132.0;
    pub const CARD_H: f32 = 148.0;
    pub const PANEL_W: f32 = 640.0;
    /// The Home page's navigation rail.
    pub const RAIL_W: f32 = 232.0;
    pub const ROW_H: f32 = 34.0;
    pub const EDGE_FADE: f32 = 28.0;
    /// Buttons and fields.
    pub const CONTROL_H: f32 = 28.0;
}

/// The 4 pt spacing grid.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

/// Corner radii.
pub mod radius {
    pub const TAB: u8 = 8;
    pub const CARD: u8 = 10;
    pub const POPOVER: u8 = 12;
    pub const CONTROL: u8 = 6;
    /// Small things inside a control: keycaps, the close button, icon tiles in rows.
    pub const CHIP: u8 = 4;
}

/// Durations in seconds (zero with Reduce Motion; see [`motion::dur`]), and the chrome's two
/// ways of moving: [`motion::fade`] (an eased 0–1 for hover, press and appear) and
/// [`motion::glide`] (a position that eases towards wherever it should be now, and can be
/// redirected mid-flight). Both ask for frames only while something is still moving.
pub mod motion {
    use egui::{Context, Id};

    pub const HOVER: f32 = 0.12;
    pub const MOVE: f32 = 0.24;
    pub const PANEL: f32 = 0.18;
    /// One step of a staggered entrance (cards, rows).
    pub const STAGGER: f32 = 0.035;

    /// `secs`, or zero when the system asks for reduced motion.
    pub fn dur(secs: f32, reduce_motion: bool) -> f32 {
        if reduce_motion { 0.0 } else { secs }
    }

    /// Whether this context has animation off (Reduce Motion).
    pub fn reduced(ctx: &Context) -> bool {
        ctx.global_style().animation_time <= 0.0
    }

    /// 0 → 1 as `on` turns true (and back), eased out over `secs`.
    pub fn fade(ctx: &Context, id: Id, on: bool, secs: f32) -> f32 {
        ctx.animate_bool_with_time_and_easing(id, on, dur(secs, reduced(ctx)), egui::emath::easing::cubic_out)
    }

    /// `target`, approached smoothly: about 99% of the way after `secs`, whatever the frame
    /// rate, and never overshooting. The first call for `id` starts at `target`.
    pub fn glide(ctx: &Context, id: Id, target: f32, secs: f32) -> f32 {
        let secs = dur(secs, reduced(ctx));
        let cur = ctx.data_mut(|d| *d.get_temp_mut_or(id, target));
        let diff = target - cur;
        if secs <= 0.0 || !diff.is_finite() || diff.abs() < 0.25 {
            if cur != target {
                ctx.data_mut(|d| d.insert_temp(id, target));
            }
            return target;
        }
        let dt = ctx.input(|i| i.stable_dt).clamp(0.0, 0.05);
        let next = cur + diff * (1.0 - (-dt * 4.6 / secs).exp());
        ctx.data_mut(|d| d.insert_temp(id, next));
        ctx.request_repaint();
        next
    }

    /// 0 → 1 over `secs` starting `delay` after `since` (both in seconds of `ctx` time),
    /// eased out: for entrances that begin at a known moment.
    pub fn enter(ctx: &Context, since: f64, delay: f32, secs: f32) -> f32 {
        let secs = dur(secs, reduced(ctx));
        if secs <= 0.0 {
            return 1.0;
        }
        let t = ((ctx.input(|i| i.time) - since) as f32 - delay) / secs;
        if t < 1.0 {
            ctx.request_repaint();
        }
        egui::emath::easing::cubic_out(t.clamp(0.0, 1.0))
    }
}

/// The chrome's text styles.
pub mod text {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// The chrome's text size, in percent (View ▸ Interface Text Size).
    static SCALE: AtomicU32 = AtomicU32::new(100);

    /// The text sizes the View menu offers: Default, Large, Larger.
    pub const SCALES: [(u32, &str); 3] = [(100, "Default"), (115, "Large"), (130, "Larger")];

    /// Sets the chrome's text size (percent; kept between 80 and 160).
    pub fn set_scale(percent: u32) {
        SCALE.store(percent.clamp(80, 160), Ordering::Relaxed);
    }

    pub fn scale() -> u32 {
        SCALE.load(Ordering::Relaxed)
    }

    fn sized(pt: f32) -> f32 {
        pt * scale() as f32 / 100.0
    }

    pub fn tab() -> FontId {
        FontId::new(sized(12.5), FontFamily::Proportional)
    }
    pub fn body() -> FontId {
        FontId::new(sized(13.0), FontFamily::Proportional)
    }
    pub fn meta() -> FontId {
        FontId::new(sized(11.5), FontFamily::Proportional)
    }
    pub fn section() -> FontId {
        FontId::new(sized(12.0), FontFamily::Name(SEMIBOLD.into()))
    }
    /// Small capitals over a group of things ("PINNED", "RECENT"); see [`caps`].
    pub fn label() -> FontId {
        FontId::new(sized(10.5), FontFamily::Name(SEMIBOLD.into()))
    }
    pub fn title() -> FontId {
        FontId::new(sized(26.0), FontFamily::Name(SEMIBOLD.into()))
    }
    /// A popover's heading.
    pub fn heading() -> FontId {
        FontId::new(sized(14.0), FontFamily::Name(SEMIBOLD.into()))
    }

    /// `s` in `font` with `tracking` points between letters, on one line: tight for large
    /// titles, loose for small capitals.
    pub fn tracked(s: impl Into<String>, font: FontId, color: Color32, tracking: f32) -> egui::text::LayoutJob {
        let mut job = egui::text::LayoutJob::single_section(
            s.into(),
            egui::TextFormat { font_id: font, color, extra_letter_spacing: tracking, ..Default::default() },
        );
        job.wrap = egui::text::TextWrapping::no_max_width();
        job
    }

    /// A section label: `s` in spaced small capitals.
    pub fn caps(s: &str, color: Color32) -> egui::text::LayoutJob {
        tracked(s.to_uppercase(), label(), color, 0.7)
    }
    pub fn card() -> FontId {
        FontId::new(sized(13.0), FontFamily::Name(SEMIBOLD.into()))
    }
}

/// The font family name of Inter SemiBold.
pub const SEMIBOLD: &str = "inter-semibold";

/// Inter (OFL-1.1, from craft-fonts, the Craft apps' interface face) as the chrome's font,
/// with egui's defaults after it for anything Inter lacks.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("inter".into(), Arc::new(FontData::from_static(include_bytes!("../../../vendor/craft-fonts/fonts/inter/Inter-Regular.ttf"))));
    fonts
        .font_data
        .insert(SEMIBOLD.into(), Arc::new(FontData::from_static(include_bytes!("../../../vendor/craft-fonts/fonts/inter/Inter-SemiBold.ttf"))));
    let fallbacks: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut regular = vec!["inter".to_string()];
    regular.extend(fallbacks.iter().cloned());
    let mut semibold = vec![SEMIBOLD.to_string()];
    semibold.extend(fallbacks);
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts
}

/// Styles the chrome's egui context for `p`. The chrome is the window's context: it keeps no
/// keyboard zoom (⌘+ and ⌘− belong to the document) and no quit shortcut of its own.
pub fn apply(ctx: &egui::Context, p: &Palette) {
    ctx.options_mut(|o| {
        o.zoom_with_keyboard = false;
        o.quit_shortcuts.clear();
    });
    let mut visuals = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.panel_fill = p.surface;
    visuals.window_fill = p.surface_raised;
    // Text fields: a recessed well, visible on raised panels and on the sidebar.
    visuals.extreme_bg_color = p.sunken;
    visuals.text_edit_bg_color = Some(p.sunken);
    visuals.faint_bg_color = p.surface_hover;
    visuals.window_stroke = Stroke::new(1.0, p.border);
    visuals.window_corner_radius = CornerRadius::same(radius::POPOVER);
    visuals.menu_corner_radius = CornerRadius::same(radius::POPOVER - 4);
    visuals.window_shadow = egui::Shadow { offset: [0, 12], blur: 32, spread: 0, color: p.shadow };
    visuals.popup_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow };
    visuals.selection.bg_fill = Palette::tint(p.accent, 0.28);
    visuals.selection.stroke = Stroke::new(1.5, p.accent);
    visuals.hyperlink_color = p.accent;
    visuals.override_text_color = Some(p.text);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
        w.corner_radius = CornerRadius::same(radius::CONTROL);
        w.expansion = 0.0;
    }
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_fill = p.sunken;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.hairline);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.hovered.weak_bg_fill = p.surface_hover;
    visuals.widgets.hovered.bg_fill = p.surface_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.active.weak_bg_fill = Palette::mix(p.surface_hover, p.text, 0.08);
    visuals.widgets.active.bg_fill = visuals.widgets.active.weak_bg_fill;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
    visuals.widgets.open.weak_bg_fill = p.surface_hover;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, p.border);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text_muted);
    ctx.all_styles_mut(|s| {
        s.visuals = visuals.clone();
        s.interaction.tooltip_delay = 0.6;
        s.spacing.item_spacing = egui::vec2(space::S, space::XS);
        s.spacing.button_padding = egui::vec2(space::M, 5.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.interact_size.y = 26.0;
        s.text_styles.insert(egui::TextStyle::Body, text::body());
        s.text_styles.insert(egui::TextStyle::Button, text::body());
        s.text_styles.insert(egui::TextStyle::Small, text::meta());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG relative luminance of an sRGB colour.
    fn luminance(c: Color32) -> f32 {
        let f = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn increased_contrast_is_at_least_as_readable() {
        for p in [Palette::LIGHT, Palette::DARK] {
            let c = p.with_contrast();
            for bg in [c.surface, c.surface_raised, c.surface_hover] {
                assert!(contrast(c.text_muted, bg) >= 7.0, "muted text in increased contrast (dark={}): {:.2}", p.dark, contrast(c.text_muted, bg));
                assert!(contrast(c.text_muted, bg) >= contrast(p.text_muted, bg) - 0.01);
            }
        }
    }

    #[test]
    fn text_sizes_scale_and_stay_in_range() {
        text::set_scale(130);
        assert!((text::body().size - 13.0 * 1.3).abs() < 0.01);
        text::set_scale(500);
        assert_eq!(text::scale(), 160);
        text::set_scale(100);
        assert!((text::body().size - 13.0).abs() < 0.01);
    }

    #[test]
    fn group_labels_are_readable_on_their_colour() {
        for p in [Palette::LIGHT, Palette::DARK] {
            for i in 0..GROUP_NAMES.len() as u8 {
                let (fill, ink) = p.group(i);
                let c = contrast(ink, fill);
                assert!(c >= 4.5, "{} label (dark={}): {c:.2}", GROUP_NAMES[usize::from(i)], p.dark);
            }
        }
    }

    #[test]
    fn text_meets_wcag_aa_on_every_surface() {
        for p in [Palette::LIGHT, Palette::DARK] {
            for bg in [p.band, p.surface, p.surface_raised, p.surface_hover] {
                assert!(contrast(p.text, bg) >= 4.5, "text on {bg:?} (dark={})", p.dark);
                assert!(contrast(p.text_muted, bg) >= 4.5, "muted text on {bg:?} (dark={})", p.dark);
            }
        }
    }

    #[test]
    fn kind_colours_and_the_accent_read_as_icons() {
        for p in [Palette::LIGHT, Palette::DARK] {
            for c in [p.doc, p.sheet, p.deck, p.pdf, p.accent] {
                assert!(contrast(c, p.surface_raised) >= 3.0, "{c:?} on raised (dark={})", p.dark);
            }
        }
    }

    #[test]
    fn ink_on_the_accent_and_the_kind_colours_reads() {
        for p in [Palette::LIGHT, Palette::DARK] {
            assert!(contrast(p.accent_ink, p.accent) >= 4.5, "accent ink (dark={}): {:.2}", p.dark, contrast(p.accent_ink, p.accent));
            for c in [p.doc, p.sheet, p.deck, p.pdf] {
                assert!(contrast(Palette::ink_on(c), c) >= 3.0, "glyph on {c:?} (dark={}): {:.2}", p.dark, contrast(Palette::ink_on(c), c));
            }
            for bg in [p.surface, p.surface_raised] {
                assert!(contrast(p.success, bg) >= 3.0 && contrast(p.danger, bg) >= 3.0);
            }
        }
    }

    /// A glide reaches its target and then stops asking for frames; with motion reduced it is
    /// there at once.
    #[test]
    fn a_glide_settles_and_goes_quiet() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("cw_test_glide");
        let (mut t, mut x, mut asked) = (0.0, 0.0, true);
        for frame in 0..90 {
            t += 1.0 / 60.0;
            let mut out = ctx.run_ui(egui::RawInput { time: Some(t), ..Default::default() }, |ui| {
                x = motion::glide(ui.ctx(), id, if frame == 0 { 0.0 } else { 100.0 }, motion::MOVE);
            });
            out.textures_delta.clear();
            asked = out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay < std::time::Duration::from_secs(1));
            assert!((0.0..=100.0).contains(&x), "no overshoot: {x}");
        }
        assert_eq!(x, 100.0);
        assert!(!asked, "still asking for frames after it arrived");
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| x = motion::glide(ui.ctx(), id, 7.0, motion::MOVE));
        out.textures_delta.clear();
        assert_eq!(x, 7.0);
    }

    #[test]
    fn the_fonts_load() {
        let f = fonts();
        assert!(f.font_data.contains_key("inter") && f.font_data.contains_key(SEMIBOLD));
        assert_eq!(f.families.get(&FontFamily::Proportional).and_then(|v| v.first()).map(String::as_str), Some("inter"));
    }
}
