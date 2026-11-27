//! "キー" page: edits `%APPDATA%\SweetIME\keymap.txt`.

use eframe::egui::{self, Align, Layout, RichText, Vec2};
use sweet_engine::keymap::{ACTIONS, Action, Binding, Chord, KEYS, WHENS, When, action_label, key_label, when_label};
use sweet_engine::{Keymap, paths};
use sweet_ui::{card_frame, eframe};

pub struct KeysPage {
    map: Keymap,
    /// Row whose editor is open.
    editing: Option<usize>,
    filter: String,
}

impl KeysPage {
    pub fn new() -> Self {
        Self { map: paths::load_keymap(), editing: None, filter: String::new() }
    }

    fn save(&self) -> (String, bool) {
        match paths::save_keymap(&self.map) {
            Ok(()) => ("保存しました。アプリを切り替えると反映されます。".into(), true),
            Err(e) => (format!("保存できませんでした: {e}"), false),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        ui.label(
            RichText::new(
                "キーと動作の組み合わせです。割り当てのない文字キーはそのまま入力されます。\
                 「入力中のみ」の割り当ては、同じキーの「常に」より優先されます。",
            )
            .weak(),
        );
        ui.add_space(4.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.add(sweet_ui::primary_button(ui, "割り当てを追加")).clicked() {
                self.map.bindings.insert(
                    0,
                    Binding {
                        chord: Chord { vk: 0x20, ctrl: true, shift: false, alt: false },
                        action: Action::ImeToggle,
                        when: When::Always,
                    },
                );
                self.editing = Some(0);
                self.filter.clear();
                changed = true;
            }
            if ui.add(egui::Button::new("初期設定に戻す").min_size(Vec2::new(0.0, 34.0))).clicked() {
                *status = Some(match paths::reset_keymap() {
                    Ok(()) => ("初期設定に戻しました。".into(), true),
                    Err(e) => (format!("戻せませんでした: {e}"), false),
                });
                self.map = Keymap::default();
                self.editing = None;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("検索").desired_width(160.0));
            });
        });
        ui.add_space(4.0);

        let filter = self.filter.trim().to_lowercase();
        let mut remove = None;
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let n = self.map.bindings.len();
            let mut first = true;
            for i in 0..n {
                let b = self.map.bindings[i];
                let chord_text = b.chord.label();
                let action_text = action_label(b.action);
                if !filter.is_empty()
                    && !chord_text.to_lowercase().contains(&filter)
                    && !action_text.to_lowercase().contains(&filter)
                {
                    continue;
                }
                if !first {
                    ui.separator();
                }
                first = false;
                let duplicate = self.map.bindings.iter().filter(|x| x.chord == b.chord && x.when == b.when).count() > 1;
                ui.horizontal(|ui| {
                    // Fixed-width key column.
                    let w = ui.label(RichText::new(chord_text).strong()).rect.width();
                    ui.add_space((170.0 - w).max(8.0));
                    ui.label(action_text);
                    if b.when != When::Always {
                        ui.label(RichText::new(format!("({})", when_label(b.when))).small().weak());
                    }
                    if duplicate {
                        ui.label(RichText::new("重複").small().color(ui.visuals().warn_fg_color));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let small = |t: &str| egui::Button::new(RichText::new(t).size(13.5));
                        if ui.add(small("削除")).clicked() {
                            remove = Some(i);
                        }
                        let open = self.editing == Some(i);
                        if ui.add(small(if open { "閉じる" } else { "変更" })).clicked() {
                            self.editing = if open { None } else { Some(i) };
                        }
                    });
                });
                if self.editing == Some(i) {
                    changed |= editor(ui, i, &mut self.map.bindings[i]);
                }
            }
            if first {
                ui.label(RichText::new("一致する割り当てはありません。").weak());
            }
        });
        if let Some(i) = remove {
            self.map.bindings.remove(i);
            self.editing = match self.editing {
                Some(e) if e == i => None,
                Some(e) if e > i => Some(e - 1),
                e => e,
            };
            changed = true;
        }
        if changed {
            *status = Some(self.save());
        }
    }
}

/// Inline editor for one binding. Returns whether it changed.
fn editor(ui: &mut egui::Ui, i: usize, b: &mut Binding) -> bool {
    let before = *b;
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new(("binding-editor", i)).num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label("キー");
                ui.horizontal(|ui| {
                    for (on, name) in [(&mut b.chord.ctrl, "Ctrl"), (&mut b.chord.shift, "Shift"), (&mut b.chord.alt, "Alt")] {
                        if ui.add(egui::Button::new(name).selected(*on).min_size(Vec2::new(56.0, 30.0))).clicked() {
                            *on = !*on;
                        }
                    }
                    ui.label("+");
                    egui::ComboBox::from_id_salt(("key", i))
                        .selected_text(key_label(b.chord.vk))
                        .width(140.0)
                        .height(360.0)
                        .show_ui(ui, |ui| {
                            for (_, label, vk) in KEYS {
                                ui.selectable_value(&mut b.chord.vk, *vk, *label);
                            }
                        });
                });
                ui.end_row();
                ui.label("動作");
                egui::ComboBox::from_id_salt(("action", i))
                    .selected_text(action_label(b.action))
                    .width(220.0)
                    .height(360.0)
                    .show_ui(ui, |ui| {
                        for (_, label, a) in ACTIONS {
                            ui.selectable_value(&mut b.action, *a, *label);
                        }
                    });
                ui.end_row();
                ui.label("条件");
                egui::ComboBox::from_id_salt(("when", i))
                    .selected_text(when_label(b.when))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (_, label, w) in WHENS {
                            ui.selectable_value(&mut b.when, *w, *label);
                        }
                    });
                ui.end_row();
            });
        });
    *b != before
}
