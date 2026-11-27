//! Neural model settings, model switching, deletion, and download progress.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use sweet_engine::paths;
use sweet_ui::eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText};
use sweet_ui::{
    card_frame, card_group, contrasting_ink, section_header, setting_divider, setting_row,
    setting_row_custom, toggle,
};

use super::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModelKind {
    Xsmall,
    Small,
}

impl ModelKind {
    const ALL: [Self; 2] = [Self::Xsmall, Self::Small];

    const fn repo(self) -> &'static str {
        match self {
            Self::Xsmall => "Miwa-Keita/zenz-v3.1-xsmall",
            Self::Small => "Miwa-Keita/zenz-v3.1-small",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Xsmall => "zenz v3.1 xsmall",
            Self::Small => "zenz v3.1 small",
        }
    }

    const fn stored_filename(self) -> &'static str {
        match self {
            Self::Xsmall => "zenz-xsmall.swz",
            Self::Small => "zenz-small.swz",
        }
    }

    const fn summary(self) -> &'static str {
        match self {
            Self::Xsmall => "軽量・高速（約 100 MB・平均 約 3.4 ms）— ライブ変換におすすめ",
            Self::Small => "高精度（約 380 MB・平均 約 12.4 ms）— 長文や固有名詞に強い",
        }
    }

    const fn recommended(self) -> bool {
        matches!(self, Self::Xsmall)
    }

    fn url(self) -> String {
        format!("https://huggingface.co/{}", self.repo())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModelSlot {
    Active(u64),
    Downloaded(u64),
    NotInstalled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ModelInventory {
    xsmall: ModelSlot,
    small: ModelSlot,
    custom_active: Option<u64>,
}

impl ModelInventory {
    fn slot(&self, kind: ModelKind) -> ModelSlot {
        match kind {
            ModelKind::Xsmall => self.xsmall,
            ModelKind::Small => self.small,
        }
    }

    fn has_active(&self) -> bool {
        matches!(self.xsmall, ModelSlot::Active(_))
            || matches!(self.small, ModelSlot::Active(_))
            || self.custom_active.is_some()
    }
}

/// Identifies a packed `.swz` file by its size (xsmall is ~98 MB, small is ~380 MB).
fn classify_swz_size(bytes: u64) -> Option<ModelKind> {
    match bytes {
        80_000_000..=160_000_000 => Some(ModelKind::Xsmall),
        280_000_000..=480_000_000 => Some(ModelKind::Small),
        _ => None,
    }
}

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

fn inspect_models(data_dir: &Path) -> ModelInventory {
    let mut inv = ModelInventory {
        xsmall: ModelSlot::NotInstalled,
        small: ModelSlot::NotInstalled,
        custom_active: None,
    };
    if let Some(sz) = file_size(&data_dir.join("zenz.swz")) {
        match classify_swz_size(sz) {
            Some(ModelKind::Xsmall) => inv.xsmall = ModelSlot::Active(sz),
            Some(ModelKind::Small) => inv.small = ModelSlot::Active(sz),
            None => inv.custom_active = Some(sz),
        }
    }
    if inv.xsmall == ModelSlot::NotInstalled
        && let Some(sz) = file_size(&data_dir.join(ModelKind::Xsmall.stored_filename()))
    {
        inv.xsmall = ModelSlot::Downloaded(sz);
    }
    if inv.small == ModelSlot::NotInstalled
        && let Some(sz) = file_size(&data_dir.join(ModelKind::Small.stored_filename()))
    {
        inv.small = ModelSlot::Downloaded(sz);
    }
    inv
}

/// Removes a `.swz` file safely even if another process has it memory-mapped
/// on Windows (renaming an open file on the same volume succeeds).
fn safe_remove(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let old = path.with_extension("swz.old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(path, &old)?;
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// Switches `zenz.swz` to `target`, preserving the previously active known
/// model in its named file so switching back does not require re-downloading.
/// Keeps a reversible backup of the previous active file until the final
/// rename succeeds.
fn activate_model(data_dir: &Path, target: ModelKind) -> Result<(), String> {
    let active = data_dir.join("zenz.swz");
    let source = data_dir.join(target.stored_filename());
    if !source.exists() {
        if file_size(&active).and_then(classify_swz_size) == Some(target) {
            return Ok(());
        }
        return Err(format!("{} のファイルが見つかりません。", target.name()));
    }
    let mut backup: Option<(PathBuf, bool)> = None;
    if let Some(sz) = file_size(&active) {
        match classify_swz_size(sz) {
            Some(old) if old != target => {
                let archive = data_dir.join(old.stored_filename());
                safe_remove(&archive)
                    .map_err(|e| format!("既存モデルの退避に失敗しました: {e}"))?;
                std::fs::rename(&active, &archive)
                    .map_err(|e| format!("既存モデルの退避に失敗しました: {e}"))?;
                backup = Some((archive, false));
            }
            _ => {
                let temp = active.with_extension("swz.bak");
                let _ = std::fs::remove_file(&temp);
                std::fs::rename(&active, &temp)
                    .map_err(|e| format!("既存モデルの退避に失敗しました: {e}"))?;
                backup = Some((temp, true));
            }
        }
    }
    if let Err(e) = std::fs::rename(&source, &active) {
        if let Some((path, _)) = backup {
            let _ = std::fs::rename(&path, &active);
        }
        return Err(format!("モデルの切り替えに失敗しました: {e}"));
    }
    if let Some((temp, true)) = backup {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(())
}

/// Deletes `target` whether it is currently active or stored as inactive.
fn delete_model(data_dir: &Path, target: ModelKind) -> Result<(), String> {
    let active = data_dir.join("zenz.swz");
    if file_size(&active).and_then(classify_swz_size) == Some(target) {
        safe_remove(&active).map_err(|e| format!("削除できませんでした: {e}"))?;
    }
    let stored = data_dir.join(target.stored_filename());
    if stored.exists() {
        safe_remove(&stored).map_err(|e| format!("削除できませんでした: {e}"))?;
    }
    Ok(())
}

fn delete_custom_model(data_dir: &Path) -> Result<(), String> {
    let active = data_dir.join("zenz.swz");
    safe_remove(&active).map_err(|e| format!("削除できませんでした: {e}"))
}

/// Activates a staged `.swz` file (from custom `.swz` or `.gguf` import) as
/// `zenz.swz`, preserving any previously active built-in model (`xsmall`/`small`)
/// and restoring the previous `zenz.swz` if the final rename fails.
fn activate_staged_custom(data_dir: &Path, staged: &Path) -> Result<(), String> {
    if !staged.exists() {
        return Err("変換後のモデルファイルが見つかりません。".into());
    }
    let active = data_dir.join("zenz.swz");
    let mut backup: Option<(PathBuf, bool)> = None;
    if let Some(sz) = file_size(&active) {
        match classify_swz_size(sz) {
            Some(old) => {
                let archive = data_dir.join(old.stored_filename());
                safe_remove(&archive)
                    .map_err(|e| format!("既存モデルの退避に失敗しました: {e}"))?;
                std::fs::rename(&active, &archive)
                    .map_err(|e| format!("既存モデルの退避に失敗しました: {e}"))?;
                backup = Some((archive, false));
            }
            None => {
                let temp = active.with_extension("swz.bak");
                let _ = std::fs::remove_file(&temp);
                std::fs::rename(&active, &temp)
                    .map_err(|e| format!("既存モデルの退避に失敗しました: {e}"))?;
                backup = Some((temp, true));
            }
        }
    }
    if let Err(e) = std::fs::rename(staged, &active) {
        if let Some((path, _)) = backup {
            let _ = std::fs::rename(&path, &active);
        }
        return Err(format!("モデルの適用に失敗しました: {e}"));
    }
    if let Some((temp, true)) = backup {
        let _ = std::fs::remove_file(&temp);
    }
    Ok(())
}

fn read_magic(path: &Path) -> Result<[u8; 8], String> {
    use std::io::Read;
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("ファイルを開けませんでした: {e}"))?;
    let mut buf = [0u8; 8];
    f.read_exact(&mut buf)
        .map_err(|e| format!("ファイルを読み込めませんでした: {e}"))?;
    Ok(buf)
}

#[cfg(windows)]
const PACK_BIN: &str = "sweet-zenz-pack.exe";
#[cfg(not(windows))]
const PACK_BIN: &str = "sweet-zenz-pack";

fn pack_tool() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        for candidate in [
            dir.join("tools").join(PACK_BIN),
            dir.join(PACK_BIN),
            dir.join("../MacOS").join(PACK_BIN),
            dir.join("../bin").join(PACK_BIN),
            dir.join("../release").join(PACK_BIN),
        ] {
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    Some(PathBuf::from(PACK_BIN))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DownloadTarget {
    Builtin(ModelKind),
    Custom { label: String, staged: PathBuf },
}

impl DownloadTarget {
    fn display_name(&self) -> &str {
        match self {
            Self::Builtin(kind) => kind.name(),
            Self::Custom { label, .. } => label.as_str(),
        }
    }

    fn builtin(&self) -> Option<ModelKind> {
        match self {
            Self::Builtin(kind) => Some(*kind),
            Self::Custom { .. } => None,
        }
    }
}

fn applied_notice() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "次回のログインから反映されます。"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "アプリを切り替えると反映されます。"
    }
}

/// A running model download or custom model import/conversion.
pub(super) struct Download {
    target: DownloadTarget,
    log: Vec<String>,
    lines: Receiver<DownloadEvent>,
    done: Option<bool>,
}

enum DownloadEvent {
    Output(String),
    Finished(bool),
}

impl Download {
    fn poll(&mut self) -> Option<bool> {
        let mut finished_now = None;
        while let Ok(event) = self.lines.try_recv() {
            match event {
                DownloadEvent::Output(line) => self.log.push(line),
                DownloadEvent::Finished(success) => {
                    self.done = Some(success);
                    finished_now = Some(success);
                }
            }
        }
        finished_now
    }

    fn is_busy(&self) -> bool {
        self.done.is_none()
    }
}

fn human_size(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1_000_000.0)
}

fn format_step(line: Option<&str>) -> String {
    let Some(line) = line else {
        return "準備中…".into();
    };
    if let Some(file) = line.strip_prefix("downloading ") {
        return format!("ダウンロード中: {file}");
    }
    if line.starts_with("installed ") {
        return "仕上げ処理中…".into();
    }
    line.to_string()
}

#[cfg(windows)]
const GET_ZENZ: &str = "get-zenz.ps1";
#[cfg(not(windows))]
const GET_ZENZ: &str = "get-zenz.sh";

/// The model download script: next to this program (Windows install), in
/// the app resources (macOS), `../share/sweet-ime` (Linux), or in scripts/.
fn get_zenz_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    [
        dir.join(GET_ZENZ),
        dir.join("../share/sweet-ime").join(GET_ZENZ),
        dir.join("../Resources").join(GET_ZENZ),
        dir.join("../../scripts").join(GET_ZENZ),
    ]
    .into_iter()
    .find(|p| p.exists())
}

fn start_download(
    target: ModelKind,
    out_path: &Path,
    ctx: egui::Context,
) -> Result<Download, String> {
    use std::io::BufRead;
    let script = get_zenz_script().ok_or_else(|| format!("{GET_ZENZ} が見つかりません"))?;
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("powershell.exe");
        c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .args(["-Model", target.repo()])
            .arg("-Out")
            .arg(out_path)
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = std::process::Command::new("sh");
        c.arg(&script).arg(target.repo()).arg(out_path);
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
        target: DownloadTarget::Builtin(target),
        log: Vec::new(),
        lines: rx,
        done: None,
    })
}

fn start_import(src_path: &Path, data_dir: &Path, ctx: egui::Context) -> Result<Download, String> {
    if !src_path.is_file() {
        return Err("指定されたファイルが見つかりません。".into());
    }
    let ext = src_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let label = src_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "カスタムモデル".into());
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("モデル保存フォルダを作成できませんでした: {e}"))?;
    let staged = data_dir.join("zenz-custom-import.swz");

    match ext.as_str() {
        "swz" => {
            let magic = read_magic(src_path)?;
            if &magic != b"SWZENZ01" {
                return Err("有効な .swz モデルファイルではありません。".into());
            }
            if let (Ok(a), Ok(b)) = (
                std::fs::canonicalize(src_path),
                std::fs::canonicalize(data_dir.join("zenz.swz")),
            ) && a == b
            {
                return Err("すでにこのモデルが使用中です。".into());
            }
            let src = src_path.to_path_buf();
            let dst = staged.clone();
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let _ = tx.send(DownloadEvent::Output("モデルファイルをコピー中…".into()));
                ctx.request_repaint();
                match std::fs::copy(&src, &dst) {
                    Ok(_) => {
                        let _ = tx.send(DownloadEvent::Finished(true));
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&dst);
                        let _ =
                            tx.send(DownloadEvent::Output(format!("コピーに失敗しました: {e}")));
                        let _ = tx.send(DownloadEvent::Finished(false));
                    }
                }
                ctx.request_repaint();
            });
            Ok(Download {
                target: DownloadTarget::Custom { label, staged },
                log: Vec::new(),
                lines: rx,
                done: None,
            })
        }
        "gguf" => {
            use std::io::BufRead;
            let magic = read_magic(src_path)?;
            if &magic[..4] != b"GGUF" {
                return Err("有効な .gguf モデルファイルではありません。".into());
            }
            let tool = pack_tool().ok_or_else(|| format!("{PACK_BIN} が見つかりません"))?;
            let _ = std::fs::remove_file(&staged);
            #[cfg(windows)]
            let mut command = {
                use std::os::windows::process::CommandExt;
                let mut c = std::process::Command::new(&tool);
                c.arg(src_path).arg(&staged).creation_flags(0x0800_0000);
                c
            };
            #[cfg(not(windows))]
            let mut command = {
                let mut c = std::process::Command::new(&tool);
                c.arg(src_path).arg(&staged);
                c
            };
            let mut child = command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| format!("GGUF 変換ツール ({PACK_BIN}) を起動できません: {e}"))?;
            let (tx, rx) = channel();
            let _ = tx.send(DownloadEvent::Output("GGUF モデルを .swz へ変換中…".into()));
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
            let staged_cleanup = staged.clone();
            std::thread::spawn(move || {
                for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = tx.send(DownloadEvent::Output(line));
                    ctx.request_repaint();
                }
                let ok = child.wait().map(|s| s.success()).unwrap_or(false);
                if !ok {
                    let _ = std::fs::remove_file(&staged_cleanup);
                }
                let _ = tx.send(DownloadEvent::Finished(ok));
                ctx.request_repaint();
            });
            Ok(Download {
                target: DownloadTarget::Custom { label, staged },
                log: Vec::new(),
                lines: rx,
                done: None,
            })
        }
        _ => Err(".swz または .gguf ファイルを選択してください。".into()),
    }
}

fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, ink: Color32) {
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).strong().color(ink));
        });
}

impl App {
    pub(super) fn poll_download(&mut self) {
        let finished = self.download.as_mut().and_then(Download::poll);
        if let Some(true) = finished {
            let target = self.download.take().unwrap().target;
            if let Some(dir) = paths::data_dir() {
                match target {
                    DownloadTarget::Builtin(kind) => match activate_model(&dir, kind) {
                        Ok(()) => {
                            self.status = Some((
                                format!("{} を導入しました。{}", kind.name(), applied_notice()),
                                true,
                            ));
                        }
                        Err(e) => self.status = Some((e, false)),
                    },
                    DownloadTarget::Custom { label, staged } => {
                        match activate_staged_custom(&dir, &staged) {
                            Ok(()) => {
                                self.import_model_path.clear();
                                self.status = Some((
                                    format!("{label} をインポートしました。{}", applied_notice()),
                                    true,
                                ));
                            }
                            Err(e) => {
                                let _ = std::fs::remove_file(&staged);
                                self.status = Some((e, false));
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn neural_page(&mut self, ui: &mut egui::Ui) {
        let data_dir = paths::data_dir();
        let inv = data_dir
            .as_deref()
            .map(inspect_models)
            .unwrap_or(ModelInventory {
                xsmall: ModelSlot::NotInstalled,
                small: ModelSlot::NotInstalled,
                custom_active: None,
            });

        let mut changed = false;
        card_group(ui, |ui| {
            setting_row(
                ui,
                "AI 変換を使う",
                "文脈に合わせてニューラル言語モデル (zenz) で変換候補を選び直します。",
                |ui| {
                    changed |= toggle(ui, &mut self.opts.neural).changed();
                },
            );
            if self.opts.neural && !inv.has_active() {
                setting_divider(ui);
                Frame::new()
                    .inner_margin(Margin::symmetric(0, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(
                                "現在モデルが未選択のため、辞書のみで変換しています。下からモデルをダウンロードしてください。",
                            )
                            .small()
                            .color(ui.visuals().warn_fg_color),
                        );
                    });
            }
        });
        if changed {
            self.save();
        }

        section_header(ui, "使用するモデル");

        let busy = self.download.as_ref().is_some_and(Download::is_busy);
        let downloading_kind = self
            .download
            .as_ref()
            .filter(|d| d.is_busy())
            .and_then(|d| d.target.builtin());
        let accent = ui.visuals().selection.bg_fill;
        let subtle_fill = ui.visuals().widgets.inactive.weak_bg_fill;
        let subtle_ink = ui.visuals().text_color();

        let mut action: Option<ModelAction> = None;

        card_group(ui, |ui| {
            for (idx, kind) in ModelKind::ALL.into_iter().enumerate() {
                if idx > 0 {
                    setting_divider(ui);
                }
                let slot = inv.slot(kind);
                let is_downloading = downloading_kind == Some(kind);
                setting_row_custom(
                    ui,
                    ("neural_model", kind.repo()),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(RichText::new(kind.name()).size(15.5).strong());
                            match slot {
                                ModelSlot::Active(_) => {
                                    badge(ui, "使用中", accent, contrasting_ink(accent));
                                }
                                ModelSlot::Downloaded(_) => {
                                    badge(ui, "ダウンロード済み", subtle_fill, subtle_ink);
                                }
                                ModelSlot::NotInstalled => {
                                    ui.label(RichText::new("未導入").small().weak());
                                }
                            }
                            if kind.recommended() {
                                badge(ui, "おすすめ", subtle_fill, subtle_ink);
                            }
                        });
                        ui.label(RichText::new(kind.summary()).small().weak());
                        if let ModelSlot::Active(sz) | ModelSlot::Downloaded(sz) = slot {
                            ui.label(
                                RichText::new(format!("ディスク使用量: {}", human_size(sz)))
                                    .small()
                                    .weak(),
                            );
                        }
                    },
                    |ui| {
                        if is_downloading {
                            ui.label(RichText::new("ダウンロード中…").small().weak());
                            ui.spinner();
                        } else {
                            match slot {
                                ModelSlot::Active(_) => {
                                    if ui.add_enabled(!busy, egui::Button::new("削除")).clicked()
                                    {
                                        action = Some(ModelAction::Delete(kind));
                                    }
                                }
                                ModelSlot::Downloaded(_) => {
                                    if ui.add_enabled(!busy, egui::Button::new("削除")).clicked()
                                    {
                                        action = Some(ModelAction::Delete(kind));
                                    }
                                    let use_btn = sweet_ui::accent_button(ui, "このモデルを使う");
                                    if ui.add_enabled(!busy, use_btn).clicked() {
                                        action = Some(ModelAction::Activate(kind));
                                    }
                                }
                                ModelSlot::NotInstalled => {
                                    if ui
                                        .add_enabled(
                                            !busy,
                                            sweet_ui::accent_button(ui, "ダウンロードして使う"),
                                        )
                                        .clicked()
                                    {
                                        action = Some(ModelAction::Download(kind));
                                    }
                                }
                            }
                        }
                    },
                );
            }

            if let Some(sz) = inv.custom_active {
                setting_divider(ui);
                setting_row_custom(
                    ui,
                    "neural_custom_model",
                    |ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                RichText::new("カスタムモデル (zenz.swz)")
                                    .size(15.5)
                                    .strong(),
                            );
                            badge(ui, "使用中", accent, contrasting_ink(accent));
                        });
                        ui.label(
                            RichText::new(format!("ディスク使用量: {}", human_size(sz)))
                                .small()
                                .weak(),
                        );
                    },
                    |ui| {
                        if ui.add_enabled(!busy, egui::Button::new("削除")).clicked() {
                            action = Some(ModelAction::DeleteCustom);
                        }
                    },
                );
            }
        });

        if let Some(act) = action {
            match (act, data_dir.as_deref()) {
                (ModelAction::Activate(kind), Some(dir)) => match activate_model(dir, kind) {
                    Ok(()) => {
                        self.status = Some((
                            format!("{} に切り替えました。{}", kind.name(), applied_notice()),
                            true,
                        ));
                    }
                    Err(e) => self.status = Some((e, false)),
                },
                (ModelAction::Download(kind), Some(dir)) => {
                    let out = dir.join(kind.stored_filename());
                    match start_download(kind, &out, ui.ctx().clone()) {
                        Ok(d) => self.download = Some(d),
                        Err(e) => self.status = Some((e, false)),
                    }
                }
                (ModelAction::Delete(kind), Some(dir)) => match delete_model(dir, kind) {
                    Ok(()) => {
                        if self
                            .download
                            .as_ref()
                            .is_some_and(|d| d.target.builtin() == Some(kind) && !d.is_busy())
                        {
                            self.download = None;
                        }
                        self.status = Some((format!("{} を削除しました。", kind.name()), true));
                    }
                    Err(e) => self.status = Some((e, false)),
                },
                (ModelAction::DeleteCustom, Some(dir)) => match delete_custom_model(dir) {
                    Ok(()) => {
                        self.status = Some(("カスタムモデルを削除しました。".into(), true));
                    }
                    Err(e) => self.status = Some((e, false)),
                },
                (_, None) => {
                    self.status =
                        Some(("モデル保存フォルダを取得できませんでした。".into(), false));
                }
            }
        }

        // Accept drag-and-drop of `.swz` or `.gguf` files onto the AI page.
        let dropped: Option<PathBuf> = ui.ctx().input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .find(|p| {
                    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        e.eq_ignore_ascii_case("swz") || e.eq_ignore_ascii_case("gguf")
                    })
                })
        });
        if let Some(path) = dropped {
            self.import_model_path = path.to_string_lossy().into_owned();
        }

        section_header(ui, "カスタムモデルのインポート");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(
                    "ローカルの zenz モデルファイル（.swz または GPT-2 形式の .gguf）を読み込んで AI 変換に使います。ファイルをこのウィンドウへドラッグ＆ドロップしても指定できます。",
                )
                .small()
                .weak(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field_w = (ui.available_width() - 180.0).max(160.0);
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.import_model_path)
                        .hint_text("モデルファイルのパス (.swz / .gguf)")
                        .desired_width(field_w),
                );
                let trimmed = self.import_model_path.trim().trim_matches('"').to_string();
                if ui
                    .add_enabled(
                        !busy && !trimmed.is_empty(),
                        sweet_ui::accent_button(ui, "インポートして使う"),
                    )
                    .clicked()
                {
                    match data_dir.as_deref() {
                        Some(dir) => match start_import(Path::new(&trimmed), dir, ui.ctx().clone()) {
                            Ok(d) => self.download = Some(d),
                            Err(e) => self.status = Some((e, false)),
                        },
                        None => {
                            self.status = Some((
                                "モデル保存フォルダを取得できませんでした。".into(),
                                false,
                            ));
                        }
                    }
                }
            });
        });

        if let Some(d) = &self.download {
            let mut dismiss = false;
            card_frame(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                match d.done {
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!(
                                "{} — {}",
                                d.target.display_name(),
                                format_step(d.log.last().map(String::as_str))
                            ));
                        });
                    }
                    Some(false) => {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!("{} の処理に失敗しました:", d.target.display_name()),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("閉じる").clicked() {
                                        dismiss = true;
                                    }
                                },
                            );
                        });
                        for l in d.log.iter().rev().take(4).rev() {
                            ui.label(RichText::new(l).small().monospace());
                        }
                    }
                    Some(true) => {}
                }
            });
            if dismiss {
                self.download = None;
            }
        }

        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "zenz モデル（作者: Miwa Keita、ライセンス: CC BY-SA 4.0）は Sweet IME には同梱されず、Hugging Face からダウンロードして変換します。複数モデルを導入しておくと再ダウンロードなしで切り替えられます。",
            )
            .small()
            .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to("xsmall モデルページ", ModelKind::Xsmall.url());
            ui.label(RichText::new("·").weak());
            ui.hyperlink_to("small モデルページ", ModelKind::Small.url());
            if let Some(dir) = &data_dir {
                ui.label(RichText::new("·").weak());
                if ui.small_button("モデルフォルダを開く").clicked() {
                    let _ = std::fs::create_dir_all(dir);
                    super::open_folder(dir);
                }
            }
        });
    }
}

enum ModelAction {
    Activate(ModelKind),
    Download(ModelKind),
    Delete(ModelKind),
    DeleteCustom,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sweet-settings-neural-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_sized(path: &Path, size: u64) {
        let f = std::fs::File::create(path).unwrap();
        f.set_len(size).unwrap();
    }

    #[test]
    fn classify_identifies_xsmall_small_and_custom() {
        assert_eq!(classify_swz_size(97_800_000), Some(ModelKind::Xsmall));
        assert_eq!(classify_swz_size(380_000_000), Some(ModelKind::Small));
        assert_eq!(classify_swz_size(12_000_000), None);
    }

    #[test]
    fn activate_swaps_and_preserves_previous_model_and_delete_removes_either() {
        let dir = temp_dir("swap");
        write_sized(&dir.join("zenz.swz"), 380_000_000);
        write_sized(&dir.join(ModelKind::Xsmall.stored_filename()), 98_000_000);

        let inv = inspect_models(&dir);
        assert_eq!(inv.small, ModelSlot::Active(380_000_000));
        assert_eq!(inv.xsmall, ModelSlot::Downloaded(98_000_000));

        // Switching to xsmall preserves small as zenz-small.swz.
        activate_model(&dir, ModelKind::Xsmall).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.xsmall, ModelSlot::Active(98_000_000));
        assert_eq!(inv.small, ModelSlot::Downloaded(380_000_000));

        // Deleting inactive small removes only small.
        delete_model(&dir, ModelKind::Small).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.xsmall, ModelSlot::Active(98_000_000));
        assert_eq!(inv.small, ModelSlot::NotInstalled);

        // Deleting active xsmall removes zenz.swz.
        delete_model(&dir, ModelKind::Xsmall).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.xsmall, ModelSlot::NotInstalled);
        assert!(!inv.has_active());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
