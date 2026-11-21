//! Sweet IME setup: a graphical installer and uninstaller.
//!
//!   sweet-setup                     install the bundled files (elevates)
//!   sweet-setup --uninstall         uninstall (elevates)
//!   sweet-setup --bundle <dir> <out.exe>
//!                                   build a setup exe carrying <dir>
//!   sweet-setup --verify            list the bundled files
//!   sweet-setup --preview [--uninstall]
//!                                   show the UI only (no elevation, no changes)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ops;
mod payload;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use sweet_ui::eframe;
use eframe::egui::{self, Color32, RichText, Vec2};
use sweet_ui::{card, card_frame, toggle};

const ZENZ_MODEL: &str = "Miwa-Keita/zenz-v3.1-xsmall";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--bundle") {
        let (Some(dir), Some(out)) = (args.get(2), args.get(3)) else {
            eprintln!("usage: sweet-setup --bundle <dir> <out.exe>");
            std::process::exit(2);
        };
        match payload::bundle(dir.as_ref(), out.as_ref()) {
            Ok(()) => println!("wrote {out}"),
            Err(e) => {
                eprintln!("bundle failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("--verify") {
        // Decompress the payload and list it (for build checks).
        match payload::read() {
            Ok(files) => {
                for f in &files {
                    println!("{}	{}", f.name, f.data.len());
                }
            }
            Err(e) => {
                eprintln!("verify failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    // --preview: show the UI without elevating or changing anything.
    let preview = args.iter().any(|a| a == "--preview");
    if !preview && !elevated() {
        relaunch_elevated(&args[1..]);
        return;
    }
    let uninstall = args.iter().any(|a| a == "--uninstall");
    // Don't keep the install directory busy (uninstall.exe runs from it).
    let _ = std::env::set_current_dir(std::env::temp_dir());
    let title = if uninstall { "Sweet IME のアンインストール" } else { "Sweet IME のセットアップ" };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([640.0, 600.0])
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    let _ = eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            sweet_ui::setup(&cc.egui_ctx);
            let mut s = Setup::new(uninstall);
            s.preview = preview;
            Ok(Box::new(s))
        }),
    );
}

fn elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

/// Restarts this exe with administrator rights (UAC prompt).
fn relaunch_elevated(args: &[String]) {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    let Ok(exe) = std::env::current_exe() else { return };
    let params = args.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(" ");
    unsafe {
        ShellExecuteW(None, w!("runas"), &HSTRING::from(exe.as_os_str()), &HSTRING::from(params), None, SW_SHOWNORMAL);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StepState {
    Pending,
    Running,
    Done,
    Skipped,
    Failed,
}

enum Msg {
    Step(usize, StepState),
    Detail(String),
    /// Ok(true): finished, Ok(false): finished but a restart is needed.
    Finished(Result<bool, String>),
}

enum Page {
    Welcome,
    Progress,
    Finished(Result<bool, String>),
}

struct Setup {
    uninstall: bool,
    upgrade: bool,
    target: PathBuf,
    page: Page,
    // Install options.
    add_keyboard: bool,
    shortcut: bool,
    model: bool,
    // Uninstall option.
    remove_data: bool,
    steps: Vec<(&'static str, StepState)>,
    detail: String,
    rx: Option<Receiver<Msg>>,
    preview: bool,
}

impl Setup {
    fn new(uninstall: bool) -> Self {
        Self {
            uninstall,
            upgrade: ops::is_installed(),
            target: ops::target_dir(),
            page: Page::Welcome,
            add_keyboard: true,
            shortcut: true,
            model: false,
            remove_data: false,
            steps: Vec::new(),
            detail: String::new(),
            rx: None,
            preview: false,
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        if self.preview {
            // Sample progress screen, every icon state.
            use StepState::*;
            self.steps = [Done, Done, Failed, Skipped, Running, Pending]
                .into_iter()
                .zip(["完了", "完了", "失敗", "スキップ", "実行中", "待機中"])
                .map(|(st, l)| (l, st))
                .collect();
            self.detail = "プレビュー(何も変更していません)".into();
            self.page = Page::Progress;
            return;
        }
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.page = Page::Progress;
        let target = self.target.clone();
        let ctx = ctx.clone();
        if self.uninstall {
            self.steps = vec![
                ("キーボードの一覧から削除", StepState::Pending),
                ("Windows への登録を解除", StepState::Pending),
                ("スタートメニューから削除", StepState::Pending),
                ("アプリの一覧から削除", StepState::Pending),
                ("ファイルを削除", StepState::Pending),
                ("学習データと設定を削除", StepState::Pending),
            ];
            let remove_data = self.remove_data;
            std::thread::spawn(move || run_uninstall(&target, remove_data, &Reporter { tx, ctx }));
        } else {
            self.steps = vec![
                ("ファイルを展開", StepState::Pending),
                ("64 ビットアプリ用に登録", StepState::Pending),
                ("32 ビットアプリ用に登録", StepState::Pending),
                ("日本語キーボードに追加", StepState::Pending),
                ("スタートメニューに追加", StepState::Pending),
                ("アプリの一覧に登録", StepState::Pending),
                ("AI 変換モデルをダウンロード", StepState::Pending),
            ];
            let opts = (self.add_keyboard, self.shortcut, self.model);
            std::thread::spawn(move || run_install(&target, opts, &Reporter { tx, ctx }));
        }
    }
}

struct Reporter {
    tx: Sender<Msg>,
    ctx: egui::Context,
}

impl Reporter {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }

    /// Runs step `i` if `enabled`, reporting its state; returns its error.
    fn step(&self, i: usize, enabled: bool, f: impl FnOnce() -> ops::Result<()>) -> ops::Result<()> {
        if !enabled {
            self.send(Msg::Step(i, StepState::Skipped));
            return Ok(());
        }
        self.send(Msg::Step(i, StepState::Running));
        let r = f();
        self.send(Msg::Step(i, if r.is_ok() { StepState::Done } else { StepState::Failed }));
        r
    }
}

fn run_install(target: &std::path::Path, (keyboard, shortcut, model): (bool, bool, bool), r: &Reporter) {
    let result = (|| -> ops::Result<bool> {
        r.step(0, true, || {
            r.send(Msg::Detail("展開しています…".into()));
            let files = payload::read().map_err(|e| format!("インストーラーが壊れています: {e}"))?;
            ops::copy_files(&files, target)?;
            ops::write_uninstaller(target)
        })?;
        r.step(1, true, || ops::regsvr32(false, &target.join(r"x64\sweet_tsf.dll"), false))?;
        r.step(2, true, || ops::regsvr32(true, &target.join(r"x86\sweet_tsf.dll"), false))?;
        r.step(3, keyboard, ops::add_to_language_list)?;
        r.step(4, shortcut, || ops::create_shortcut(target))?;
        r.step(5, true, || ops::register_uninstall(target))?;
        // The model is optional: report a failure but keep the install.
        let model_result = r.step(6, model, || {
            ops::download_model(target, ZENZ_MODEL, |l| r.send(Msg::Detail(l)))
        });
        if let Err(e) = model_result {
            return Err(format!(
                "Sweet IME はインストールできましたが、AI 変換モデルのダウンロードに失敗しました。\
                 設定アプリからもう一度お試しください。\n{e}"
            ));
        }
        Ok(true)
    })();
    r.send(Msg::Detail(String::new()));
    r.send(Msg::Finished(result));
}

fn run_uninstall(target: &std::path::Path, remove_data: bool, r: &Reporter) {
    // Best effort: keep going so as much as possible is removed.
    let _ = r.step(0, true, ops::remove_from_language_list);
    let _ = r.step(1, true, || {
        let a = ops::regsvr32(false, &target.join(r"x64\sweet_tsf.dll"), true);
        let b = ops::regsvr32(true, &target.join(r"x86\sweet_tsf.dll"), true);
        a.and(b)
    });
    let _ = r.step(2, true, || {
        ops::remove_shortcut();
        Ok(())
    });
    let _ = r.step(3, true, || {
        ops::unregister_uninstall();
        Ok(())
    });
    let mut clean = true;
    let _ = r.step(4, true, || {
        clean = ops::remove_files(target);
        Ok(())
    });
    let _ = r.step(5, remove_data, || {
        ops::remove_user_data();
        Ok(())
    });
    r.send(Msg::Finished(Ok(clean)));
}

fn step_row(ui: &mut egui::Ui, label: &str, state: StepState) {
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        // Icons are painted: the UI font (Yu Gothic) has no check-mark glyphs.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::hover());
        if state == StepState::Running {
            egui::Spinner::new().size(16.0).paint_at(ui, rect.shrink(3.0));
        } else {
            paint_step_icon(ui.painter(), rect, state, ui.visuals());
        }
        let text = RichText::new(label);
        ui.label(if matches!(state, StepState::Pending | StepState::Skipped) { text.weak() } else { text });
    });
}

fn paint_step_icon(p: &egui::Painter, rect: egui::Rect, state: StepState, v: &egui::Visuals) {
    use egui::{Stroke, pos2};
    let c = rect.center();
    let r = 8.0;
    let at = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    match state {
        StepState::Pending => {
            p.circle_stroke(c, r - 1.0, Stroke::new(1.5, v.weak_text_color()));
        }
        StepState::Skipped => {
            p.circle_stroke(c, r - 1.0, Stroke::new(1.5, v.weak_text_color()));
            p.line_segment([at(-3.5, 0.0), at(3.5, 0.0)], Stroke::new(1.5, v.weak_text_color()));
        }
        StepState::Done => {
            p.circle_filled(c, r, Color32::from_rgb(16, 150, 80));
            let s = Stroke::new(2.0, Color32::WHITE);
            p.line_segment([at(-4.0, 0.2), at(-1.2, 3.0)], s);
            p.line_segment([at(-1.2, 3.0), at(4.2, -2.8)], s);
        }
        StepState::Failed => {
            p.circle_filled(c, r, v.error_fg_color);
            let s = Stroke::new(2.0, Color32::WHITE);
            p.line_segment([at(-3.0, -3.0), at(3.0, 3.0)], s);
            p.line_segment([at(-3.0, 3.0), at(3.0, -3.0)], s);
        }
        StepState::Running => {}
    }
}

impl eframe::App for Setup {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    Msg::Step(i, s) => self.steps[i].1 = s,
                    Msg::Detail(d) => self.detail = d,
                    Msg::Finished(r) => self.page = Page::Finished(r),
                }
            }
        }
        let ctx = ui.ctx().clone();
        egui::Panel::bottom("buttons").show_separator_line(false).show(ui, |ui| {
            ui.add_space(10.0);
            let row = Vec2::new(ui.available_width(), 36.0);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                match &self.page {
                    Page::Welcome => {
                        let label = match (self.uninstall, self.upgrade) {
                            (true, _) => "アンインストール",
                            (false, true) => "更新",
                            (false, false) => "インストール",
                        };
                        if ui.add(sweet_ui::primary_button(label)).clicked() {
                            self.start(&ctx);
                        }
                        if ui.add(egui::Button::new("キャンセル").min_size(Vec2::new(100.0, 34.0))).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    Page::Progress => {
                        ui.add_enabled(false, egui::Button::new("お待ちください…").min_size(Vec2::new(120.0, 34.0)));
                    }
                    Page::Finished(r) => {
                        if ui.add(sweet_ui::primary_button("閉じる")).clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if !self.uninstall
                            && r.is_ok()
                            && ui.add(egui::Button::new("設定を開く").min_size(Vec2::new(110.0, 34.0))).clicked()
                        {
                            ops::open_settings(&self.target);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            });
            ui.add_space(10.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 16)).show(ui, |ui| match &self.page {
                    Page::Welcome if self.uninstall => self.uninstall_welcome(ui),
                    Page::Welcome => self.install_welcome(ui),
                    Page::Progress => self.progress(ui),
                    Page::Finished(r) => {
                        let r = r.clone();
                        self.finished(ui, &r);
                    }
                });
            });
        });
    }
}

impl Setup {
    fn install_welcome(&mut self, ui: &mut egui::Ui) {
        if !payload::has_payload() {
            sweet_ui::title(ui, "Sweet IME");
            ui.colored_label(ui.visuals().error_fg_color, "このプログラムにはインストールするファイルが含まれていません。");
            return;
        }
        sweet_ui::title(ui, if self.upgrade { "Sweet IME を更新" } else { "Sweet IME へようこそ" });
        ui.label(RichText::new("高精度で高速な日本語入力システムです。").weak());
        ui.add_space(6.0);
        card(ui, "日本語キーボードに追加", "Windows のキーボードの一覧に Sweet IME を追加します。", |ui| {
            toggle(ui, &mut self.add_keyboard);
        });
        card(ui, "スタートメニューに追加", "「Sweet IME の設定」をスタートメニューに追加します。", |ui| {
            toggle(ui, &mut self.shortcut);
        });
        card(
            ui,
            "AI 変換モデルもダウンロード",
            "文脈に合った候補を選ぶ zenz モデル(約 100 MB、作者: Miwa Keita、CC BY-SA 4.0)を Hugging Face から取得します。あとから設定でも追加できます。",
            |ui| {
                toggle(ui, &mut self.model);
            },
        );
        ui.add_space(4.0);
        ui.label(RichText::new(format!("インストール先: {}", self.target.display())).small().weak());
        ui.label(
            RichText::new("Sweet IME は MIT ライセンスです。同梱の辞書は Mozc OSS 辞書(IPAdic などを含む)のライセンスに従います。")
                .small()
                .weak(),
        );
    }

    fn uninstall_welcome(&mut self, ui: &mut egui::Ui) {
        sweet_ui::title(ui, "Sweet IME をアンインストール");
        ui.label(RichText::new("Sweet IME をこのパソコンから削除します。").weak());
        ui.add_space(6.0);
        card(ui, "学習データと設定も削除", "覚えた単語、設定、ダウンロードした AI 変換モデルを削除します。", |ui| {
            toggle(ui, &mut self.remove_data);
        });
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
        sweet_ui::title(ui, if self.uninstall { "アンインストールしています…" } else { "インストールしています…" });
        ui.add_space(6.0);
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (label, state) in &self.steps {
                step_row(ui, label, *state);
            }
        });
        if !self.detail.is_empty() {
            ui.label(RichText::new(&self.detail).small().weak());
        }
    }

    fn finished(&mut self, ui: &mut egui::Ui, r: &Result<bool, String>) {
        match r {
            Ok(clean) => {
                sweet_ui::title(ui, if self.uninstall { "アンインストールしました" } else { "準備ができました" });
                ui.add_space(6.0);
                card_frame(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if self.uninstall {
                        ui.label("Sweet IME を削除しました。");
                        if !clean {
                            ui.label(RichText::new("使用中のファイルは、次にパソコンを再起動したときに削除されます。").weak());
                        }
                    } else {
                        ui.label("入力方式の切り替え(Win + Space)で Sweet IME を選ぶと使えます。");
                        ui.label(
                            RichText::new(
                                "すでに開いているアプリは、アプリを開き直すと新しい版になります。\
                                 タスクバーの検索などは、サインインし直すと反映されます。",
                            )
                            .weak(),
                        );
                    }
                });
            }
            Err(e) => {
                sweet_ui::title(ui, if self.uninstall { "アンインストールできませんでした" } else { "問題が発生しました" });
                ui.add_space(6.0);
                card_frame(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (label, state) in &self.steps {
                        step_row(ui, label, *state);
                    }
                });
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        }
    }
}
