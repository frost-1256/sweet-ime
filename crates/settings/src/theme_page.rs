//! Theme picker: a small preview of the candidate window per theme, plus a
//! separate customizable user theme with background opacity and saved presets.

#[cfg(not(windows))]
use sweet_engine::theme::{MAX_PANEL_PADDING, MIN_PANEL_PADDING};
use sweet_engine::theme::{Colors, MIN_BG_ALPHA, Rgb, SavedUserTheme, sanitize_theme_name};
use sweet_engine::{Theme, UserTheme};
use sweet_ui::card_frame;
use sweet_ui::eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, Vec2, pos2,
    vec2,
};

const TILE: Vec2 = Vec2::new(176.0, 150.0);

fn c32(v: Rgb) -> Color32 {
    Color32::from_rgb(v.0, v.1, v.2)
}

/// Which theme files changed during a frame.
#[derive(Default)]
pub struct ThemeChange {
    pub theme_selected: bool,
    pub user_theme_edited: bool,
    pub saved_themes_changed: bool,
}

impl ThemeChange {
    pub fn any(&self) -> bool {
        self.theme_selected || self.user_theme_edited
    }
}

/// Draws the built-in theme gallery and the separate user theme editor.
pub fn ui(
    ui: &mut egui::Ui,
    current: &mut Theme,
    user_theme: &mut UserTheme,
    saved_themes: &mut Vec<SavedUserTheme>,
    saved_themes_error: Option<&str>,
    save_name: &mut String,
) -> ThemeChange {
    let mut change = ThemeChange::default();
    let sys_light = crate::appearance::system_light(ui);
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("候補ウィンドウのテーマ").size(15.5));
        #[cfg(windows)]
        let note = "変換候補と入力モード表示の色を選びます。デフォルトは Windows のライト/ダークとアクセントカラーに合わせます。";
        #[cfg(not(windows))]
        let note = "fcitx5 の候補ウィンドウのテーマとして書き込みます (ほかの入力メソッドにも使われます)。デフォルトはデスクトップのライト/ダークに合わせます。";
        ui.label(RichText::new(note).small().weak());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for t in Theme::BUILTINS {
                let colors = t.colors(sys_light);
                let name = if t == Theme::Default { "デフォルト" } else { t.label() };
                if tile(ui, name, t.label(), colors, *current == t).clicked() && *current != t {
                    *current = t;
                    change.theme_selected = true;
                }
            }
        });
    });

    ui.add_space(4.0);

    // Separate User Theme card.
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("ユーザーテーマ").size(15.5));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let selected = *current == Theme::Custom;
                if selected {
                    ui.label(RichText::new("使用中").small().weak());
                } else if ui.button("ユーザーテーマを使う").clicked() {
                    *current = Theme::Custom;
                    change.theme_selected = true;
                }
            });
        });
        ui.label(
            RichText::new(
                "既存テーマとは別に、色や背景の透過度を自由に設定して保存できます。",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);

        let mut edited = false;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(20.0, 10.0);
            let colors = user_theme.colors();
            if tile(
                ui,
                "ユーザーテーマ",
                "ユーザーテーマ (背景透過・カスタム配色)",
                colors,
                *current == Theme::Custom,
            )
            .clicked()
                && *current != Theme::Custom
            {
                *current = Theme::Custom;
                change.theme_selected = true;
            }

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
                ui.spacing_mut().slider_width = 180.0;

                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("user_theme_preset")
                        .selected_text("既存テーマからコピー")
                        .show_ui(ui, |ui| {
                            for preset in Theme::BUILTINS {
                                if ui.selectable_label(false, preset.label()).clicked() {
                                    let keep_alpha = user_theme.bg_alpha;
                                    *user_theme = UserTheme::from_colors(preset.colors(sys_light));
                                    user_theme.bg_alpha = keep_alpha;
                                    edited = true;
                                }
                            }
                        });
                    if ui.button("初期値に戻す").clicked() {
                        *user_theme = UserTheme::default();
                        edited = true;
                    }
                });

                egui::Grid::new("user_theme_sliders")
                    .num_columns(2)
                    .spacing(vec2(14.0, 8.0))
                    .show(ui, |ui| {
                        let mut bg_pct =
                            (user_theme.bg_alpha.clamp(MIN_BG_ALPHA, 1.0) * 100.0).round() as u32;
                        ui.label("背景の不透明度");
                        if ui
                            .add(
                                egui::Slider::new(
                                    &mut bg_pct,
                                    (MIN_BG_ALPHA * 100.0) as u32..=100,
                                )
                                .suffix("%"),
                            )
                            .changed()
                        {
                            user_theme.bg_alpha = (bg_pct as f32 / 100.0).clamp(MIN_BG_ALPHA, 1.0);
                            edited = true;
                        }
                        ui.end_row();

                        let mut hover_pct =
                            (user_theme.hover_alpha.clamp(0.0, 0.5) * 100.0).round() as u32;
                        ui.label("選択行の不透明度");
                        if ui
                            .add(egui::Slider::new(&mut hover_pct, 0..=50).suffix("%"))
                            .changed()
                        {
                            user_theme.hover_alpha = (hover_pct as f32 / 100.0).clamp(0.0, 0.5);
                            edited = true;
                        }
                        ui.end_row();

                        #[cfg(not(windows))]
                        {
                            let mut pad = user_theme
                                .panel_padding
                                .clamp(MIN_PANEL_PADDING, MAX_PANEL_PADDING);
                            ui.label("パネルの余白");
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut pad,
                                        MIN_PANEL_PADDING..=MAX_PANEL_PADDING,
                                    )
                                    .suffix("px"),
                                )
                                .changed()
                            {
                                user_theme.panel_padding = pad;
                                edited = true;
                            }
                            ui.end_row();
                        }
                    });

                ui.add_space(2.0);
                egui::Grid::new("user_theme_colors")
                    .num_columns(2)
                    .spacing(vec2(24.0, 8.0))
                    .show(ui, |ui| {
                        edited |= color_field(ui, "背景", &mut user_theme.bg);
                        edited |= color_field(ui, "文字", &mut user_theme.text);
                        ui.end_row();
                        edited |= color_field(ui, "補助文字", &mut user_theme.sub);
                        edited |= color_field(ui, "アクセント", &mut user_theme.accent);
                        ui.end_row();
                        edited |= color_field(ui, "枠線", &mut user_theme.border);
                        edited |= color_field(ui, "選択行", &mut user_theme.hover);
                        ui.end_row();
                    });

                ui.add_space(4.0);
                if let Some(err) = saved_themes_error {
                    ui.label(
                        RichText::new(format!(
                            "保存済みテーマを読み込めませんでした ({err})。保護のため保存を無効にしています。"
                        ))
                        .small()
                        .color(ui.visuals().error_fg_color),
                    );
                } else {
                    ui.horizontal(|ui| {
                        let field_w = (ui.available_width() - 90.0).clamp(240.0, 360.0);
                        ui.add(
                            egui::TextEdit::singleline(save_name)
                                .hint_text("テーマ名を入力")
                                .desired_width(field_w),
                        );
                        let clean_name = sanitize_theme_name(save_name);
                        if ui
                            .add_enabled(!clean_name.is_empty(), egui::Button::new("保存"))
                            .clicked()
                        {
                            if let Some(existing) =
                                saved_themes.iter_mut().find(|t| t.name == clean_name)
                            {
                                existing.theme = *user_theme;
                            } else {
                                saved_themes.push(SavedUserTheme {
                                    name: clean_name,
                                    theme: *user_theme,
                                });
                            }
                            change.saved_themes_changed = true;
                        }
                    });
                }
            });
        });

        if !saved_themes.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(6.0);
            ui.label(RichText::new("保存したユーザーテーマ").size(14.5));
            ui.add_space(6.0);
            let mut remove_idx = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
                for (i, saved) in saved_themes.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                        let is_active =
                            *current == Theme::Custom && saved.theme.colors() == user_theme.colors();
                        if tile(ui, &saved.name, &saved.name, saved.theme.colors(), is_active)
                            .clicked()
                        {
                            *user_theme = saved.theme;
                            *save_name = saved.name.clone();
                            edited = true;
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("削除").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    });
                }
            });
            if let Some(i) = remove_idx {
                saved_themes.remove(i);
                change.saved_themes_changed = true;
            }
        }

        if edited {
            change.user_theme_edited = true;
            if *current != Theme::Custom {
                *current = Theme::Custom;
                change.theme_selected = true;
            }
        }
    });

    change
}

fn color_field(ui: &mut egui::Ui, label: &str, rgb: &mut [u8; 3]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_sized([74.0, 22.0], egui::Label::new(label));
        changed = ui.color_edit_button_srgb(rgb).changed();
        let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
        ui.label(RichText::new(hex).small().weak().monospace());
    });
    changed
}

fn tile(
    ui: &mut egui::Ui,
    name: &str,
    hover_text: &str,
    c: Colors,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(TILE, Sense::click());
    let visuals = ui.visuals();
    let accent_ui = visuals.selection.bg_fill;
    let outline = if selected {
        Stroke::new(2.0, accent_ui)
    } else if response.hovered() {
        Stroke::new(1.0, visuals.widgets.hovered.bg_stroke.color)
    } else {
        Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color)
    };
    let label_color = visuals.text_color();
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        CornerRadius::same(8),
        visuals.extreme_bg_color,
        outline,
        StrokeKind::Inside,
    );

    // Mini candidate window.
    let win = egui::Rect::from_min_size(rect.min + vec2(14.0, 12.0), vec2(TILE.x - 28.0, 98.0));
    let bg_alpha = c.bg_alpha.clamp(MIN_BG_ALPHA, 1.0);
    if bg_alpha < 0.99 {
        // Subtle checkerboard behind translucent previews so opacity is visible.
        let cell = 10.0;
        let light_sq = Color32::from_gray(165);
        let dark_sq = Color32::from_gray(120);
        let cols = (win.width() / cell).ceil() as usize;
        let rows = (win.height() / cell).ceil() as usize;
        for row in 0..rows {
            for col in 0..cols {
                let min = pos2(win.left() + col as f32 * cell, win.top() + row as f32 * cell);
                let max = pos2((min.x + cell).min(win.right()), (min.y + cell).min(win.bottom()));
                let sq = egui::Rect::from_min_max(min, max);
                painter.rect_filled(
                    sq,
                    CornerRadius::ZERO,
                    if (row + col) % 2 == 0 { light_sq } else { dark_sq },
                );
            }
        }
    }
    let bg_fill = Color32::from_rgba_unmultiplied(
        c.bg.0,
        c.bg.1,
        c.bg.2,
        (bg_alpha * 255.0).round() as u8,
    );
    painter.rect(
        win,
        CornerRadius::same(6),
        bg_fill,
        Stroke::new(1.0, c32(c.border)),
        StrokeKind::Inside,
    );
    let accent = c
        .accent
        .map(c32)
        .unwrap_or_else(|| crate::appearance::system_accent(c.dark));
    let row_h = 26.0;
    for (i, word) in ["今日", "京", "強"].iter().enumerate() {
        let top = win.top() + 4.0 + i as f32 * row_h;
        if i == 0 {
            let pill = egui::Rect::from_min_max(
                pos2(win.left() + 4.0, top + 2.0),
                pos2(win.right() - 4.0, top + row_h - 2.0),
            );
            let h = c.hover;
            let a = (c.hover_alpha * 255.0).round() as u8;
            painter.rect_filled(
                pill,
                CornerRadius::same(4),
                Color32::from_rgba_unmultiplied(h.0, h.1, h.2, a),
            );
            let bar = egui::Rect::from_min_max(
                pos2(win.left() + 4.0, top + 8.0),
                pos2(win.left() + 7.0, top + row_h - 8.0),
            );
            painter.rect_filled(bar, CornerRadius::same(1), accent);
        }
        let y = top + row_h / 2.0;
        painter.text(
            pos2(win.left() + 16.0, y),
            Align2::LEFT_CENTER,
            (i + 1).to_string(),
            FontId::proportional(11.0),
            c32(c.sub),
        );
        painter.text(
            pos2(win.left() + 32.0, y),
            Align2::LEFT_CENTER,
            *word,
            FontId::proportional(14.0),
            c32(c.text),
        );
    }
    painter.text(
        pos2(win.right() - 8.0, win.bottom() - 9.0),
        Align2::RIGHT_CENTER,
        "1 / 3",
        FontId::proportional(10.0),
        c32(c.sub),
    );

    painter.text(
        pos2(rect.center().x, rect.bottom() - 20.0),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(14.0),
        label_color,
    );
    response.on_hover_text(hover_text)
}
