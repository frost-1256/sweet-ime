//! Review and manage the user's learned conversions.

use sweet_ui::eframe::egui::{self, Color32, RichText};
use sweet_ui::{card_frame, card_group, section_header, setting_divider, setting_row, toggle};

use super::App;

impl App {
    pub(super) fn learning_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        let mut forget_within = None;
        section_header(ui, "学習の設定");
        card_group(ui, |ui| {
            setting_row(
                ui,
                "学習する",
                "選んだ候補を覚えて、次から優先します。オフにすると覚えた内容も使いません。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.learning).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "パスワード欄では学習しない",
                "パスワード欄やブラウザーのプライベートウィンドウで入力した内容は覚えません。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.private_fields).changed();
                },
            );
            if self.history.is_some() {
                setting_divider(ui);
                setting_row(
                    ui,
                    "最近の学習を消す",
                    "うっかり覚えさせた変換を、時間を区切って取り消します。",
                    |ui| {
                        if ui.button("今日 (24 時間)").clicked() {
                            forget_within = Some((24 * 3600, "24 時間"));
                        }
                        if ui.button("直近 1 時間").clicked() {
                            forget_within = Some((3600, "1 時間"));
                        }
                    },
                );
            }
        });
        if changed {
            self.save();
        }
        let Some(_) = self.history else {
            ui.label("学習データの場所がわかりません(APPDATA が未設定)。");
            return;
        };
        if let Some((secs, label)) = forget_within {
            let r = self
                .history
                .as_mut()
                .map(|h| h.forget_recent(std::time::Duration::from_secs(secs)));
            self.reload_history();
            self.status = match r {
                Some(Ok(n)) => Some((format!("直近 {label}に覚えた {n} 件を消しました。"), true)),
                Some(Err(e)) => Some((format!("消去できませんでした: {e}"), false)),
                None => None,
            };
        }

        section_header(ui, "学習履歴");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("覚えた単語({} 件)", self.learned.len())).size(15.5),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_clear {
                        if ui.button("取り消し").clicked() {
                            self.confirm_clear = false;
                        }
                        let danger = egui::Button::new(
                            RichText::new("本当にすべて消去").color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(196, 43, 28));
                        if ui.add(danger).clicked() {
                            let r = self.history.as_mut().map(|h| h.clear());
                            self.confirm_clear = false;
                            self.reload_history();
                            self.status = Some(match r {
                                Some(Ok(())) => ("学習データを消去しました。".into(), true),
                                Some(Err(e)) => (format!("消去できませんでした: {e}"), false),
                                None => return,
                            });
                        }
                    } else if ui
                        .add_enabled(!self.learned.is_empty(), egui::Button::new("すべて消去"))
                        .clicked()
                    {
                        self.confirm_clear = true;
                    }
                    if ui.button("更新").clicked() {
                        self.reload_history();
                    }
                });
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("読みや単語で絞り込み")
                    .desired_width(f32::INFINITY),
            );
            let f = self.filter.trim();
            let rows: Vec<usize> = (0..self.learned.len())
                .filter(|&i| {
                    let (r, e) = &self.learned[i];
                    f.is_empty() || r.contains(f) || e.surface.contains(f)
                })
                .collect();
            if rows.is_empty() {
                ui.label(
                    RichText::new("まだありません。変換で 2 番目以降の候補を選ぶと覚えます。")
                        .weak(),
                );
            }
            let mut forget = None;
            let row_h = 30.0;
            egui::ScrollArea::vertical()
                .id_salt("learned_words")
                .max_height(320.0)
                .auto_shrink([false, true])
                .show_rows(ui, row_h, rows.len(), |ui, range| {
                    for &i in &rows[range] {
                        let (reading, e) = &self.learned[i];
                        ui.horizontal(|ui| {
                            ui.set_min_height(row_h - 6.0);
                            ui.label(RichText::new(&e.surface).size(16.0));
                            ui.label(RichText::new(reading).weak());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(6.0);
                                    if ui.small_button("削除").clicked() {
                                        forget = Some(i);
                                    }
                                    ui.label(
                                        RichText::new(format!("{} 回", e.count)).small().weak(),
                                    );
                                },
                            );
                        });
                    }
                });
            if let Some(i) = forget {
                let (reading, e) = self.learned[i].clone();
                if let Some(h) = &mut self.history {
                    h.forget(&reading, &e.surface);
                }
                self.reload_history();
            }
        });
        self.reports.ui(ui, &mut self.status);
    }
}
