//! Conversions the user reported, and sending them as a GitHub issue.

use std::path::PathBuf;

use sweet_engine::paths;
use sweet_engine::reports::{self, Report};
use sweet_ui::card_frame;
use sweet_ui::eframe::egui::{self, Color32, RichText};

pub struct ReportsSection {
    path: Option<PathBuf>,
    reports: Vec<Report>,
    confirm_clear: bool,
    reading: String,
    wanted: String,
}

impl ReportsSection {
    pub fn new() -> Self {
        let path = paths::reports_file();
        let reports = path.as_deref().and_then(|p| reports::load(p).ok()).unwrap_or_default();
        Self { path, reports, confirm_clear: false, reading: String::new(), wanted: String::new() }
    }

    fn reload(&mut self) {
        self.reports = self.path.as_deref().and_then(|p| reports::load(p).ok()).unwrap_or_default();
    }

    fn save(&mut self, status: &mut Option<(String, bool)>, done: &str) {
        let Some(path) = &self.path else { return };
        *status = Some(match reports::save(path, &self.reports) {
            Ok(()) => (done.to_string(), true),
            Err(e) => (format!("保存できませんでした: {e}"), false),
        });
        self.reload();
    }

    fn add(&mut self, status: &mut Option<(String, bool)>) {
        let Some(path) = &self.path else { return };
        let report = Report { reading: self.reading.trim().to_string(), text: self.wanted.trim().to_string() };
        *status = Some(match reports::add(path, report) {
            Ok(_) => {
                self.reading.clear();
                self.wanted.clear();
                ("報告に追加しました。".to_string(), true)
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {
                ("読みと期待する変換を入力してください。".to_string(), false)
            }
            Err(e) => (format!("保存できませんでした: {e}"), false),
        });
        self.reload();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("変換の報告({} 件)", self.reports.len())).size(15.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_clear {
                        if ui.button("取り消し").clicked() {
                            self.confirm_clear = false;
                        }
                        let danger = egui::Button::new(RichText::new("本当にすべて消去").color(Color32::WHITE))
                            .fill(Color32::from_rgb(196, 43, 28));
                        if ui.add(danger).clicked() {
                            self.confirm_clear = false;
                            self.reports.clear();
                            self.save(status, "報告を消去しました。");
                        }
                    } else if ui.add_enabled(!self.reports.is_empty(), egui::Button::new("すべて消去")).clicked() {
                        self.confirm_clear = true;
                    }
                    if ui.add_enabled(!self.reports.is_empty(), egui::Button::new("GitHub に送る")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(reports::issue_url(&self.reports)));
                    }
                    if ui.button("更新").clicked() {
                        self.reload();
                    }
                });
            });
            ui.label(
                RichText::new(
                    "うまく変換できなかった言葉の読みと期待する変換を書いて追加してください。変換を直して確定した\
                     直後に Ctrl+F12 を押しても追加できます(キーは「キー」で変えられます)。報告はこの PC の中だけに\
                     保存され、「GitHub に送る」で開く画面で内容を確かめて送信するまで、どこにも送られません。",
                )
                .weak(),
            );
            ui.horizontal(|ui| {
                let reading = egui::TextEdit::singleline(&mut self.reading).hint_text("読み(例: ほかにあんある)");
                ui.add(reading.desired_width(180.0));
                let wanted = egui::TextEdit::singleline(&mut self.wanted).hint_text("期待する変換(例: 他に案ある)");
                ui.add(wanted.desired_width(180.0));
                let ready = !self.reading.trim().is_empty() && !self.wanted.trim().is_empty();
                if ui
                    .add_enabled(ready, sweet_ui::accent_button(ui, "追加"))
                    .clicked()
                {
                    self.add(status);
                }
            });
            if self.reports.is_empty() {
                ui.label(RichText::new("まだありません。").weak());
            }
            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("conversion_reports")
                .max_height(240.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                for (i, r) in self.reports.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&r.text).size(16.0));
                        ui.label(RichText::new(&r.reading).weak());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            if ui.small_button("削除").clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                }
            });
            if let Some(i) = remove {
                self.reports.remove(i);
                self.save(status, "報告を 1 件消しました。");
            }
        });
    }
}
