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
    /// Why `romaji.tsv` could not be read, if it could not.
    load_error: Option<String>,
    /// Measured height of one row, for the virtualized list.
    row_h: f32,
}

impl RomajiPage {
    pub fn new() -> Self {
        let (rules, load_error) = match paths::load_romaji_rules() {
            Ok(rules) => (rules.unwrap_or_else(romaji::default_rules), None),
            Err(e) => (romaji::default_rules(), Some(e.to_string())),
        };
        Self {
            rules,
            filter: String::new(),
            file: String::new(),
            confirm_reset: false,
            load_error,
            row_h: 40.0,
        }
    }

    fn save(&self) -> (String, bool) {
        let invalid = self
            .rules
            .iter()
            .filter(|r| romaji::validate_rule(r).is_some())
            .count();
        if invalid > 0 {
            return (
                format!(
                    "使えない行が {invalid} 件あるため保存していません。修正するか削除してください。"
                ),
                false,
            );
        }
        if self.rules.is_empty() {
            return (
                "行が 1 件もないため保存していません。初期設定に戻すか行を追加してください。"
                    .into(),
                false,
            );
        }
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
        if let Some(e) = &self.load_error {
            ui.label(
                RichText::new(format!(
                    "romaji.tsv を読めませんでした ({e})。IME はそれまでの表(なければ組み込みの表)を使い続けます。\
                     ここに出ている組み込みの表を変更すると、そのファイルは上書きされます。"
                ))
                .color(ui.visuals().warn_fg_color),
            );
        }
        ui.add_space(4.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.add(sweet_ui::primary_button("行を追加")).clicked() {
                self.rules.insert(0, Rule::default());
                self.filter.clear();
            }
            if self.confirm_reset {
                ui.label("表を初期設定に戻しますか?");
                if ui.button("戻す").clicked() {
                    *status = Some(match paths::reset_romaji_rules() {
                        Ok(()) => {
                            self.rules = romaji::default_rules();
                            self.load_error = None;
                            ("初期設定に戻しました。".into(), true)
                        }
                        Err(e) => (format!("戻せませんでした: {e}"), false),
                    });
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
            let scroll_reserve = ui.spacing().scroll.allocated_width();
            let col_gap = ui.spacing().item_spacing.x;
            let right_reserve = 140.0 + scroll_reserve + col_gap * 3.0;
            let width = ((ui.available_width() - right_reserve) / 3.0).max(80.0);
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
            let table_max_h = (ui.ctx().content_rect().height() - 360.0).clamp(240.0, 600.0);
            egui::ScrollArea::vertical()
                .id_salt("romaji_rows")
                .max_height(table_max_h)
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
                            if let Some(reason) = romaji::validate_rule(r) {
                                ui.label(
                                    RichText::new("無効")
                                        .small()
                                        .color(ui.visuals().error_fg_color),
                                )
                                .on_hover_text(reason);
                            } else if duplicate {
                                ui.label(
                                    RichText::new("重複")
                                        .small()
                                        .color(ui.visuals().warn_fg_color),
                                );
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.add_space(6.0);
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
                    *status = Some(match paths::read_romaji_table(std::path::Path::new(&path)) {
                        Ok(None) => ("ファイルが見つかりませんでした。".into(), false),
                        Ok(Some((rules, _))) if rules.is_empty() => {
                            ("ローマ字の行が見つかりませんでした。".into(), false)
                        }
                        // The editor keeps its table unless the new one was saved.
                        Ok(Some((rules, skipped))) => match paths::save_romaji_rules(&rules) {
                            Ok(()) => {
                                let n = rules.len();
                                self.rules = rules;
                                self.filter.clear();
                                self.load_error = None;
                                let mut message = format!("{n} 行を読み込みました。");
                                if skipped > 0 {
                                    message.push_str(&format!(
                                        "入力が空か長すぎるなどで使えない {skipped} 行は読み飛ばしました。"
                                    ));
                                }
                                (message, true)
                            }
                            Err(e) => (format!("保存できませんでした: {e}"), false),
                        },
                        Err(e) => (format!("読み込めませんでした: {e}"), false),
                    });
                }
                if ui.add_enabled(!path.is_empty(), egui::Button::new("エクスポート")).clicked() {
                    let valid_count = self
                        .rules
                        .iter()
                        .filter(|r| romaji::validate_rule(r).is_none())
                        .count();
                    *status = Some(match std::fs::write(&path, romaji::format_rules(&self.rules)) {
                        Ok(()) => (format!("{valid_count} 行を書き出しました。"), true),
                        Err(e) => (format!("書き出せませんでした: {e}"), false),
                    });
                }
            });
        });
    }
}
