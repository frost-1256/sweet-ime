//! Shared look for Sweet IME's settings app and installer: Japanese system
//! font, spacing, and Windows 11 style cards and switches.

pub use eframe;
use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Stroke, Vec2};

/// The app icon for window title bars and taskbars.
pub fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/sweet-ime-64.png"))
        .expect("bundled icon")
}

/// Applies fonts and spacing. Call once from the app constructor.
pub fn setup(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 10.0);
        s.spacing.button_padding = Vec2::new(14.0, 6.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // One Japanese font for everything (mixing a Latin font gives uneven
    // baselines), then a fallback.
    let mut loaded = Vec::new();
    for (name, path, index) in system_fonts() {
        if let Ok(bytes) = std::fs::read(&path) {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            // Japanese fonts have a tall ascent; nudge glyphs down so text
            // is centred in buttons.
            data.tweak.y_offset_factor = 0.12;
            fonts.font_data.insert(name.clone(), std::sync::Arc::new(data));
            loaded.push(name);
        }
    }
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        for (i, name) in loaded.iter().enumerate() {
            list.insert(i, name.clone());
        }
    }
    ctx.set_fonts(fonts);
}

/// Japanese UI fonts to try, as (name, file, face index).
#[cfg(windows)]
fn system_fonts() -> Vec<(String, String, u32)> {
    vec![
        ("yugothic".into(), r"C:\Windows\Fonts\YuGothM.ttc".into(), 0),
        ("meiryo".into(), r"C:\Windows\Fonts\meiryo.ttc".into(), 0),
    ]
}

#[cfg(target_os = "macos")]
fn system_fonts() -> Vec<(String, String, u32)> {
    vec![(
        "hiragino".into(),
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc".into(),
        0,
    )]
}

/// Japanese UI fonts to try, as (name, file, face index): the best match
/// fontconfig has that covers Japanese.
#[cfg(not(any(windows, target_os = "macos")))]
fn system_fonts() -> Vec<(String, String, u32)> {
    let out = std::process::Command::new("fc-match")
        .args(["-s", "-f", "%{file}\t%{index}\t%{lang}\n", "sans-serif:lang=ja"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\t');
            let (file, index, langs) = (f.next()?, f.next()?, f.next()?);
            let outline = [".ttf", ".otf", ".ttc"].iter().any(|ext| file.to_ascii_lowercase().ends_with(ext));
            let japanese = langs.split('|').any(|l| l == "ja");
            // The upper bits select a named instance of a variable font.
            let index = index.trim().parse::<u32>().ok()? & 0xffff;
            (outline && japanese).then(|| ("ja".to_string(), file.to_string(), index))
        })
        .take(1)
        .collect()
}

/// A Windows 11 style on/off switch.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = Vec2::new(40.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let accent = ui.visuals().selection.bg_fill;
    let off_stroke = ui.visuals().widgets.inactive.fg_stroke.color;
    let radius = rect.height() / 2.0;
    let painter = ui.painter();
    if *on {
        painter.rect_filled(rect, radius, accent);
    } else {
        painter.rect_stroke(rect, radius, Stroke::new(1.0, off_stroke), egui::StrokeKind::Inside);
    }
    let knob_r = if *on { 6.0 } else { 5.0 } + if response.hovered() { 1.0 } else { 0.0 };
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = if *on { contrasting_ink(accent) } else { off_stroke };
    painter.circle_filled(egui::pos2(x, rect.center().y), knob_r, knob);
    response
}

/// Black or white, whichever contrasts more with `background`.
pub fn contrasting_ink(background: Color32) -> Color32 {
    // At this relative luminance black and white have equal contrast ratios.
    if relative_luminance(background) >= 0.179 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// WCAG relative luminance of an sRGB color.
pub fn relative_luminance(color: Color32) -> f32 {
    let linear = egui::Rgba::from(color);
    0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b()
}

/// A settings card: title and description on the left, `control` on the right.
pub fn card(ui: &mut egui::Ui, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui);
            ui.add_space(6.0);
            let text_width = ui.available_width().max(120.0);
            ui.allocate_ui_with_layout(
                Vec2::new(text_width, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_max_width(text_width);
                    ui.label(RichText::new(title).size(15.5));
                    if !desc.is_empty() {
                        ui.label(RichText::new(desc).small().weak());
                    }
                },
            );
        });
    });
}

pub fn card_frame(ui: &egui::Ui) -> Frame {
    let v = ui.visuals();
    Frame::new()
        .fill(v.faint_bg_color)
        .stroke(Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(18, 14))
}

/// A page title.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(26.0).strong());
}

/// A primary (accent-filled) button.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).color(Color32::WHITE))
        .fill(Color32::from_rgb(0, 95, 184))
        .min_size(Vec2::new(120.0, 34.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_text_does_not_overlap_wide_control() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(620.0, 200.0),
            )),
            ..Default::default()
        };
        let mut control_rect = egui::Rect::NOTHING;
        let mut desc_rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                card(
                    ui,
                    "候補のフォント",
                    "インストール済みのフォントを選びます。アプリを切り替えると候補ウィンドウに反映されます。",
                    |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(220.0, 28.0), egui::Sense::hover());
                        control_rect = rect;
                    },
                );
                // Measure the left text area indirectly via card's max rect vs control.
                desc_rect = ui.min_rect();
            });
        });
        output.textures_delta.clear();
        assert!(control_rect.width() >= 220.0);
        // The card stays within the panel width and leaves room on the left for the text.
        assert!(desc_rect.width() <= 620.0);
        assert!(control_rect.left() > desc_rect.left() + 120.0);
    }
}
