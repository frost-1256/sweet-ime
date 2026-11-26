//! Theme picker: a small preview of the candidate window per theme.

use sweet_engine::Theme;
use sweet_engine::theme::Rgb;
use sweet_ui::card_frame;
use sweet_ui::eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, Vec2, pos2,
    vec2,
};

const TILE: Vec2 = Vec2::new(176.0, 150.0);

fn c32(v: Rgb) -> Color32 {
    Color32::from_rgb(v.0, v.1, v.2)
}

/// Draws the theme tiles; returns true when the selection changed.
pub fn ui(ui: &mut egui::Ui, current: &mut Theme) -> bool {
    let mut changed = false;
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
            for t in Theme::ALL {
                if tile(ui, t, *current == t).clicked() && *current != t {
                    *current = t;
                    changed = true;
                }
            }
        });
    });
    changed
}

fn tile(ui: &mut egui::Ui, theme: Theme, selected: bool) -> egui::Response {
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
    let c = theme.colors(crate::appearance::system_light(ui));
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
    painter.rect(
        win,
        CornerRadius::same(6),
        c32(c.bg),
        Stroke::new(1.0, c32(c.border)),
        StrokeKind::Inside,
    );
    let accent = c.accent.map(c32).unwrap_or(if c.dark {
        Color32::from_rgb(0x60, 0xcd, 0xff)
    } else {
        Color32::from_rgb(0x00, 0x5f, 0xb8)
    });
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

    let name = if theme == Theme::Default {
        "デフォルト"
    } else {
        theme.label()
    };
    painter.text(
        pos2(rect.center().x, rect.bottom() - 20.0),
        Align2::CENTER_CENTER,
        name,
        FontId::proportional(14.0),
        label_color,
    );
    response.on_hover_text(theme.label())
}
