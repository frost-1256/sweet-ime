//! Neural model settings and progress from the download process.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use sweet_engine::paths;
use sweet_ui::eframe::egui::{self, RichText};
use sweet_ui::{card, card_frame, toggle};

use super::App;

pub(super) const MODELS: [(&str, &str); 2] = [
    ("Miwa-Keita/zenz-v3.1-xsmall", "xsmall(約 100 MB・高速)"),
    ("Miwa-Keita/zenz-v3.1-small", "small(約 380 MB・高精度)"),
];

/// A running model download (get-zenz.ps1).
pub(super) struct Download {
    log: Vec<String>,
    lines: Receiver<DownloadEvent>,
    done: Option<bool>,
}

enum DownloadEvent {
    Output(String),
    Finished(bool),
}

impl Download {
    pub(super) fn poll(&mut self) {
        while let Ok(event) = self.lines.try_recv() {
            match event {
                DownloadEvent::Output(line) => self.log.push(line),
                DownloadEvent::Finished(success) => self.done = Some(success),
            }
        }
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
    [
        dir.join(GET_ZENZ),
        dir.join("../share/sweet-ime").join(GET_ZENZ),
        dir.join("../../scripts").join(GET_ZENZ),
    ]
    .into_iter()
    .find(|p| p.exists())
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
            let _ = tx2.send(DownloadEvent::Output(line));
            ctx2.request_repaint();
        }
    });
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            let _ = tx.send(DownloadEvent::Output(line));
            ctx.request_repaint();
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        let _ = tx.send(DownloadEvent::Finished(ok));
        ctx.request_repaint();
    });
    Ok(Download {
        log: Vec::new(),
        lines: rx,
        done: None,
    })
}

impl App {
    pub(super) fn neural_page(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;
        card(
            ui,
            "AI 変換を使う",
            "文脈に合わせてニューラル言語モデル(zenz)で変換候補を選び直します。",
            |ui| {
                changed |= toggle(ui, &mut self.opts.neural).changed();
            },
        );
        if changed {
            self.save();
        }

        let zenz = paths::zenz_file();
        let size = zenz
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len());
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
}
