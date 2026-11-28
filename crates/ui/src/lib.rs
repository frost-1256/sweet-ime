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
        s.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        s.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        s.text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        s.text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.5));
    });
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Keep the Japanese UI fonts first for consistent text metrics.
    let mut loaded = Vec::new();
    let mut font_files = std::collections::HashMap::new();
    for (name, path, index) in system_fonts() {
        let data = font_files
            .entry(path)
            .or_insert_with_key(|path| std::fs::read(path).ok().map(egui::FontData::from_owned));
        if let Some(mut data) = data.clone() {
            data.index = index;
            if matches!(name.as_str(), "yugothic" | "meiryo" | "hiragino" | "ja") {
                // Adjust the primary fonts for vertical alignment in buttons.
                data.tweak.y_offset_factor = 0.12;
            }
            fonts
                .font_data
                .insert(name.clone(), std::sync::Arc::new(data));
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

/// UI fonts in fallback order, as (name, file, face index).
#[cfg(windows)]
fn system_fonts() -> Vec<(String, String, u32)> {
    let directory = std::env::var_os("WINDIR")
        .map_or_else(
            || std::path::PathBuf::from(r"C:\Windows"),
            std::path::PathBuf::from,
        )
        .join("Fonts");
    [
        ("yugothic", "YuGothM.ttc"),
        ("meiryo", "meiryo.ttc"),
        ("yahei", "msyh.ttc"),
        ("jhenghei", "msjh.ttc"),
        ("malgun", "malgun.ttf"),
        ("emoji", "seguiemj.ttf"),
        ("segoe", "segoeui.ttf"),
        ("nirmala", "Nirmala.ttc"),
        ("leelawadee", "LeelawUI.ttf"),
        ("ebrima", "ebrima.ttf"),
        ("segoe_symbol", "seguisym.ttf"),
    ]
    .into_iter()
    .map(|(name, file)| {
        (
            name.into(),
            directory.join(file).to_string_lossy().into_owned(),
            0,
        )
    })
    .collect()
}

#[cfg(target_os = "macos")]
fn system_fonts() -> Vec<(String, String, u32)> {
    [
        ("hiragino", "ヒラギノ角ゴシック W3.ttc"),
        ("pingfang", "PingFang.ttc"),
        ("songti", "Supplemental/Songti.ttc"),
        ("korean", "AppleSDGothicNeo.ttc"),
        ("emoji", "Apple Color Emoji.ttc"),
        ("arial", "Supplemental/Arial.ttf"),
        ("arabic", "GeezaPro.ttc"),
        ("hebrew", "Supplemental/ArialHB.ttc"),
        ("devanagari", "Supplemental/Devanagari Sangam MN.ttc"),
        ("bangla", "Supplemental/Bangla Sangam MN.ttc"),
        ("thai", "Thonburi.ttc"),
        ("khmer", "Supplemental/Khmer Sangam MN.ttf"),
        ("ethiopic", "Supplemental/Kefa.ttc"),
    ]
    .into_iter()
    .map(|(name, file)| (name.into(), format!("/System/Library/Fonts/{file}"), 0))
    .collect()
}

/// Use fontconfig's ordered fallback chain, prioritizing its Japanese match.
#[cfg(not(any(windows, target_os = "macos")))]
fn system_fonts() -> Vec<(String, String, u32)> {
    let out = std::process::Command::new("fc-match")
        .args([
            "-s",
            "-f",
            "%{file}\t%{index}\t%{lang}\n",
            "sans-serif:lang=ja",
        ])
        .output();
    match out {
        Ok(out) if out.status.success() => fontconfig_fonts(&String::from_utf8_lossy(&out.stdout)),
        _ => Vec::new(),
    }
}

#[cfg(any(test, not(any(windows, target_os = "macos"))))]
fn fontconfig_fonts(output: &str) -> Vec<(String, String, u32)> {
    let mut seen = std::collections::HashSet::new();
    let mut has_japanese_match = false;
    let mut fonts: Vec<_> = output
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let mut f = line.split('\t');
            let (file, index, langs) = (f.next()?, f.next()?, f.next()?);
            let supported = [".ttf", ".otf", ".ttc", ".otc"]
                .iter()
                .any(|ext| file.to_ascii_lowercase().ends_with(ext));
            // The upper bits select a named instance of a variable font.
            let index = index.trim().parse::<u32>().ok()? & 0xffff;
            if !supported || !seen.insert((file, index)) {
                return None;
            }
            let name = if !has_japanese_match && langs.split('|').any(|lang| lang == "ja") {
                has_japanese_match = true;
                "ja".into()
            } else {
                format!("fontconfig-{i}")
            };
            Some((name, file.to_owned(), index))
        })
        .collect();
    if let Some(index) = fonts.iter().position(|(name, _, _)| name == "ja") {
        let primary = fonts.remove(index);
        fonts.insert(0, primary);
    }
    fonts
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
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.0, off_stroke),
            egui::StrokeKind::Inside,
        );
    }
    let knob_r = if *on { 6.0 } else { 5.0 } + if response.hovered() { 1.0 } else { 0.0 };
    let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob = if *on {
        contrasting_ink(accent)
    } else {
        off_stroke
    };
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
    card_inner(ui, title, desc, control);
}

/// A subtle section heading placed above a group of related settings.
pub fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(13.5).strong().weak());
}

/// A grouped settings card that holds multiple [`setting_row`] entries
/// separated by [`setting_divider`] inside a single rounded container.
pub fn card_group(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    let v = ui.visuals();
    Frame::new()
        .fill(v.faint_bg_color)
        .stroke(Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(18, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            content(ui);
        });
}

/// One row inside a [`card_group`]: title and description on the left,
/// `control` on the right, vertically centered against the full row height.
pub fn setting_row(
    ui: &mut egui::Ui,
    title: &str,
    desc: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    setting_row_inner(ui, title, desc, control);
}

/// One row inside a [`card_group`] with custom left-hand content (such as
/// title + badges + multi-line description) and a vertically centered right
/// `control`.
pub fn setting_row_custom(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    left: impl FnOnce(&mut egui::Ui),
    control: impl FnOnce(&mut egui::Ui),
) {
    Frame::new()
        .inner_margin(Margin::symmetric(0, 12))
        .show(ui, |ui| {
            balanced_row(ui, id_salt, 44.0, left, control);
        });
}

/// A subtle horizontal hairline between two [`setting_row`] items in a [`card_group`].
pub fn setting_divider(ui: &mut egui::Ui) {
    let color = ui.visuals().widgets.noninteractive.bg_stroke.color;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.center().y,
        Stroke::new(1.0, color),
    );
}

/// Lays out a row with `left` content and a right-aligned `control`, keeping
/// top/bottom padding symmetrical and vertically centering the shorter side
/// against the full row height.
fn balanced_row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    estimated_height: f32,
    left: impl FnOnce(&mut egui::Ui),
    control: impl FnOnce(&mut egui::Ui),
) -> (egui::Rect, egui::Rect, egui::Rect) {
    let row_id = ui.make_persistent_id((id_salt, ui.next_auto_id()));
    let interact_h = ui.spacing().interact_size.y;
    let (cached_row_h, cached_left_h, cached_ctrl_h) = ui
        .data(|d| d.get_temp::<(f32, f32, f32)>(row_id))
        .unwrap_or((estimated_height, estimated_height, interact_h));
    let mut text_rect = egui::Rect::NOTHING;
    let mut control_rect = egui::Rect::NOTHING;
    let mut left_content_h = 0.0f32;
    let mut ctrl_content_h = 0.0f32;
    let row_w = ui.available_width();
    let row_size = Vec2::new(row_w, cached_row_h);
    let ctrl_pad = ((cached_row_h - cached_ctrl_h).max(0.0) * 0.5).floor();
    let left_pad = ((cached_row_h - cached_left_h).max(0.0) * 0.5).floor();
    let outer = ui.allocate_ui_with_layout(
        row_size,
        egui::Layout::right_to_left(egui::Align::Min),
        |ui| {
            let max_ctrl_rect = ui.available_rect_before_wrap();
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(max_ctrl_rect)
                    .layout(egui::Layout::top_down(egui::Align::RIGHT)),
                |ui| {
                    let prev_y = ui.spacing().item_spacing.y;
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if ctrl_pad > 0.0 {
                        ui.add_space(ctrl_pad);
                    }
                    let mut inner_max = ui.available_rect_before_wrap();
                    inner_max.max.y = inner_max.min.y;
                    let inner = ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(inner_max)
                            .layout(egui::Layout::right_to_left(egui::Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.y = prev_y;
                            control(ui);
                        },
                    );
                    control_rect = inner.response.rect;
                    ctrl_content_h = control_rect.height();
                },
            );
            ui.add_space(12.0);
            let text_width = ui.available_width().max(0.0);
            ui.allocate_ui_with_layout(
                Vec2::new(text_width, 0.0),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_max_width(text_width);
                    let prev_y = ui.spacing().item_spacing.y;
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if left_pad > 0.0 {
                        ui.add_space(left_pad);
                    }
                    let inner = ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.y = prev_y;
                        ui.set_max_width(text_width);
                        left(ui);
                    });
                    text_rect = inner.response.rect;
                    left_content_h = text_rect.height();
                },
            );
        },
    );
    let actual_row_h = left_content_h.max(ctrl_content_h).max(interact_h);
    if (actual_row_h - cached_row_h).abs() > 0.5
        || (left_content_h - cached_left_h).abs() > 0.5
        || (ctrl_content_h - cached_ctrl_h).abs() > 0.5
    {
        ui.data_mut(|d| d.insert_temp(row_id, (actual_row_h, left_content_h, ctrl_content_h)));
        ui.ctx().request_repaint();
    }
    (text_rect, control_rect, outer.response.rect)
}

fn setting_row_inner(
    ui: &mut egui::Ui,
    title: &str,
    desc: &str,
    control: impl FnOnce(&mut egui::Ui),
) -> (egui::Rect, egui::Rect, egui::Rect) {
    let est = if desc.is_empty() { 24.0 } else { 44.0 };
    let mut rects = (
        egui::Rect::NOTHING,
        egui::Rect::NOTHING,
        egui::Rect::NOTHING,
    );
    Frame::new()
        .inner_margin(Margin::symmetric(0, 12))
        .show(ui, |ui| {
            rects = balanced_row(
                ui,
                ("setting_row", title),
                est,
                |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new(title).size(15.5));
                    if !desc.is_empty() {
                        ui.label(RichText::new(desc).small().weak());
                    }
                },
                control,
            );
        });
    rects
}

fn card_inner(
    ui: &mut egui::Ui,
    title: &str,
    desc: &str,
    control: impl FnOnce(&mut egui::Ui),
) -> (egui::Rect, egui::Rect) {
    let est = if desc.is_empty() { 24.0 } else { 44.0 };
    let mut text_rect = egui::Rect::NOTHING;
    let frame_response = card_frame(ui).show(ui, |ui| {
        let (t, _, _) = balanced_row(
            ui,
            ("card", title),
            est,
            |ui| {
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.label(RichText::new(title).size(15.5));
                if !desc.is_empty() {
                    ui.label(RichText::new(desc).small().weak());
                }
            },
            control,
        );
        text_rect = t;
    });
    (text_rect, frame_response.response.rect)
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

/// A compact accent-filled button matching the active theme's accent color.
pub fn accent_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    let fill = accent_button_fill(ui.visuals().selection.bg_fill);
    egui::Button::new(RichText::new(text.to_owned()).color(Color32::WHITE)).fill(fill)
}

fn accent_button_fill(accent: Color32) -> Color32 {
    // White text needs at least 4.5:1 contrast. Find the lightest shade
    // along the accent-to-black ramp that meets it after sRGB rounding.
    const MAX_LUMINANCE: f32 = 1.05 / 4.5 - 0.05;
    if relative_luminance(accent) <= MAX_LUMINANCE {
        return accent;
    }
    let mut lower = 0;
    let mut upper = 255;
    while upper - lower > 1 {
        let middle = (lower + upper) / 2;
        let shade = Color32::BLACK.lerp_to_gamma(accent, middle as f32 / 255.0);
        if relative_luminance(shade) <= MAX_LUMINANCE {
            lower = middle;
        } else {
            upper = middle;
        }
    }
    Color32::BLACK.lerp_to_gamma(accent, lower as f32 / 255.0)
}

/// A primary (accent-filled) button matching the active theme's accent color.
pub fn primary_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    accent_button(ui, text).min_size(Vec2::new(120.0, 34.0))
}

#[cfg(test)]
mod tests {
    #[test]
    fn fontconfig_keeps_multilingual_fallbacks_after_japanese() {
        let fonts = super::fontconfig_fonts(concat!(
            "/fonts/Arabic.ttf\t0\tar\n",
            "/fonts/CJK.ttc\t65538\ten|ja\n",
            "/fonts/CJK.ttc\t2\tja\n",
            "/fonts/CJK.ttc\t3\tko\n",
            "/fonts/Emoji.ttf\t0\t\n",
            "/fonts/Bitmap.pcf\t0\tth\n",
        ));
        assert_eq!(
            fonts
                .iter()
                .map(|(_, path, index)| (path.as_str(), *index))
                .collect::<Vec<_>>(),
            [
                ("/fonts/CJK.ttc", 2),
                ("/fonts/Arabic.ttf", 0),
                ("/fonts/CJK.ttc", 3),
                ("/fonts/Emoji.ttf", 0),
            ],
        );
    }

    #[cfg(windows)]
    #[test]
    fn settings_fonts_cover_multilingual_dictionary_entries() {
        let ctx = egui::Context::default();
        super::install_fonts(&ctx);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            ui.ctx().fonts_mut(|fonts| {
                let font = egui::FontId::proportional(15.0);
                for sample in [
                    "闲鱼",
                    "臺灣",
                    "한글",
                    "😀",
                    "Tiếng Việt",
                    "Ελληνικά",
                    "Кириллица",
                    "العربية",
                    "עברית",
                    "हिन्दी",
                    "বাংলা",
                    "ไทย",
                    "ភាសាខ្មែរ",
                    "አማርኛ",
                ] {
                    assert!(
                        fonts.has_glyphs(&font, sample),
                        "Missing glyphs for {sample}"
                    );
                }
            });
        });
        output.textures_delta.clear();
    }

    use super::*;

    #[test]
    fn accent_buttons_keep_white_text_readable() {
        let dark_blue = Color32::from_rgb(26, 70, 140);
        assert_eq!(accent_button_fill(dark_blue), dark_blue);
        for accent in [
            Color32::from_rgb(255, 117, 198),
            Color32::YELLOW,
            Color32::WHITE,
        ] {
            let fill = accent_button_fill(accent);
            assert!(1.05 / (relative_luminance(fill) + 0.05) >= 4.5);
            assert_ne!(fill, Color32::BLACK);
            assert_ne!(fill, accent);
        }
    }

    #[test]
    fn card_text_does_not_overlap_wide_control() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(620.0, 400.0),
            )),
            ..Default::default()
        };
        let mut control_rect = egui::Rect::NOTHING;
        let mut text_rect = egui::Rect::NOTHING;
        let mut card_rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let (t_rect, c_rect) = card_inner(
                    ui,
                    "候補のフォント",
                    "インストール済みのフォントを選びます。アプリを切り替えると候補ウィンドウに反映されます。",
                    |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(220.0, 28.0), egui::Sense::hover());
                        control_rect = rect;
                    },
                );
                text_rect = t_rect;
                card_rect = c_rect;
            });
        });
        output.textures_delta.clear();
        assert!(control_rect.width() >= 220.0);
        // The title/description block stays within the card and to the left of the control.
        assert!(
            text_rect.left() >= card_rect.left(),
            "text_rect={text_rect:?} overflowed left of card_rect={card_rect:?}"
        );
        assert!(
            text_rect.right() <= control_rect.left(),
            "text_rect={text_rect:?} overlapped control_rect={control_rect:?}"
        );
        // The card stays compact instead of inheriting the full 400px panel height.
        assert!(
            card_rect.height() < 140.0,
            "card_rect stretched unexpectedly: {card_rect:?}"
        );
    }

    #[test]
    fn card_text_stays_inside_narrow_card() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(320.0, 400.0),
            )),
            ..Default::default()
        };
        let mut control_rect = egui::Rect::NOTHING;
        let mut text_rect = egui::Rect::NOTHING;
        let mut card_rect = egui::Rect::NOTHING;
        let mut output = ctx.run_ui(input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let (t_rect, c_rect) =
                    card_inner(ui, "候補のフォント", "説明テキスト", |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(220.0, 28.0), egui::Sense::hover());
                        control_rect = rect;
                    });
                text_rect = t_rect;
                card_rect = c_rect;
            });
        });
        output.textures_delta.clear();
        assert!(
            text_rect.left() >= card_rect.left(),
            "text_rect={text_rect:?} overflowed left of card_rect={card_rect:?}"
        );
        assert!(
            text_rect.right() <= control_rect.left(),
            "text_rect={text_rect:?} overlapped control_rect={control_rect:?}"
        );
    }

    #[test]
    fn setting_row_centers_control_vertically_with_symmetrical_top_bottom_padding() {
        let ctx = egui::Context::default();
        let mut text_rect = egui::Rect::NOTHING;
        let mut control_rect = egui::Rect::NOTHING;
        let mut row_rect = egui::Rect::NOTHING;
        // Run two frames so cached row height settles.
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(620.0, 400.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    card_group(ui, |ui| {
                        let (t, c, r) = setting_row_inner(
                            ui,
                            "英単語の判定",
                            "文の途中でもローマ字にならない英単語を変換します。",
                            |ui| {
                                let mut on = true;
                                toggle(ui, &mut on);
                            },
                        );
                        text_rect = t;
                        control_rect = c;
                        row_rect = r;
                    });
                });
            });
            output.textures_delta.clear();
        }
        // No mysterious extra space above the text: text starts at the top of the row.
        assert!(
            (text_rect.top() - row_rect.top()).abs() <= 1.0,
            "unexpected top gap: text_rect={text_rect:?}, row_rect={row_rect:?}"
        );
        // Right-hand toggle is vertically centered against the full row height.
        assert!(
            (control_rect.center().y - row_rect.center().y).abs() <= 1.5,
            "control not centered: control_rect={control_rect:?}, row_rect={row_rect:?}"
        );
        // No horizontal overlap.
        assert!(
            text_rect.right() <= control_rect.left(),
            "text_rect={text_rect:?} overlapped control_rect={control_rect:?}"
        );
    }
}
