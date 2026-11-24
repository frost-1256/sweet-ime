//! Per-application input modes: the mode for most programs, and direct
//! input or no IME in chosen programs.

use eframe::egui::{self, RichText};
use sweet_engine::app_modes::{AppMode, AppModes};
use sweet_engine::paths;
use sweet_ui::{card_frame, eframe};

/// Programs offered with one click.
const PRESETS: [(&str, &str, AppMode); 4] = [
    ("リモートデスクトップ", "mstsc.exe", AppMode::Off),
    ("Hyper-V の接続", "vmconnect.exe", AppMode::Off),
    ("Windows Terminal", "WindowsTerminal.exe", AppMode::English),
    ("VS Code", "Code.exe", AppMode::English),
];

pub struct AppsPage {
    modes: AppModes,
    exe: String,
    mode: AppMode,
}

impl AppsPage {
    pub fn new() -> Self {
        Self { modes: paths::load_app_modes(), exe: String::new(), mode: AppMode::English }
    }

    fn save(&self, status: &mut Option<(String, bool)>) {
        *status = Some(match paths::save_app_modes(&self.modes) {
            Ok(()) => ("保存しました。次にそのアプリへ切り替えたときから使われます。".into(), true),
            Err(e) => (format!("保存できませんでした: {e}"), false),
        });
    }

    fn add(&mut self, exe: &str, mode: AppMode) {
        let exe = exe.trim().trim_matches('"');
        let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_string();
        // `*` is the line for unlisted programs, set by the picker above.
        if name.is_empty() || name == "*" {
            return;
        }
        self.modes.entries.retain(|(e, _)| !e.eq_ignore_ascii_case(&name));
        self.modes.entries.push((name, mode));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        ui.label(
            RichText::new(
                "IME に切り替えたときにひらがなと英字のどちらで始めるかを決められます。\
                 アプリごとに別のモードにしたり、まったく使わないようにもできます。",
            )
            .weak(),
        );
        ui.add_space(6.0);
        let mut changed = false;

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let size = egui::vec2(ui.available_width(), 34.0);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new("一覧にないアプリ").size(15.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let before = self.modes.default;
                    egui::ComboBox::from_id_salt("default_mode").selected_text(before.label()).show_ui(ui, |ui| {
                        for m in [AppMode::Normal, AppMode::English] {
                            ui.selectable_value(&mut self.modes.default, m, m.label());
                        }
                    });
                    changed |= self.modes.default != before;
                });
            });
        });
        ui.add_space(6.0);

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("設定したアプリ({} 件)", self.modes.entries.len())).size(15.5));
            if self.modes.entries.is_empty() {
                ui.label(RichText::new("まだありません。下から追加してください。").weak());
            }
            let mut remove = None;
            for (i, (exe, mode)) in self.modes.entries.iter_mut().enumerate() {
                let size = egui::vec2(ui.available_width(), 34.0);
                ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(RichText::new(exe.as_str()).monospace());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("削除").clicked() {
                            remove = Some(i);
                        }
                        changed |= mode_combo(ui, ("mode", i), mode);
                    });
                });
            }
            if let Some(i) = remove {
                self.modes.entries.remove(i);
                changed = true;
            }
        });

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("追加").size(15.5));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.exe)
                        .hint_text("プログラム名(例: game.exe)")
                        .desired_width(220.0),
                );
                mode_combo(ui, "new_mode", &mut self.mode);
                if ui.add_enabled(!self.exe.trim().is_empty(), egui::Button::new("追加")).clicked() {
                    let exe = std::mem::take(&mut self.exe);
                    self.add(&exe, self.mode);
                    changed = true;
                }
            });
            ui.label(
                RichText::new("プログラム名はタスク マネージャーの「詳細」タブで確認できます。").weak().small(),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (label, exe, mode) in PRESETS {
                    let present = self.modes.entries.iter().any(|(e, _)| e.eq_ignore_ascii_case(exe));
                    let text = format!("{label}:{}", mode.label());
                    if ui.add_enabled(!present, egui::Button::new(text)).clicked() {
                        self.add(exe, mode);
                        changed = true;
                    }
                }
            });
        });

        if changed {
            self.save(status);
        }
    }
}

/// A mode picker; returns whether the mode changed.
fn mode_combo(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, mode: &mut AppMode) -> bool {
    let before = *mode;
    egui::ComboBox::from_id_salt(id).selected_text(mode.label()).show_ui(ui, |ui| {
        for m in AppMode::ALL {
            ui.selectable_value(mode, m, m.label());
        }
    });
    *mode != before
}
