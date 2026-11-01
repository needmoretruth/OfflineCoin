//! Which colours apply, and the toolkit's defaults set to them.
use super::tokens::{self, Tokens, DARK, LIGHT};
use crate::settings::Theme;
use egui::epaint::Shadow;
use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Stroke, Style, TextStyle};
use olc_service::platform;
use std::sync::OnceLock;

/// The desktop's own setting, for when the toolkit cannot tell.
fn desktop_prefers_dark() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| platform().prefers_dark())
}

/// egui does not report a reduced-motion preference, so the desktop's setting is read once.
pub fn desktop_reduces_motion() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| platform().reduces_motion())
}

pub fn tokens_for(ctx: &Context, theme: Theme) -> Tokens {
    let dark = match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => ctx
            .system_theme()
            .map_or_else(desktop_prefers_dark, |theme| theme == egui::Theme::Dark),
    };
    if dark {
        DARK
    } else {
        LIGHT
    }
}

fn set_text(style: &mut Style) {
    use TextStyle::{Body, Button, Heading, Monospace, Small};
    let regular = FontFamily::Proportional;
    style.text_styles = [
        (Small, FontId::new(tokens::META, regular.clone())),
        (Body, FontId::new(tokens::TEXT, regular.clone())),
        (Button, FontId::new(tokens::TEXT, regular)),
        (Heading, FontId::new(tokens::TITLE, super::fonts::strong())),
        (Monospace, FontId::new(tokens::META, FontFamily::Monospace)),
    ]
    .into();
}

pub fn apply(ctx: &Context, t: &Tokens) {
    ctx.global_style_mut(|style| {
        set_text(style);
        style.spacing.item_spacing = egui::vec2(tokens::GAP, tokens::GAP);
        style.spacing.button_padding = egui::vec2(12.0, 7.0);
        style.spacing.interact_size.y = tokens::CONTROL;
        style.spacing.text_edit_width = tokens::FIELD;
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = true;
        style.animation_time = 0.12;

        let visuals = &mut style.visuals;
        *visuals = if t.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = t.back_bottom;
        visuals.window_fill = t.card_top;
        visuals.extreme_bg_color = t.card_bottom;
        visuals.faint_bg_color = t.card_bottom;
        visuals.override_text_color = Some(t.text);
        visuals.hyperlink_color = t.info;
        visuals.selection.bg_fill = t.line_strong.gamma_multiply(0.6);
        visuals.selection.stroke = Stroke::new(1.0, t.text);
        visuals.text_cursor.stroke = Stroke::new(1.5, t.text);
        visuals.window_stroke = Stroke::new(1.0, t.line);
        visuals.window_corner_radius = CornerRadius::same(tokens::RADIUS_CARD);
        visuals.menu_corner_radius = CornerRadius::same(tokens::RADIUS_CONTROL);
        visuals.popup_shadow = Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: t.shadow,
        };
        visuals.window_shadow = visuals.popup_shadow;
        let round = CornerRadius::same(tokens::RADIUS_CONTROL);
        let sets = [
            (&mut visuals.widgets.noninteractive, Color32::TRANSPARENT, t.line),
            (&mut visuals.widgets.inactive, t.raised, t.line),
            (&mut visuals.widgets.hovered, t.hover, t.line_strong),
            (&mut visuals.widgets.active, t.hover, t.line_strong),
            (&mut visuals.widgets.open, t.hover, t.line_strong),
        ];
        for (set, fill, line) in sets {
            set.bg_fill = fill;
            set.weak_bg_fill = fill;
            set.bg_stroke = Stroke::new(1.0, line);
            set.fg_stroke = Stroke::new(1.0, t.text);
            set.corner_radius = round;
            set.expansion = 0.0;
        }
    });
}
