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
    #[cfg(windows)]
    feedback_invite: String,
}

impl ReportsSection {
    pub fn new() -> Self {
        let path = paths::reports_file();
        let reports = path.as_deref().and_then(|p| reports::load(p).ok()).unwrap_or_default();
        Self {
            path, reports, confirm_clear: false, reading: String::new(), wanted: String::new(),
            #[cfg(windows)]
            feedback_invite: String::new(),
        }
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
        #[cfg(windows)]
        self.alpha_feedback_ui(ui, status);
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

    #[cfg(windows)]
    fn alpha_feedback_ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        use sweet_engine::alpha_feedback as feedback;
        let Some(endpoint) = feedback::endpoint() else { return };
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.heading("Alpha 版の変換データ提供");
            ui.label("同意すると、確定した日本語の読みと変換結果、候補を選び直した場合は最初の候補を、自動でまとめて開発者に送ります。");
            ui.label("パスワード・PIN・プライベート欄と、入力欄の種類を確認できない場合は対象外です。送信対象には日本語の氏名や内容が含まれることがあります。");
            ui.label("送信先では報告を非公開で保存し、30日で削除対象になります。同意を取り消すと以後の収集と送信が止まります。送信中・送信済みのデータは取り消せません。");
            ui.label(RichText::new(endpoint).weak());
            if feedback::consent().is_some() {
                ui.label("データ提供に同意済み");
                if ui.button("同意を取り消す").clicked() {
                    *status = Some(match feedback::disable() {
                        Ok(()) => ("データ提供を停止しました。".into(), true),
                        Err(_) => ("データ提供を停止できませんでした。".into(), false),
                    });
                }
            } else {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.feedback_invite).password(true).hint_text("招待コード"));
                    if ui.add_enabled(!self.feedback_invite.is_empty(), egui::Button::new("同意して提供を開始")).clicked() {
                        *status = Some(match feedback::enable(self.feedback_invite.trim()) {
                            Ok(()) => {
                                self.feedback_invite.clear();
                                ("データ提供を開始しました。".into(), true)
                            }
                            Err(_) => ("招待コードを確認してください。設定を保存できませんでした。".into(), false),
                        });
                    }
                });
            }
        });
    }
}
