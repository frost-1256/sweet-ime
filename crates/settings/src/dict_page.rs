//! The user dictionary page: add, edit, delete, import and export words.

use eframe::egui::{self, RichText};
use sweet_engine::user_dict::{DEFAULT_POS, POS};
use sweet_engine::{UserDictionary, paths};
use sweet_ui::{card_frame, eframe};

pub struct DictPage {
    dict: Option<UserDictionary>,
    reading: String,
    surface: String,
    pos: String,
    comment: String,
    /// Index of the word being edited, if any.
    editing: Option<usize>,
    filter: String,
    /// Path typed for import or export.
    file: String,
}

impl DictPage {
    pub fn new() -> Self {
        Self {
            dict: paths::user_dict_file().map(UserDictionary::with_file),
            reading: String::new(),
            surface: String::new(),
            pos: DEFAULT_POS.into(),
            comment: String::new(),
            editing: None,
            filter: String::new(),
            file: String::new(),
        }
    }

    fn clear_form(&mut self) {
        self.reading.clear();
        self.surface.clear();
        self.comment.clear();
        self.pos = DEFAULT_POS.into();
        self.editing = None;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        let Some(mut dict) = self.dict.take() else {
            ui.label("辞書の保存場所がわかりません。");
            return;
        };
        self.body(ui, &mut dict, status);
        self.dict = Some(dict);
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        dict: &mut UserDictionary,
        status: &mut Option<(String, bool)>,
    ) {
        dict.reload_if_changed();
        ui.label(
            RichText::new("登録した単語は変換候補の上位に出ます。保存するとすぐに使われます。")
                .small()
                .weak(),
        );

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(if self.editing.is_some() {
                    "単語を編集"
                } else {
                    "単語を登録"
                })
                .size(15.5),
            );
            egui::Grid::new("dict_form")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("読み");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.reading)
                            .hint_text("ひらがな")
                            .desired_width(260.0),
                    );
                    ui.end_row();
                    ui.label("単語");
                    ui.add(egui::TextEdit::singleline(&mut self.surface).desired_width(260.0));
                    ui.end_row();
                    ui.label("品詞");
                    egui::ComboBox::from_id_salt("dict_pos")
                        .selected_text(self.pos.as_str())
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            for (label, _) in POS {
                                ui.selectable_value(&mut self.pos, (*label).to_string(), *label);
                            }
                        });
                    ui.end_row();
                    ui.label("コメント");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.comment)
                            .hint_text("任意")
                            .desired_width(260.0),
                    );
                    ui.end_row();
                });
            ui.horizontal(|ui| {
                let label = if self.editing.is_some() {
                    "保存"
                } else {
                    "登録"
                };
                if ui.add(sweet_ui::primary_button(label)).clicked() {
                    let r = match self.editing {
                        Some(i) => {
                            dict.replace(i, &self.reading, &self.surface, &self.pos, &self.comment)
                        }
                        None => dict.add(&self.reading, &self.surface, &self.pos, &self.comment),
                    };
                    let ok = r.is_ok();
                    *status = Some(match r {
                        Ok(()) => (format!("「{}」を登録しました。", self.surface.trim()), true),
                        Err(e) => (e.to_string(), false),
                    });
                    if ok {
                        self.clear_form();
                    }
                }
                if self.editing.is_some() && ui.button("取り消し").clicked() {
                    self.clear_form();
                }
            });
        });

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(format!("登録した単語({} 件)", dict.len())).size(15.5));
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("読みや単語で絞り込み")
                    .desired_width(f32::INFINITY),
            );
            let f = self.filter.trim();
            let rows: Vec<usize> = (0..dict.len())
                .filter(|&i| {
                    let w = &dict.words()[i];
                    f.is_empty()
                        || w.reading.contains(f)
                        || w.surface.contains(f)
                        || w.comment.contains(f)
                })
                .collect();
            if rows.is_empty() {
                ui.label(RichText::new("まだありません。").weak());
            }
            let (mut remove, mut edit) = (None, None);
            let row_h = 30.0;
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .auto_shrink([false, true])
                .show_rows(ui, row_h, rows.len(), |ui, range| {
                    for &i in &rows[range] {
                        let w = &dict.words()[i];
                        ui.horizontal(|ui| {
                            ui.set_min_height(row_h - 6.0);
                            ui.label(RichText::new(&w.surface).size(16.0));
                            ui.label(RichText::new(&w.reading).weak());
                            ui.label(RichText::new(&w.pos).small().weak());
                            if !w.comment.is_empty() {
                                ui.label(RichText::new(&w.comment).small().weak());
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("削除").clicked() {
                                        remove = Some(i);
                                    }
                                    if ui.small_button("編集").clicked() {
                                        edit = Some(i);
                                    }
                                },
                            );
                        });
                    }
                });
            if let Some(i) = edit {
                let w = dict.words()[i].clone();
                self.reading = w.reading;
                self.surface = w.surface;
                self.pos = w.pos;
                self.comment = w.comment;
                self.editing = Some(i);
            }
            if let Some(i) = remove {
                let surface = dict.words()[i].surface.clone();
                *status = Some(match dict.remove(i) {
                    Ok(()) => (format!("「{surface}」を削除しました。"), true),
                    Err(e) => (format!("削除できませんでした: {e}"), false),
                });
                self.editing = None;
            }
        });

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("インポート・エクスポート").size(15.5));
            ui.label(
                RichText::new(
                    "タブ区切りのテキスト(読み・単語・品詞・コメント)を読み書きします。\
                     Mozc、Google 日本語入力、Microsoft IME から書き出したファイルも読み込めます(UTF-8 または UTF-16)。",
                )
                .small()
                .weak(),
            );
            ui.add(egui::TextEdit::singleline(&mut self.file).hint_text("ファイルのパス").desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                let path = self.file.trim().trim_matches('"').to_string();
                if ui.add_enabled(!path.is_empty(), egui::Button::new("インポート")).clicked() {
                    *status = Some(match std::fs::read(&path).map(|b| decode_text(&b)) {
                        Ok(text) => match dict.import(&text) {
                            Ok(n) => (format!("{n} 件の単語を追加しました。"), true),
                            Err(e) => (format!("保存できませんでした: {e}"), false),
                        },
                        Err(e) => (format!("読み込めませんでした: {e}"), false),
                    });
                }
                if ui.add_enabled(!path.is_empty(), egui::Button::new("エクスポート")).clicked() {
                    *status = Some(match std::fs::write(&path, dict.export()) {
                        Ok(()) => (format!("{} 件を書き出しました。", dict.len()), true),
                        Err(e) => (format!("書き出せませんでした: {e}"), false),
                    });
                }
            });
        });
    }
}

/// Text from a file: UTF-16 with a byte order mark (Microsoft IME exports)
/// or UTF-8.
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}
