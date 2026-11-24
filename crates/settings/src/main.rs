//! Sweet IME settings.
//!
//! Edits `config.txt` and `keymap.txt` (saved on every
//! change), manages the learning history and the user dictionary, and
//! downloads the optional neural model and updates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apps_page;
mod dict_page;
mod fcitx_theme;
mod keys_page;
mod learning_page;
mod neural_page;
mod reports_page;
mod theme_page;
mod update;
mod update_ui;

use eframe::egui::{self, RichText, Vec2};
use neural_page::{Download, MODELS};
use sweet_engine::learning::Learned;
use sweet_engine::{Options, UserHistory, paths};
use sweet_ui::eframe;
use sweet_ui::{card, card_frame, toggle};

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--check-update") {
        return update_ui::check_at_sign_in();
    }
    // Packaging: `--write-fcitx-themes <dir>` writes every fcitx5 theme.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--write-fcitx-themes") {
        let dir = args.get(i + 1).expect("--write-fcitx-themes <dir>");
        fcitx_theme::write_themes(std::path::Path::new(dir), &fcitx_theme::all_themes())
            .expect("cannot write the fcitx5 themes");
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sweet IME の設定")
            .with_icon(sweet_ui::app_icon())
            // Matches sweet-settings.desktop (Wayland uses it to find the icon).
            .with_app_id("sweet-settings")
            .with_inner_size([820.0, 600.0])
            .with_min_inner_size([560.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native("Sweet IME の設定", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Input,
    Keys,
    Apps,
    Theme,
    Neural,
    Learning,
    Dictionary,
    About,
}

impl Page {
    #[cfg(windows)]
    const ALL: &[Page] = &[Page::Input, Page::Keys, Page::Apps, Page::Theme, Page::Neural, Page::Learning, Page::Dictionary, Page::About];
    /// Elsewhere the input method framework picks the program's mode, and
    /// the theme is written as an fcitx5 theme.
    #[cfg(not(windows))]
    const ALL: &[Page] = &[Page::Input, Page::Keys, Page::Theme, Page::Neural, Page::Learning, Page::Dictionary, Page::About];

    fn label(self) -> &'static str {
        match self {
            Page::Input => "入力",
            Page::Keys => "キー",
            Page::Apps => "アプリ",
            Page::Theme => "テーマ",
            Page::Neural => "AI 変換",
            Page::Learning => "学習",
            Page::Dictionary => "辞書",
            Page::About => "情報",
        }
    }
}

struct App {
    page: Page,
    opts: Options,
    status: Option<(String, bool)>,
    history: Option<UserHistory>,
    learned: Vec<(String, Learned)>,
    filter: String,
    confirm_clear: bool,
    model: &'static str,
    download: Option<Download>,
    keys: keys_page::KeysPage,
    apps: apps_page::AppsPage,
    dict: dict_page::DictPage,
    reports: reports_page::ReportsSection,
    updater: update::Updater,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        sweet_ui::setup(&cc.egui_ctx);
        let history = paths::history_file().map(UserHistory::with_file);
        let page = match std::env::args().skip_while(|a| a != "--page").nth(1).as_deref() {
            Some("neural") => Page::Neural,
            Some("keys") => Page::Keys,
            Some("apps") => Page::Apps,
            Some("theme") => Page::Theme,
            Some("learning") => Page::Learning,
            Some("dictionary") => Page::Dictionary,
            Some("about") => Page::About,
            _ => Page::Input,
        };
        let opts = paths::load_options();
        let mut updater = update::Updater::new();
        if opts.update_check {
            updater.check(&cc.egui_ctx);
        }
        let mut app = Self {
            page,
            opts,
            status: None,
            history,
            learned: Vec::new(),
            filter: String::new(),
            confirm_clear: false,
            model: MODELS[0].0,
            download: None,
            keys: keys_page::KeysPage::new(),
            apps: apps_page::AppsPage::new(),
            dict: dict_page::DictPage::new(),
            reports: reports_page::ReportsSection::new(),
            updater,
        };
        app.reload_history();
        app
    }

    fn reload_history(&mut self) {
        if let Some(h) = &mut self.history {
            self.learned = h.all();
        }
    }

    fn save(&mut self) {
        self.status = Some(match paths::save_options(&self.opts) {
            Ok(()) => ("保存しました。アプリを切り替えると反映されます。".into(), true),
            Err(e) => (format!("保存できませんでした: {e}"), false),
        });
    }
}

/// Opens a folder in the file manager.
fn open_folder(dir: &std::path::Path) {
    #[cfg(windows)]
    let opener = "explorer.exe";
    #[cfg(not(windows))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.updater.poll(ui.ctx());
        if matches!(self.updater.state, update::State::Launched) {
            // Let the installer replace this program.
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(download) = &mut self.download {
            download.poll();
        }

        egui::Panel::left("nav").resizable(false).exact_size(190.0).show(ui, |ui| {
            ui.add_space(18.0);
            ui.label(RichText::new("Sweet IME").size(20.0).strong());
            ui.add_space(14.0);
            for &p in Page::ALL {
                let selected = self.page == p;
                let text = RichText::new(p.label()).size(15.5);
                let b = egui::Button::selectable(selected, text).min_size(Vec2::new(ui.available_width(), 34.0));
                if ui.add(b).clicked() {
                    self.page = p;
                    if p == Page::Learning {
                        self.reload_history();
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.add_space(14.0);
                if self.page != Page::About {
                    update_ui::banner(ui, &mut self.updater);
                }
                sweet_ui::title(ui, self.page.label());
                ui.add_space(6.0);
                match self.page {
                    Page::Input => self.input_page(ui),
                    Page::Keys => self.keys.ui(ui, &mut self.status),
                    Page::Apps => self.apps.ui(ui, &mut self.status),
                    Page::Theme => {
                        if theme_page::ui(ui, &mut self.opts.theme) {
                            self.save();
                            #[cfg(not(windows))]
                            {
                                self.status = Some(match fcitx_theme::apply(self.opts.theme) {
                                    Ok(()) => ("fcitx5 のテーマに設定しました。".into(), true),
                                    Err(e) => (e, false),
                                });
                            }
                        }
                        #[cfg(windows)]
                        card(
                            ui,
                            "ゲーミングモード",
                            "候補ウィンドウの縁を流れる虹色で光らせます。",
                            |ui| {
                                if toggle(ui, &mut self.opts.gaming_mode).changed() {
                                    self.save();
                                }
                            },
                        );
                        #[cfg(not(windows))]
                        if ui.button("fcitx5 の標準テーマに戻す").clicked() {
                            self.status = Some(match fcitx_theme::restore() {
                                Ok(()) => ("fcitx5 の標準テーマに戻しました。".into(), true),
                                Err(e) => (e, false),
                            });
                        }
                    }
                    Page::Neural => self.neural_page(ui),
                    Page::Learning => self.learning_page(ui),
                    Page::Dictionary => self.dict.ui(ui, &mut self.status),
                    Page::About => self.about_page(ui),
                }
                if let Some((msg, ok)) = &self.status {
                    ui.add_space(8.0);
                    let color = if *ok { ui.visuals().weak_text_color() } else { ui.visuals().error_fg_color };
                    ui.label(RichText::new(msg).small().color(color));
                }
            });
        });
    }
}

impl App {
    fn input_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        card(ui, "ライブ変換", "入力しながら漢字に変換して表示します。Enter でそのまま確定、Esc で読みに戻ります。", |ui| {
            changed |= toggle(ui, &mut self.opts.live_conversion).changed();
        });
        card(ui, "予測変換", "入力中に続きの候補を表示します。Tab キーで選択できます。", |ui| {
            changed |= toggle(ui, &mut self.opts.suggestions).changed();
        });
        #[cfg(windows)]
        card(ui, "入力した文字を表示", "「こんにちは」の近くに、打ったローマ字（konnnitiha）を表示します。", |ui| {
            changed |= toggle(ui, &mut self.opts.show_typed_input).changed();
        });
        card(ui, "英単語の判定", "「korehabestdesu」→「これはベストです」、「softwarewo」→「softwareを」のように、ローマ字にならない英単語を文の途中でも変換します。F10 で英字に直した単語も覚えます。", |ui| {
            changed |= toggle(ui, &mut self.opts.english_words).changed();
        });
        card(ui, "Shift の英字入力を1単語で戻す", "Shift を押しながら打ち始めた英字は、知っている単語のあとにローマ字が続くと、そこで確定して日本語入力に戻ります (「Googlehakensaku」→「Google」+「検索」)。オフにすると Space か Enter まで英字のままです。", |ui| {
            changed |= toggle(ui, &mut self.opts.shift_one_word).changed();
        });
        card(ui, "英単語をカタカナで出す", "「best」→「ベスト」のように、よく使うカタカナ語がある英単語はカタカナを先に出します。英字は次の候補になります。", |ui| {
            changed |= toggle(ui, &mut self.opts.english_katakana).changed();
        });
        card(ui, "日付と時刻","「きょう」「あした」で日付、「いま」で時刻を候補に出します。", |ui| {
            changed |= toggle(ui, &mut self.opts.dates).changed();
        });
        card(ui, "絵文字と顔文字", "「えがお」「ねこ」で絵文字、「かお」で顔文字を候補に出します。", |ui| {
            changed |= toggle(ui, &mut self.opts.emoji).changed();
        });
        card(ui, "計算", "「1+2*3=」のように = で終わる式を打つと、答えを候補に出します。", |ui| {
            changed |= toggle(ui, &mut self.opts.calculator).changed();
        });
        #[cfg(windows)]
        card(ui, "確定の取り消し", "確定した直後に Ctrl+Backspace を押すと、確定前の読みに戻します。", |ui| {
            changed |= toggle(ui, &mut self.opts.undo_commit).changed();
        });
        #[cfg(windows)]
        card(ui, "入力モードを表示", "IME のオン/オフを切り替えたとき、文字カーソルの近くに「あ」「A」を表示します。", |ui| {
            changed |= toggle(ui, &mut self.opts.mode_indicator).changed();
        });
        if changed {
            self.save();
        }
    }
}

impl App {
fn about_page(&mut self, ui: &mut egui::Ui) {
    card(ui, "Sweet IME", concat!("バージョン ", env!("CARGO_PKG_VERSION")), |_| {});
    if update_ui::section(ui, &mut self.updater, &mut self.opts.update_check) {
        self.save();
    }
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("ライセンス").size(15.5));
        ui.label("Sweet IME 本体: MIT License");
        ui.label("システム辞書: Mozc OSS 辞書(IPAdic・沖縄辞書を含む)。それぞれのライセンスに従います。");
        ui.label("追加語彙: SudachiDict (Works Applications Co., Ltd.、Apache License 2.0)");
        ui.label("AI 変換モデル(任意・別途ダウンロード): zenz by Miwa Keita, CC BY-SA 4.0");
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
        let licenses = exe_dir.and_then(|d| {
            [d.join("licenses"), d.join("../share/sweet-ime/licenses")].into_iter().find(|p| p.exists())
        });
        if let Some(dir) = licenses {
            if ui.button("ライセンス文書を開く").clicked() {
                open_folder(&dir);
            }
        }
    });
    if let Some(dir) = paths::config_dir() {
        card(ui, "設定フォルダー", &dir.to_string_lossy(), |ui| {
            if ui.button("開く").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                open_folder(&dir);
            }
        });
    }
}
}
