//! Design tokens (docs/design.md "Tokens"): colours for light and dark, sizes, radii, motion,
//! and the chrome's fonts and egui style.

use craftworks_core::Kind;
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke};
use std::sync::Arc;

/// The chrome's colours in one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_hover: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub danger: Color32,
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
        surface: rgb(0xF4F4F6),
        surface_raised: rgb(0xFFFFFF),
        surface_hover: rgb(0xE8E8EC),
        hairline: Color32::from_black_alpha(0x14),
        text: rgb(0x1D1D21),
        text_muted: rgb(0x62626C),
        accent: rgb(0x4F6BED),
        danger: rgb(0xD92D20),
        doc: rgb(0x3B82F6),
        sheet: rgb(0x16A34A),
        deck: rgb(0xD9650B),
        pdf: rgb(0xDC3545),
        shadow: Color32::from_black_alpha(31),
    };

    pub const DARK: Palette = Palette {
        dark: true,
        surface: rgb(0x1C1C1F),
        surface_raised: rgb(0x2A2A2E),
        surface_hover: rgb(0x2F2F34),
        hairline: Color32::from_rgba_premultiplied(0x14, 0x14, 0x14, 0x14),
        text: rgb(0xECECEF),
        text_muted: rgb(0x9A9AA5),
        accent: rgb(0x7C8CFF),
        danger: rgb(0xF97066),
        doc: rgb(0x60A5FA),
        sheet: rgb(0x4ADE80),
        deck: rgb(0xFB923C),
        pdf: rgb(0xF87171),
        shadow: Color32::from_black_alpha(102),
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

    /// `c` at `alpha` (0–1) over transparent: a soft tint for hover states.
    pub fn tint(c: Color32, alpha: f32) -> Color32 {
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
    }
}

/// Sizes in points (docs/design.md).
pub mod size {
    /// The title row is never shorter than this; it grows to centre the traffic lights.
    pub const TITLE_ROW_MIN: f32 = 38.0;
    pub const TAB_HEIGHT: f32 = 28.0;
    pub const TAB_MIN: f32 = 120.0;
    pub const TAB_MAX: f32 = 220.0;
    pub const TAB_GAP: f32 = 2.0;
    pub const TAB_ACCENT: f32 = 2.0;
    pub const ICON: f32 = 16.0;
    pub const CARD_W: f32 = 132.0;
    pub const CARD_H: f32 = 112.0;
    pub const PANEL_W: f32 = 600.0;
    pub const ROW_H: f32 = 30.0;
    pub const EDGE_FADE: f32 = 24.0;
}

/// Corner radii.
pub mod radius {
    pub const TAB: u8 = 8;
    pub const CARD: u8 = 12;
    pub const POPOVER: u8 = 12;
    pub const CONTROL: u8 = 6;
}

/// Durations in seconds (zero with Reduce Motion; see [`motion`]).
pub mod motion {
    pub const HOVER: f32 = 0.14;
    pub const MOVE: f32 = 0.20;
    pub const PANEL: f32 = 0.16;

    /// `secs`, or zero when the system asks for reduced motion.
    pub fn dur(secs: f32, reduce_motion: bool) -> f32 {
        if reduce_motion { 0.0 } else { secs }
    }
}

/// The chrome's text styles.
pub mod text {
    use super::*;

    pub fn tab() -> FontId {
        FontId::new(12.5, FontFamily::Proportional)
    }
    pub fn body() -> FontId {
        FontId::new(13.0, FontFamily::Proportional)
    }
    pub fn meta() -> FontId {
        FontId::new(11.5, FontFamily::Proportional)
    }
    pub fn section() -> FontId {
        FontId::new(12.0, FontFamily::Name(SEMIBOLD.into()))
    }
    pub fn title() -> FontId {
        FontId::new(22.0, FontFamily::Name(SEMIBOLD.into()))
    }
    pub fn card() -> FontId {
        FontId::new(13.0, FontFamily::Name(SEMIBOLD.into()))
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
    // Text fields: a recessed fill and a hairline, visible on raised panels.
    visuals.extreme_bg_color = p.surface;
    visuals.text_edit_bg_color = Some(p.surface);
    visuals.faint_bg_color = p.surface_hover;
    visuals.window_stroke = Stroke::new(1.0, p.hairline);
    visuals.window_corner_radius = CornerRadius::same(radius::POPOVER);
    visuals.menu_corner_radius = CornerRadius::same(radius::POPOVER);
    visuals.window_shadow = egui::Shadow { offset: [0, 8], blur: 24, spread: 0, color: p.shadow };
    visuals.popup_shadow = visuals.window_shadow;
    visuals.selection.bg_fill = Palette::tint(p.accent, 0.25);
    visuals.selection.stroke = Stroke::new(2.0, p.accent);
    visuals.hyperlink_color = p.accent;
    visuals.override_text_color = Some(p.text);
    for w in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
        w.corner_radius = CornerRadius::same(radius::CONTROL);
    }
    visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.hairline);
    visuals.widgets.hovered.weak_bg_fill = p.surface_hover;
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.active.weak_bg_fill = p.surface_hover;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    ctx.all_styles_mut(|s| {
        s.visuals = visuals.clone();
        s.interaction.tooltip_delay = 0.6;
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
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
    fn text_meets_wcag_aa_on_every_surface() {
        for p in [Palette::LIGHT, Palette::DARK] {
            for bg in [p.surface, p.surface_raised, p.surface_hover] {
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
    fn the_fonts_load() {
        let f = fonts();
        assert!(f.font_data.contains_key("inter") && f.font_data.contains_key(SEMIBOLD));
        assert_eq!(f.families.get(&FontFamily::Proportional).and_then(|v| v.first()).map(String::as_str), Some("inter"));
    }
}
