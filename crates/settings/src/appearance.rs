use sweet_engine::{Theme, UserTheme, theme::Rgb};
use sweet_ui::eframe::egui::{self, Color32, CornerRadius, Stroke, Ui, Vec2};

pub(super) struct Appearance {
    pub background: Color32,
    pub sidebar: Color32,
    pub border: Color32,
    accent: Color32,
    selected: Color32,
}

pub(super) fn system_light(ui: &Ui) -> bool {
    let ctx = ui.ctx();
    ctx.system_theme().unwrap_or_else(|| ctx.theme()) == egui::Theme::Light
}

pub(super) fn apply(ui: &mut Ui, theme: Theme, user_theme: &UserTheme) -> Appearance {
    let colors = if theme == Theme::Custom {
        user_theme.colors()
    } else {
        theme.colors(system_light(ui))
    };
    let background = color(colors.bg);
    let text = color(colors.text);
    let border = color(colors.border);
    let accent = colors
        .accent
        .map(color)
        .unwrap_or_else(|| system_accent(colors.dark));
    let selected = background.lerp_to_gamma(color(colors.hover), colors.hover_alpha);
    let sidebar = background.lerp_to_gamma(text, 0.035);
    let surface = background.lerp_to_gamma(Color32::WHITE, if colors.dark { 0.045 } else { 0.8 });

    let style = ui.style_mut();
    for (role, size) in [
        (egui::TextStyle::Body, 16.0),
        (egui::TextStyle::Button, 16.0),
        (egui::TextStyle::Small, 14.0),
    ] {
        style
            .text_styles
            .insert(role, egui::FontId::proportional(size));
    }

    let visuals = &mut style.visuals;
    *visuals = if colors.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = background;
    visuals.window_fill = surface;
    visuals.window_stroke = Stroke::new(1.0, border);
    visuals.window_corner_radius = CornerRadius::same(8);
    visuals.faint_bg_color = surface;
    visuals.extreme_bg_color = background;
    visuals.weak_text_color = Some(color(colors.sub).lerp_to_gamma(text, 0.35));
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    visuals.widgets.inactive.bg_fill = surface;
    visuals.widgets.inactive.weak_bg_fill = surface;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    visuals.widgets.hovered.bg_fill = selected;
    visuals.widgets.hovered.weak_bg_fill = selected;
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.fg_stroke.color = text;
    }
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke.color = sweet_ui::contrasting_ink(accent);

    Appearance {
        background,
        sidebar,
        border,
        accent,
        selected,
    }
}

impl Appearance {
    pub fn navigation_item(&self, ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
        let label = egui::RichText::new(label).color(ui.visuals().text_color());
        let mut button = egui::Button::selectable(selected, label)
            .min_size(Vec2::new(ui.available_width(), 34.0))
            .corner_radius(CornerRadius::same(7))
            .frame_when_inactive(false);
        if selected {
            button = button.fill(self.selected).stroke(Stroke::NONE);
        }
        let response = ui.add(button);
        if selected {
            let marker = egui::Rect::from_center_size(
                egui::pos2(response.rect.left() + 6.0, response.rect.center().y),
                Vec2::new(3.0, 16.0),
            );
            ui.painter()
                .rect_filled(marker, CornerRadius::same(2), self.accent);
        }
        response
    }
}

fn color(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.0, rgb.1, rgb.2)
}

/// System accent color, using the Windows accent palette when available.
pub(crate) fn system_accent(dark: bool) -> Color32 {
    #[cfg(windows)]
    if let Some(accent) = windows_accent(dark) {
        return accent;
    }
    if dark {
        Color32::from_rgb(96, 205, 255)
    } else {
        Color32::from_rgb(0, 95, 184)
    }
}

#[cfg(windows)]
fn windows_accent(dark: bool) -> Option<Color32> {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW,
    };
    use windows::core::w;

    // AccentPalette stores 8 RGBA entries (Light3, Light2, Light1, Base, Dark1, Dark2, Dark3, ...).
    // Windows 11 uses Light2 (index 1) on dark surfaces and Dark1 (index 4) on light surfaces.
    let mut palette = [0u8; 32];
    let mut size = palette.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Accent"),
            w!("AccentPalette"),
            RRF_RT_REG_BINARY,
            None,
            Some(palette.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if status.is_ok() && size >= 20 {
        let offset = if dark { 4 } else { 16 };
        return Some(Color32::from_rgb(
            palette[offset],
            palette[offset + 1],
            palette[offset + 2],
        ));
    }

    // Fallback to DWM AccentColor (stored as 0xAABBGGRR).
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    if status.is_ok() {
        let (r, g, b) = (
            (value & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            ((value >> 16) & 0xff) as u8,
        );
        return Some(if dark {
            let lighten = |c: u8| c + ((255 - c) as f32 * 0.45) as u8;
            Color32::from_rgb(lighten(r), lighten(g), lighten(b))
        } else {
            Color32::from_rgb(r, g, b)
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_text_uses_readable_sizes() {
        let ctx = egui::Context::default();
        let ut = UserTheme::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            apply(ui, Theme::Default, &ut);
            for (role, minimum) in [
                (egui::TextStyle::Body, 16.0),
                (egui::TextStyle::Button, 16.0),
                (egui::TextStyle::Small, 14.0),
            ] {
                assert!(role.resolve(ui.style()).size >= minimum, "{role:?}");
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn settings_use_the_candidate_palette_in_light_and_dark_modes() {
        let ut = UserTheme::default();
        for system_theme in [egui::Theme::Light, egui::Theme::Dark] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                system_theme: Some(system_theme),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                assert_eq!(system_light(ui), system_theme == egui::Theme::Light);
                for theme in Theme::ALL {
                    let expected = if theme == Theme::Custom {
                        ut.colors()
                    } else {
                        theme.colors(system_theme == egui::Theme::Light)
                    };
                    let appearance = apply(ui, theme, &ut);
                    assert_eq!(appearance.background, color(expected.bg), "{theme:?}");
                    assert_eq!(appearance.border, color(expected.border), "{theme:?}");
                    assert_eq!(ui.visuals().text_color(), color(expected.text), "{theme:?}");
                    assert_eq!(ui.visuals().dark_mode, expected.dark, "{theme:?}");
                }
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn description_text_has_sufficient_contrast_on_every_theme() {
        let ut = UserTheme::default();
        for system_theme in [egui::Theme::Light, egui::Theme::Dark] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                system_theme: Some(system_theme),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                for theme in Theme::ALL {
                    apply(ui, theme, &ut);
                    let visuals = ui.visuals();
                    let text = luminance(visuals.weak_text_color());
                    let card = luminance(visuals.faint_bg_color);
                    let contrast = (text.max(card) + 0.05) / (text.min(card) + 0.05);
                    assert!(
                        contrast >= 4.5,
                        "{theme:?} on {system_theme:?}: {contrast:.2}"
                    );
                }
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn selected_text_and_switch_thumbs_contrast_with_the_accent() {
        let ut = UserTheme::default();
        for system_theme in [egui::Theme::Light, egui::Theme::Dark] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                system_theme: Some(system_theme),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                for theme in Theme::ALL {
                    apply(ui, theme, &ut);
                    let selection = ui.visuals().selection;
                    let accent = selection.bg_fill;
                    for ink in [selection.stroke.color, sweet_ui::contrasting_ink(accent)] {
                        let contrast = contrast(luminance(ink), luminance(accent));
                        assert!(
                            contrast >= 4.5,
                            "{theme:?} on {system_theme:?}: {contrast:.2}"
                        );
                    }
                }
            });
            output.textures_delta.clear();
        }
    }

    fn contrast(a: f32, b: f32) -> f32 {
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn luminance(color: Color32) -> f32 {
        sweet_ui::relative_luminance(color)
    }
}
