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
mod theme_page;
mod update;
mod update_ui;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use sweet_ui::eframe;
use eframe::egui::{self, Color32, RichText, Vec2};
use sweet_ui::{card, card_frame, toggle};
use sweet_engine::learning::Learned;
use sweet_engine::{Options, UserHistory, paths};

fn main() -> eframe::Result<()> {
    if std::env::args().any(|a| a == "--check-update") {
        return update_ui::check_at_sign_in();
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

/// A running model download (get-zenz.ps1).
struct Download {
    log: Vec<String>,
    lines: Receiver<Option<String>>,
    done: Option<bool>,
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
    updater: update::Updater,
}

const MODELS: [(&str, &str); 2] = [
    ("Miwa-Keita/zenz-v3.1-xsmall", "xsmall(約 100 MB・高速)"),
    ("Miwa-Keita/zenz-v3.1-small", "small(約 380 MB・高精度)"),
];


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


fn human_size(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}

#[cfg(windows)]
const GET_ZENZ: &str = "get-zenz.ps1";
#[cfg(not(windows))]
const GET_ZENZ: &str = "get-zenz.sh";

/// The model download script: next to this program (Windows install), in
/// `../share/sweet-ime` (Linux install) or in scripts/ (development).
fn get_zenz_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [dir.join(GET_ZENZ), dir.join("../share/sweet-ime").join(GET_ZENZ), dir.join("../../scripts").join(GET_ZENZ)]
        .into_iter()
        .find(|p| p.exists())
}

/// Opens a folder in the file manager.
fn open_folder(dir: &std::path::Path) {
    #[cfg(windows)]
    let opener = "explorer.exe";
    #[cfg(not(windows))]
    let opener = "xdg-open";
    let _ = std::process::Command::new(opener).arg(dir).spawn();
}

fn start_download(model: &str, ctx: egui::Context) -> Result<Download, String> {
    use std::io::BufRead;
    let script = get_zenz_script().ok_or_else(|| format!("{GET_ZENZ} が見つかりません"))?;
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .args(["-Model", model])
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = std::process::Command::new("sh");
        c.arg(&script).arg(model);
        c
    };
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("ダウンロードを開始できません: {e}"))?;
    let (tx, rx) = channel();
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let tx2 = tx.clone();
    let ctx2 = ctx.clone();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            let _ = tx2.send(Some(line));
            ctx2.request_repaint();
        }
    });
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(Some(line));
            ctx.request_repaint();
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        let _ = tx.send(Some(if ok { "__ok__".into() } else { "__fail__".into() }));
        let _ = tx.send(None);
        ctx.request_repaint();
    });
    Ok(Download { log: Vec::new(), lines: rx, done: None })
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.updater.poll(ui.ctx());
        if matches!(self.updater.state, update::State::Launched) {
            // Let the installer replace this program.
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Drain download output.
        if let Some(d) = &mut self.download {
            while let Ok(Some(line)) = d.lines.try_recv() {
                match line.as_str() {
                    "__ok__" => d.done = Some(true),
                    "__fail__" => d.done = Some(false),
                    _ => d.log.push(line),
                }
            }
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
        card(ui, "英単語の判定", "「korehabestdesu」→「これはベストです」、「softwarewo」→「softwareを」のように、ローマ字にならない英単語を文の途中でも変換します。F10 で英字に直した単語も覚えます。", |ui| {
            changed |= toggle(ui, &mut self.opts.english_words).changed();
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

    fn neural_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        card(ui, "AI 変換を使う", "文脈に合わせてニューラル言語モデル(zenz)で変換候補を選び直します。", |ui| {
            changed |= toggle(ui, &mut self.opts.neural).changed();
        });
        if changed {
            self.save();
        }

        let zenz = paths::zenz_file();
        let size = zenz.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
        let state = match size {
            Some(n) => format!("インストール済み({})", human_size(n)),
            None => "未インストール(辞書だけで変換します)".into(),
        };
        card(ui, "モデル", &state, |ui| {
            if size.is_some() && self.download.is_none() && ui.button("削除").clicked() {
                if let Some(p) = &zenz {
                    // The IME may have it mapped: move it aside first.
                    let old = p.with_extension("swz.old");
                    let _ = std::fs::remove_file(&old);
                    let r = std::fs::rename(p, &old);
                    let _ = std::fs::remove_file(&old);
                    self.status = Some(match r {
                        Ok(()) => ("モデルを削除しました。".into(), true),
                        Err(e) => (format!("削除できませんでした: {e}"), false),
                    });
                }
            }
        });

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("モデルのダウンロード").size(15.5));
            ui.label(
                RichText::new(
                    "zenz モデル(作者: Miwa Keita、ライセンス: CC BY-SA 4.0)を Hugging Face からダウンロードして変換します。\
                     Sweet IME には同梱されていません。",
                )
                .small()
                .weak(),
            );
            let busy = self.download.as_ref().is_some_and(|d| d.done.is_none());
            ui.horizontal(|ui| {
                for (id, label) in MODELS {
                    ui.add_enabled_ui(!busy, |ui| ui.radio_value(&mut self.model, id, label));
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("ダウンロード")).clicked() {
                    match start_download(self.model, ui.ctx().clone()) {
                        Ok(d) => self.download = Some(d),
                        Err(e) => self.status = Some((e, false)),
                    }
                }
                ui.hyperlink_to("モデルのページ", format!("https://huggingface.co/{}", self.model));
            });
            if let Some(d) = &self.download {
                match d.done {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(d.log.last().map_or("準備中…", String::as_str));
                        });
                    }
                    Some(true) => {
                        ui.label("完了しました。アプリを切り替えると使われます。");
                    }
                    Some(false) => {
                        ui.colored_label(ui.visuals().error_fg_color, "失敗しました:");
                        for l in d.log.iter().rev().take(4).rev() {
                            ui.label(RichText::new(l).small().monospace());
                        }
                    }
                }
            }
        });
    }

    fn learning_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        card(ui, "学習する", "選んだ候補を覚えて、次から優先します。オフにすると覚えた内容も使いません。", |ui| {
            changed |= toggle(ui, &mut self.opts.learning).changed();
        });
        card(ui, "パスワード欄では学習しない", "パスワード欄やブラウザーのプライベートウィンドウで入力した内容は覚えません。", |ui| {
            changed |= toggle(ui, &mut self.opts.private_fields).changed();
        });
        if changed {
            self.save();
        }
        let Some(_) = self.history else {
            ui.label("学習データの場所がわかりません(APPDATA が未設定)。");
            return;
        };
        let mut forget_within = None;
        card(ui, "最近の学習を消す", "うっかり覚えさせた変換を、時間を区切って取り消します。", |ui| {
            if ui.button("今日(24 時間)").clicked() {
                forget_within = Some((24 * 3600, "24 時間"));
            }
            if ui.button("直近 1 時間").clicked() {
                forget_within = Some((3600, "1 時間"));
            }
        });
        if let Some((secs, label)) = forget_within {
            let r = self.history.as_mut().map(|h| h.forget_recent(std::time::Duration::from_secs(secs)));
            self.reload_history();
            self.status = match r {
                Some(Ok(n)) => Some((format!("直近 {label}に覚えた {n} 件を消しました。"), true)),
                Some(Err(e)) => Some((format!("消去できませんでした: {e}"), false)),
                None => None,
            };
        }

        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("覚えた単語({} 件)", self.learned.len())).size(15.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.confirm_clear {
                        if ui.button("取り消し").clicked() {
                            self.confirm_clear = false;
                        }
                        let danger = egui::Button::new(RichText::new("本当にすべて消去").color(Color32::WHITE))
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
                    } else if ui.add_enabled(!self.learned.is_empty(), egui::Button::new("すべて消去")).clicked() {
                        self.confirm_clear = true;
                    }
                    if ui.button("更新").clicked() {
                        self.reload_history();
                    }
                });
            });
            ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("読みや単語で絞り込み").desired_width(f32::INFINITY));
            let f = self.filter.trim();
            let rows: Vec<usize> = (0..self.learned.len())
                .filter(|&i| {
                    let (r, e) = &self.learned[i];
                    f.is_empty() || r.contains(f) || e.surface.contains(f)
                })
                .collect();
            if rows.is_empty() {
                ui.label(RichText::new("まだありません。変換で 2 番目以降の候補を選ぶと覚えます。").weak());
            }
            let mut forget = None;
            let row_h = 30.0;
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show_rows(
                ui,
                row_h,
                rows.len(),
                |ui, range| {
                    for &i in &rows[range] {
                        let (reading, e) = &self.learned[i];
                        ui.horizontal(|ui| {
                            ui.set_min_height(row_h - 6.0);
                            ui.label(RichText::new(&e.surface).size(16.0));
                            ui.label(RichText::new(reading).weak());
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("削除").clicked() {
                                    forget = Some(i);
                                }
                                ui.label(RichText::new(format!("{} 回", e.count)).small().weak());
                            });
                        });
                    }
                },
            );
            if let Some(i) = forget {
                let (reading, e) = self.learned[i].clone();
                if let Some(h) = &mut self.history {
                    h.forget(&reading, &e.surface);
                }
                self.reload_history();
            }
        });
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
