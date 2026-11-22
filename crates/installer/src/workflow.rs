//! Installation and removal steps executed by the setup worker.

use std::path::Path;
use std::sync::mpsc::Sender;

use sweet_ui::eframe::egui;

use crate::{ops, payload};

const ZENZ_MODEL: &str = "Miwa-Keita/zenz-v3.1-xsmall";

pub(super) struct InstallOptions {
    pub(super) keyboard: bool,
    pub(super) shortcut: bool,
    pub(super) model: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum StepState {
    Pending,
    Running,
    Done,
    Skipped,
    Failed,
}

pub(super) enum Msg {
    Step(usize, StepState),
    Detail(String),
    /// Ok(true): finished, Ok(false): finished but a restart is needed.
    Finished(Result<bool, String>),
}

pub(super) struct Reporter {
    pub(super) tx: Sender<Msg>,
    pub(super) ctx: egui::Context,
}

impl Reporter {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }

    /// Runs step `i` if `enabled`, reporting its state; returns its error.
    fn step(
        &self,
        i: usize,
        enabled: bool,
        f: impl FnOnce() -> ops::Result<()>,
    ) -> ops::Result<()> {
        if !enabled {
            self.send(Msg::Step(i, StepState::Skipped));
            return Ok(());
        }
        self.send(Msg::Step(i, StepState::Running));
        let r = f();
        self.send(Msg::Step(
            i,
            if r.is_ok() {
                StepState::Done
            } else {
                StepState::Failed
            },
        ));
        r
    }
}

pub(super) fn run_install(
    target: &Path,
    InstallOptions {
        keyboard,
        shortcut,
        model,
    }: InstallOptions,
    r: &Reporter,
) {
    let result = (|| -> ops::Result<bool> {
        r.step(0, true, || {
            r.send(Msg::Detail("展開しています…".into()));
            let files =
                payload::read().map_err(|e| format!("インストーラーが壊れています: {e}"))?;
            ops::copy_files(&files, target)?;
            ops::write_uninstaller(target)
        })?;
        r.step(1, true, || {
            ops::regsvr32(false, &target.join(r"x64\sweet_tsf.dll"), false)
        })?;
        r.step(2, true, || {
            ops::regsvr32(true, &target.join(r"x86\sweet_tsf.dll"), false)
        })?;
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

pub(super) fn run_uninstall(target: &Path, remove_data: bool, r: &Reporter) {
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
