//! Update notices: the banner and the 情報 page section of the settings app,
//! and the check at sign-in (`--check-update`), which opens a small window
//! on Windows and shows a desktop notification on Linux.

use sweet_ui::eframe::egui;
use egui::RichText;
#[cfg(not(target_os = "macos"))]
use {egui::Vec2, sweet_ui::eframe};
use sweet_ui::{card, card_frame, toggle};

use crate::update::{Release, current, State, Updater};
#[cfg(not(target_os = "macos"))]
use crate::update::{self, Remembered};

fn notes(ui: &mut egui::Ui, r: &Release, height: f32) {
    if r.notes.is_empty() {
        return;
    }
    egui::ScrollArea::vertical().id_salt("release-notes").max_height(height).show(ui, |ui| {
        ui.label(RichText::new(&r.notes).small().weak());
    });
}

/// Why this copy cannot update itself (Nix and other package managers).
fn manual_update() -> Option<String> {
    #[cfg(not(any(windows, target_os = "macos")))]
    return update::linux_install().err();
    #[cfg(any(windows, target_os = "macos"))]
    None
}

/// The update button, or how to update with the package manager.
fn actions(ui: &mut egui::Ui, u: &mut Updater, r: &Release) {
    let manual = manual_update();
    ui.horizontal(|ui| {
        if manual.is_none() && ui.add(sweet_ui::primary_button(ui, "更新する")).clicked() {
            u.install(ui.ctx());
        }
        if !r.page.is_empty() {
            ui.hyperlink_to("リリースノート", &r.page);
        }
    });
    if let Some(manual) = manual {
        ui.label(RichText::new(manual).small().weak());
    }
}

/// Shown after an in-place update.
#[cfg(not(windows))]
const INSTALLED: &str = if cfg!(target_os = "macos") {
    "更新しました。ログアウトして再ログインすると反映されます。"
} else {
    "更新して fcitx5 を再起動しました。設定アプリは開き直すと新しいバージョンになります。"
};

fn downloading(ui: &mut egui::Ui, u: &Updater, r: &Release) {
    ui.label(format!("バージョン {} をダウンロードしています…", r.version));
    match u.progress() {
        Some(p) => {
            let p = p.clamp(0.0, 1.0);
            ui.horizontal(|ui| {
                let pct = format!("{:.0}%", (p * 100.0).round());
                let bar_w = (ui.available_width() - 48.0).max(80.0);
                ui.add(egui::ProgressBar::new(p).desired_width(bar_w));
                ui.label(pct);
            });
        }
        None => {
            ui.spinner();
        }
    }
}

/// A notice above every page while an update is available or downloading.
pub fn banner(ui: &mut egui::Ui, u: &mut Updater) {
    #[cfg(not(windows))]
    if matches!(u.state, State::Installed) {
        card_frame(ui).show(ui, |ui| {
            ui.label(INSTALLED);
        });
        return;
    }
    let r = match &u.state {
        State::Available(r) | State::Downloading(r) => r.clone(),
        _ => return,
    };
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if matches!(u.state, State::Downloading(_)) {
            downloading(ui, u, &r);
        } else {
            ui.label(RichText::new(format!("新しいバージョン {} があります", r.version)).size(15.5).strong());
            ui.label(RichText::new(format!("いまのバージョン: {}", current())).small().weak());
            actions(ui, u, &r);
        }
    });
    ui.add_space(4.0);
}

/// The アップデート section of the 情報 page. Returns whether the automatic
/// check setting changed.
pub fn section(ui: &mut egui::Ui, u: &mut Updater, auto: &mut bool) -> bool {
    #[cfg(windows)]
    const DESC: &str = "設定アプリを開いたときと、サインインしたとき(1 日 1 回まで)に GitHub で新しいバージョンを確認します。";
    #[cfg(target_os = "macos")]
    const DESC: &str = "設定アプリを開いたときに GitHub で新しいバージョンを確認します。";
    #[cfg(not(any(windows, target_os = "macos")))]
    const DESC: &str = "設定アプリを開いたときと、fcitx5 が起動したとき(1 日 1 回まで)に GitHub で新しいバージョンを確認し、デスクトップに通知します。";
    let mut changed = false;
    card(ui, "アップデートを自動で確認", DESC, |ui| changed = toggle(ui, auto).changed());
    card_frame(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("アップデート").size(15.5));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!u.busy(), egui::Button::new("今すぐ確認")).clicked() {
                    u.check(ui.ctx());
                }
            });
        });
        match &u.state {
            State::Idle => {
                ui.label(RichText::new(format!("いまのバージョン: {}", current())).weak());
            }
            State::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("確認しています…");
                });
            }
            State::UpToDate => {
                ui.label(format!("最新のバージョン({})です。", current()));
            }
            State::Available(r) => {
                let r = r.clone();
                ui.label(format!("バージョン {} が公開されています(いまは {})。", r.version, current()));
                notes(ui, &r, 160.0);
                actions(ui, u, &r);
            }
            State::Downloading(r) => {
                let r = r.clone();
                downloading(ui, u, &r);
            }
            #[cfg(windows)]
            State::Launched => {
                ui.label("インストーラーを起動しました。");
            }
            #[cfg(not(windows))]
            State::Installed => {
                ui.label(INSTALLED);
            }
            State::Failed(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e);
            }
        }
    });
    changed
}

/// `--check-update`: checks quietly (at sign-in on Windows, when fcitx5
/// starts on Linux) and tells the user only when there is a new version
/// they haven't skipped.
#[cfg(not(target_os = "macos"))]
pub fn check_at_sign_in() -> eframe::Result<()> {
    if !sweet_engine::paths::load_options().update_check {
        return Ok(());
    }
    let mut m = Remembered::load();
    if !m.due() {
        return Ok(());
    }
    let Ok(r) = update::fetch_latest() else { return Ok(()) };
    m.checked();
    if !update::is_newer(&r.version, &current()) || r.version == m.skip {
        return Ok(());
    }
    // Linux: a desktop notification. Its button opens the window below.
    #[cfg(not(windows))]
    match update::notify(&r) {
        Some(update::NotifyChoice::Open) | None => {}
        Some(update::NotifyChoice::Skip) => {
            m.skip = r.version.clone();
            m.save();
            return Ok(());
        }
        Some(update::NotifyChoice::Dismissed) => return Ok(()),
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Sweet IME のアップデート")
            .with_icon(sweet_ui::app_icon())
            .with_app_id("sweet-settings")
            .with_inner_size([520.0, 360.0])
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    eframe::run_native(
        "Sweet IME のアップデート",
        options,
        Box::new(move |cc| {
            sweet_ui::setup(&cc.egui_ctx);
            Ok(Box::new(Prompt { updater: Updater::with_release(r) }))
        }),
    )
}

#[cfg(not(target_os = "macos"))]
struct Prompt {
    updater: Updater,
}

#[cfg(not(target_os = "macos"))]
impl eframe::App for Prompt {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.updater.poll(&ctx);
        #[cfg(windows)]
        if matches!(self.updater.state, State::Launched) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 12)).show(ui, |ui| {
                let u = &mut self.updater;
                match &u.state {
                    State::Available(r) => {
                        let r = r.clone();
                        sweet_ui::title(ui, "新しいバージョンがあります");
                        ui.label(format!("Sweet IME {} が公開されています(いまは {})。", r.version, current()));
                        ui.add_space(4.0);
                        notes(ui, &r, 140.0);
                        ui.add_space(8.0);
                        let manual = manual_update();
                        if let Some(manual) = &manual {
                            ui.label(RichText::new(manual).small().weak());
                        }
                        ui.horizontal(|ui| {
                            if manual.is_none() && ui.add(sweet_ui::primary_button(ui, "更新する")).clicked() {
                                u.install(&ctx);
                            }
                            if ui.add(egui::Button::new("あとで").min_size(Vec2::new(90.0, 34.0))).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if ui.add(egui::Button::new("このバージョンはスキップ").min_size(Vec2::new(90.0, 34.0))).clicked() {
                                let mut m = Remembered::load();
                                m.skip = r.version.clone();
                                m.save();
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                        if !r.page.is_empty() {
                            ui.hyperlink_to("リリースノート", &r.page);
                        }
                    }
                    State::Downloading(r) => {
                        let r = r.clone();
                        sweet_ui::title(ui, "ダウンロード中");
                        downloading(ui, u, &r);
                    }
                    #[cfg(not(windows))]
                    State::Installed => {
                        sweet_ui::title(ui, "更新しました");
                        ui.label(INSTALLED);
                        if ui.button("閉じる").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    State::Failed(e) => {
                        sweet_ui::title(ui, "更新できませんでした");
                        ui.colored_label(ui.visuals().error_fg_color, e);
                        if ui.button("閉じる").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    _ => {}
                }
            });
        });
    }
}
