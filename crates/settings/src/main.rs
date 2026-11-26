//! Sweet IME settings.
//!
//! Edits `config.txt`, `keymap.txt` and `romaji.tsv` (saved on every
//! change), manages the learning history and the user dictionary, and
//! downloads the optional neural model and updates.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod apps_page;
mod dict_page;
#[cfg(not(target_os = "macos"))]
mod fcitx_theme;
#[cfg(not(target_os = "macos"))]
mod font_picker;
mod keys_page;
mod learning_page;
mod neural_page;
mod reports_page;
mod romaji_page;
#[cfg(windows)]
mod single_instance;
#[cfg(not(target_os = "macos"))]
mod theme_page;
mod update;
mod update_ui;

use eframe::egui::{self, RichText};
use neural_page::{Download, MODELS};
use sweet_engine::learning::Learned;
use sweet_engine::{Options, UserHistory, paths};
use sweet_ui::eframe;
use sweet_ui::{card, card_frame, card_group, section_header, setting_divider, setting_row, toggle};

fn main() -> eframe::Result<()> {
    #[cfg(not(target_os = "macos"))]
    if std::env::args().any(|a| a == "--check-update") {
        return update_ui::check_at_sign_in();
    }
    // Packaging: `--write-fcitx-themes <dir>` writes every fcitx5 theme.
    let args: Vec<String> = std::env::args().collect();
    #[cfg(not(target_os = "macos"))]
    if let Some(i) = args.iter().position(|a| a == "--write-fcitx-themes") {
        let dir = args.get(i + 1).expect("--write-fcitx-themes <dir>");
        fcitx_theme::write_themes(std::path::Path::new(dir), &fcitx_theme::all_themes())
            .expect("cannot write the fcitx5 themes");
        return Ok(());
    }
    let page = args
        .iter()
        .position(|a| a == "--page")
        .map(|i| Page::from_arg(args.get(i + 1).map(String::as_str)));
    #[cfg(windows)]
    let Some(_instance) = single_instance::Instance::start(page).map_err(eframe::Error::AppCreation)? else {
        return Ok(());
    };
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
    eframe::run_native(
        "Sweet IME の設定",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, page.unwrap_or(Page::Input))?))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Input,
    Keys,
    Romaji,
    Apps,
    #[cfg(not(target_os = "macos"))]
    Theme,
    Neural,
    Learning,
    Dictionary,
    About,
}

impl Page {
    #[cfg(windows)]
    const ALL: &[Page] = &[Page::Input, Page::Keys, Page::Romaji, Page::Apps, Page::Theme, Page::Neural, Page::Learning, Page::Dictionary, Page::About];
    /// Elsewhere the input method framework picks the program's mode, and
    /// the theme is written as an fcitx5 theme.
    #[cfg(not(any(windows, target_os = "macos")))]
    const ALL: &[Page] = &[Page::Input, Page::Keys, Page::Romaji, Page::Theme, Page::Neural, Page::Learning, Page::Dictionary, Page::About];
    #[cfg(target_os = "macos")]
    const ALL: &[Page] = &[Page::Input, Page::Keys, Page::Romaji, Page::Neural, Page::Learning, Page::Dictionary, Page::About];

    fn from_arg(page: Option<&str>) -> Self {
        match page {
            Some("neural") => Self::Neural,
            Some("keys") => Self::Keys,
            Some("romaji") => Self::Romaji,
            Some("apps") => Self::Apps,
            #[cfg(not(target_os = "macos"))]
            Some("theme") => Self::Theme,
            Some("learning") => Self::Learning,
            Some("dictionary") => Self::Dictionary,
            Some("about") => Self::About,
            _ => Self::Input,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Page::Input => "入力",
            Page::Keys => "キー",
            Page::Romaji => "ローマ字",
            Page::Apps => "アプリ",
            #[cfg(not(target_os = "macos"))]
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
    #[cfg(windows)]
    activation: single_instance::Activation,
    opts: Options,
    user_theme: sweet_engine::UserTheme,
    #[cfg(not(target_os = "macos"))]
    saved_user_themes: Vec<sweet_engine::theme::SavedUserTheme>,
    #[cfg(not(target_os = "macos"))]
    saved_user_themes_error: Option<String>,
    #[cfg(not(target_os = "macos"))]
    user_theme_name: String,
    #[cfg(not(target_os = "macos"))]
    font_picker: Option<font_picker::FontPicker>,
    status: Option<(String, bool)>,
    history: Option<UserHistory>,
    learned: Vec<(String, Learned)>,
    filter: String,
    confirm_clear: bool,
    model: &'static str,
    download: Option<Download>,
    keys: keys_page::KeysPage,
    romaji: romaji_page::RomajiPage,
    apps: apps_page::AppsPage,
    dict: dict_page::DictPage,
    reports: reports_page::ReportsSection,
    updater: update::Updater,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        page: Page,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        sweet_ui::setup(&cc.egui_ctx);
        let history = paths::history_file().map(UserHistory::with_file);
        let opts = paths::load_options();
        let user_theme = paths::load_user_theme();
        #[cfg(not(target_os = "macos"))]
        let (saved_user_themes, saved_user_themes_error) = match paths::load_saved_user_themes() {
            Ok(list) => (list, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        let updater = {
            let mut updater = update::Updater::new();
            if opts.update_check {
                updater.check(&cc.egui_ctx);
            }
            updater
        };
        let mut app = Self {
            page,
            #[cfg(windows)]
            activation: single_instance::Activation::register(cc)?,
            opts,
            user_theme,
            #[cfg(not(target_os = "macos"))]
            saved_user_themes,
            #[cfg(not(target_os = "macos"))]
            saved_user_themes_error,
            #[cfg(not(target_os = "macos"))]
            user_theme_name: String::new(),
            #[cfg(not(target_os = "macos"))]
            font_picker: None,
            status: None,
            history,
            learned: Vec::new(),
            filter: String::new(),
            confirm_clear: false,
            model: MODELS[0].0,
            download: None,
            keys: keys_page::KeysPage::new(),
            romaji: romaji_page::RomajiPage::new(),
            apps: apps_page::AppsPage::new(),
            dict: dict_page::DictPage::new(),
            reports: reports_page::ReportsSection::new(),
            updater,
        };
        app.reload_history();
        Ok(app)
    }

    fn reload_history(&mut self) {
        if let Some(h) = &mut self.history {
            self.learned = h.all();
        }
    }

    fn select_page(&mut self, page: Page) {
        self.page = page;
        if page == Page::Learning {
            self.reload_history();
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
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(any(windows, target_os = "macos")))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(windows)]
        if let Some(page) = self.activation.take_page() {
            self.select_page(page);
        }
        self.updater.poll(ui.ctx());
        #[cfg(windows)]
        if matches!(self.updater.state, update::State::Launched) {
            // Let the installer replace this program.
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(download) = &mut self.download {
            download.poll();
        }

        let theme = appearance::apply(ui, self.opts.theme, &self.user_theme);
        let nav_frame = egui::Frame::new()
            .fill(theme.sidebar)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .inner_margin(egui::Margin::symmetric(12, 16));
        egui::Panel::left("nav")
            .resizable(false)
            .exact_size(168.0)
            .frame(nav_frame)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("nav_scroll")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(egui::Margin { left: 14, top: 3, ..egui::Margin::ZERO })
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(RichText::new("Sweet IME").size(20.0).strong());
                            ui.label(
                                RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                                    .size(12.0)
                                    .weak(),
                            );
                        });
                    ui.add_space(10.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for &p in Page::ALL {
                        #[cfg(not(target_os = "macos"))]
                        let split_before = matches!(p, Page::Theme | Page::About);
                        #[cfg(target_os = "macos")]
                        let split_before = matches!(p, Page::Neural | Page::About);
                        if split_before {
                            ui.add_space(6.0);
                            setting_divider(ui);
                            ui.add_space(6.0);
                        }
                        if theme.navigation_item(ui, p.label(), self.page == p).clicked() {
                            self.select_page(p);
                        }
                    }
                });
            });

        let content_frame = egui::Frame::new()
            .fill(theme.background)
            .inner_margin(egui::Margin::symmetric(22, 16));
        egui::CentralPanel::default().frame(content_frame).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("page_scroll")
                .auto_shrink(false)
                .show(ui, |ui| {
                sweet_ui::title(ui, self.page.label());
                ui.add_space(4.0);
                if self.page != Page::About {
                    update_ui::banner(ui, &mut self.updater);
                }
                match self.page {
                    Page::Input => self.input_page(ui),
                    Page::Keys => self.keys.ui(ui, &mut self.status),
                    Page::Romaji => self.romaji.ui(ui, &mut self.status),
                    Page::Apps => self.apps.ui(ui, &mut self.status),
                    #[cfg(not(target_os = "macos"))]
                    Page::Theme => {
                        section_header(ui, "候補ウィンドウの表示");
                        let mut gaming_changed = false;
                        card_group(ui, |ui| {
                            self.font_picker
                                .get_or_insert_with(font_picker::FontPicker::new)
                                .ui(ui, &mut self.status);
                            setting_divider(ui);
                            setting_row(
                                ui,
                                "ゲーミングモード",
                                "候補ウィンドウの縁を流れる虹色で光らせます。",
                                |ui| {
                                    gaming_changed = toggle(ui, &mut self.opts.gaming_mode).changed();
                                },
                            );
                        });
                        if gaming_changed {
                            self.save();
                            #[cfg(not(windows))]
                            if self.status.as_ref().is_none_or(|(_, ok)| *ok) {
                                fcitx_theme::reload_sweetui();
                            }
                        }

                        section_header(ui, "カラーテーマ");
                        let theme_change = theme_page::ui(
                            ui,
                            &mut self.opts.theme,
                            &mut self.user_theme,
                            &mut self.saved_user_themes,
                            self.saved_user_themes_error.as_deref(),
                            &mut self.user_theme_name,
                        );
                        let mut save_ok = true;
                        if theme_change.saved_themes_changed
                            && self.saved_user_themes_error.is_none()
                        {
                            if let Err(e) = paths::save_saved_user_themes(&self.saved_user_themes) {
                                self.status = Some((format!("保存できませんでした: {e}"), false));
                                save_ok = false;
                            } else if !theme_change.any() {
                                self.status =
                                    Some(("ユーザーテーマを保存しました。".into(), true));
                            }
                        }
                        if save_ok && theme_change.user_theme_edited {
                            if let Err(e) = paths::save_user_theme(&self.user_theme) {
                                self.status = Some((format!("保存できませんでした: {e}"), false));
                                save_ok = false;
                            } else if !theme_change.theme_selected {
                                self.status = Some((
                                    "保存しました。アプリを切り替えると反映されます。".into(),
                                    true,
                                ));
                            }
                        }
                        if save_ok && theme_change.theme_selected {
                            self.save();
                            save_ok = self.status.as_ref().is_none_or(|(_, ok)| *ok);
                        }
                        if save_ok && theme_change.any() {
                            #[cfg(not(windows))]
                            {
                                self.status = Some(
                                    match fcitx_theme::apply(self.opts.theme, &self.user_theme) {
                                        Ok(()) => ("fcitx5 のテーマに設定しました。".into(), true),
                                        Err(e) => (e, false),
                                    },
                                );
                            }
                        }
                        #[cfg(not(windows))]
                        {
                            ui.add_space(4.0);
                            if ui.button("fcitx5 の標準テーマに戻す").clicked() {
                                self.status = Some(match fcitx_theme::restore() {
                                    Ok(()) => ("fcitx5 の標準テーマに戻しました。".into(), true),
                                    Err(e) => (e, false),
                                });
                            }
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

        section_header(ui, "変換");
        card_group(ui, |ui| {
            setting_row(
                ui,
                "ライブ変換",
                "入力しながら漢字に変換して表示します。Enter で確定、Esc で読みに戻ります。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.live_conversion).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "ライブ変換の待機時間（デバウンス）",
                "高速入力時のちらつきを抑えるため、AI ライブ変換を始める前に少し待ちます。0 ms でオフ。",
                |ui| {
                    ui.add_enabled_ui(self.opts.live_conversion, |ui| {
                        ui.spacing_mut().slider_width = 140.0;
                        let resp = ui.add(
                            egui::Slider::new(
                                &mut self.opts.live_debounce_ms,
                                0..=Options::MAX_LIVE_DEBOUNCE_MS,
                            )
                                .step_by(10.0)
                                .suffix(" ms"),
                        );
                        if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                            changed = true;
                        }
                        if self.opts.live_debounce_ms == 0 {
                            ui.label(RichText::new("オフ").small().weak());
                        }
                    });
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "予測変換",
                "入力中に続きの候補を表示します。Tab キーで選択できます。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.suggestions).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "Simeji クラウド変換",
                "スペース変換時にひと文節のかなを Simeji へ送信し、顔文字を候補に追加します。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.simeji_cloud).changed();
                },
            );
        });

        section_header(ui, "英字入力");
        card_group(ui, |ui| {
            setting_row(
                ui,
                "英単語の判定",
                "文の途中でもローマ字にならない英単語を変換します（例: softwarewo → softwareを）。F10 で直した単語も覚えます。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.english_words).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "Shift の英字入力を 1 単語で戻す",
                "Shift で始めた英字のあとにローマ字が続くと自動で日本語入力に戻します（例: Googlehakensaku → Google検索）。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.shift_one_word).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "英単語をカタカナで出す",
                "「best」→「ベスト」のようにカタカナ語を先に出します（英字は次の候補）。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.english_katakana).changed();
                },
            );
        });

        section_header(ui, "特殊候補");
        card_group(ui, |ui| {
            setting_row(
                ui,
                "日付と時刻",
                "「きょう」「あした」で日付、「いま」で時刻を候補に出します。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.dates).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "絵文字と顔文字",
                "「えがお」「ねこ」で絵文字、「かお」で顔文字を候補に出します。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.emoji).changed();
                },
            );
            setting_divider(ui);
            setting_row(
                ui,
                "計算",
                "「1+2*3=」のように = で終わる式を打つと答えを候補に出します。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.calculator).changed();
                },
            );
        });

        #[cfg(windows)]
        {
            section_header(ui, "表示と操作");
            card_group(ui, |ui| {
                setting_row(
                    ui,
                    "入力した文字を表示",
                    "「こんにちは」の近くに、打ったローマ字（konnnitiha）を表示します。",
                    |ui| {
                        changed |= toggle(ui, &mut self.opts.show_typed_input).changed();
                    },
                );
                setting_divider(ui);
                setting_row(
                    ui,
                    "確定の取り消し",
                    "確定した直後に Ctrl+Backspace を押すと、確定前の読みに戻します。",
                    |ui| {
                        changed |= toggle(ui, &mut self.opts.undo_commit).changed();
                    },
                );
                setting_divider(ui);
                setting_row(
                    ui,
                    "入力モードを表示",
                    "IME のオン/オフを切り替えたとき、文字カーソルの近くに「あ」「A」を表示します。",
                    |ui| {
                        changed |= toggle(ui, &mut self.opts.mode_indicator).changed();
                    },
                );
            });
        }

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
            [d.join("licenses"), d.join("../share/sweet-ime/licenses"), d.join("../Resources/licenses")].into_iter().find(|p| p.exists())
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
