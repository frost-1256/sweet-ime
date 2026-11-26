//! "ローマ字" page: edits the romaji table in `romaji.tsv`.

use eframe::egui::{self, Align, Layout, RichText, Vec2};
use sweet_engine::paths;
use sweet_engine::romaji::{self, Rule};
use sweet_ui::{card_frame, eframe};

pub struct RomajiPage {
    rules: Vec<Rule>,
    filter: String,
    /// Path typed for import or export.
    file: String,
    confirm_reset: bool,
    /// Measured height of one row, for the virtualized list.
    row_h: f32,
}

impl RomajiPage {
    pub fn new() -> Self {
        Self {
            rules: paths::load_romaji_rules().unwrap_or_else(romaji::default_rules),
            filter: String::new(),
            file: String::new(),
            confirm_reset: false,
            row_h: 40.0,
        }
    }

    fn save(&self) -> (String, bool) {
        match paths::save_romaji_rules(&self.rules) {
            Ok(()) => (
                "保存しました。アプリを切り替えると反映されます。".into(),
                true,
            ),
            Err(e) => (format!("保存できませんでした: {e}"), false),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        ui.label(
            RichText::new(
                "入力したローマ字をかなに置き換える表です。「次の入力」は、置き換えたあとに入力の続きとして残す文字です\
                 (「tt」→「っ」と、次の入力「t」)。同じ入力が複数あるときは下の行が使われます。",
            )
            .weak(),
        );
        ui.add_space(4.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.add(sweet_ui::primary_button("行を追加")).clicked() {
                self.rules.insert(0, Rule::default());
                self.filter.clear();
                changed = true;
            }
            if self.confirm_reset {
                ui.label("表を初期設定に戻しますか?");
                if ui.button("戻す").clicked() {
                    *status = Some(match paths::reset_romaji_rules() {
                        Ok(()) => ("初期設定に戻しました。".into(), true),
                        Err(e) => (format!("戻せませんでした: {e}"), false),
                    });
                    self.rules = romaji::default_rules();
                    self.confirm_reset = false;
                }
                if ui.button("やめる").clicked() {
                    self.confirm_reset = false;
                }
            } else if ui
                .add(egui::Button::new("初期設定に戻す").min_size(Vec2::new(0.0, 34.0)))
                .clicked()
            {
                self.confirm_reset = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .hint_text("検索")
                        .desired_width(160.0),
                );
            });
        });
        ui.add_space(4.0);

        let filter = self.filter.trim().to_lowercase();
        let rows: Vec<usize> = (0..self.rules.len())
            .filter(|&i| {
                let r = &self.rules[i];
                filter.is_empty()
                    || r.input.to_lowercase().contains(&filter)
                    || r.output.contains(&filter)
                    || r.next.to_lowercase().contains(&filter)
            })
            .collect();
        let mut remove = None;
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let column = |ui: &mut egui::Ui, text: &str, width: f32| {
                ui.add_sized(
                    [width, 20.0],
                    egui::Label::new(RichText::new(text).strong()),
                );
            };
            let width = ((ui.available_width() - 150.0) / 3.0).clamp(80.0, 200.0);
            ui.horizontal(|ui| {
                column(ui, "入力", width);
                column(ui, "出力", width);
                column(ui, "次の入力", width);
            });
            ui.separator();
            if rows.is_empty() {
                ui.label(RichText::new("一致する行はありません。").weak());
            }
            let row_h = self.row_h;
            let mut measured = None;
            egui::ScrollArea::vertical()
                .id_salt("romaji_rows")
                .max_height(360.0)
                .auto_shrink([false, true])
                .show_rows(ui, row_h, rows.len(), |ui, range| {
                    for &i in &rows[range] {
                        let duplicate = {
                            let key = self.rules[i].input.to_ascii_lowercase();
                            !key.is_empty()
                                && self
                                    .rules
                                    .iter()
                                    .filter(|r| r.input.to_ascii_lowercase() == key)
                                    .count()
                                    > 1
                        };
                        let r = &mut self.rules[i];
                        let row = ui.horizontal(|ui| {
                            for (text, hint) in [
                                (&mut r.input, "例: kya"),
                                (&mut r.output, "例: きゃ"),
                                (&mut r.next, ""),
                            ] {
                                let edit = egui::TextEdit::singleline(text)
                                    .hint_text(hint)
                                    .desired_width(width);
                                changed |= ui.add(edit).changed();
                            }
                            if duplicate {
                                ui.label(
                                    RichText::new("重複")
                                        .small()
                                        .color(ui.visuals().warn_fg_color),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("削除").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                        measured = Some(row.response.rect.height() + ui.spacing().item_spacing.y);
                    }
                });
            if let Some(h) = measured.filter(|h| (h - self.row_h).abs() > 0.5) {
                self.row_h = h;
                ui.ctx().request_repaint();
            }
            ui.label(
                RichText::new(format!("{} 行", self.rules.len()))
                    .small()
                    .weak(),
            );
        });
        if let Some(i) = remove {
            self.rules.remove(i);
            changed = true;
        }
        if changed {
            *status = Some(self.save());
        }

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("インポート・エクスポート").size(15.5));
            ui.label(
                RichText::new(
                    "タブ区切りのテキスト(入力・出力・次の入力)を読み書きします。\
                     Mozc や Google 日本語入力のローマ字テーブル編集で書き出したファイルも読み込めます。\
                     インポートすると今の表と置き換わります。",
                )
                .small()
                .weak(),
            );
            ui.add(egui::TextEdit::singleline(&mut self.file).hint_text("ファイルのパス").desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                let path = self.file.trim().trim_matches('"').to_string();
                if ui.add_enabled(!path.is_empty(), egui::Button::new("インポート")).clicked() {
                    *status = Some(match std::fs::read(&path).map(|b| crate::dict_page::decode_text(&b)) {
                        Ok(text) => {
                            let rules = romaji::parse_rules(&text);
                            if rules.is_empty() {
                                ("ローマ字の行が見つかりませんでした。".into(), false)
                            } else {
                                let n = rules.len();
                                self.rules = rules;
                                self.filter.clear();
                                match self.save() {
                                    (_, true) => (format!("{n} 行を読み込みました。"), true),
                                    failed => failed,
                                }
                            }
                        }
                        Err(e) => (format!("読み込めませんでした: {e}"), false),
                    });
                }
                if ui.add_enabled(!path.is_empty(), egui::Button::new("エクスポート")).clicked() {
                    *status = Some(match std::fs::write(&path, romaji::format_rules(&self.rules)) {
                        Ok(()) => (format!("{} 行を書き出しました。", self.rules.iter().filter(|r| !r.input.is_empty()).count()), true),
                        Err(e) => (format!("書き出せませんでした: {e}"), false),
                    });
                }
            });
        });
    }
}
