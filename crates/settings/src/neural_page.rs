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

/// Records which model `zenz.swz` holds, so a custom model whose size falls
/// in a built-in range is never mistaken for that built-in.
const ACTIVE_MARKER: &str = "zenz-active.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveModel {
    Builtin(ModelKind),
    Custom,
}

fn file_fingerprint(path: &Path) -> Option<(u64, String)> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut buf = [0u8; 64 * 1024];
    let mut read_total = 0usize;
    let mut hasher = Sha256::new();
    while read_total < buf.len() {
        let n = f.read(&mut buf[read_total..]).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[read_total..read_total + n]);
        read_total += n;
    }
    let hex: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Some((len, hex))
}

fn read_marker(data_dir: &Path, active_fp: &(u64, String)) -> Option<ActiveModel> {
    let text = std::fs::read_to_string(data_dir.join(ACTIVE_MARKER)).ok()?;
    let mut parts = text.split_whitespace();
    let kind = match parts.next()? {
        "xsmall" => ActiveModel::Builtin(ModelKind::Xsmall),
        "small" => ActiveModel::Builtin(ModelKind::Small),
        "custom" => ActiveModel::Custom,
        _ => return None,
    };
    let size: u64 = parts.next()?.parse().ok()?;
    let hash = parts.next()?;
    if parts.next().is_some() || size != active_fp.0 || hash != active_fp.1 {
        return None;
    }
    Some(kind)
}

fn format_marker(model: ActiveModel, fp: &(u64, String)) -> String {
    let label = match model {
        ActiveModel::Builtin(ModelKind::Xsmall) => "xsmall",
        ActiveModel::Builtin(ModelKind::Small) => "small",
        ActiveModel::Custom => "custom",
    };
    format!("{label} {} {}", fp.0, fp.1)
}

fn write_marker_raw(data_dir: &Path, content: Option<&str>) -> std::io::Result<()> {
    let path = data_dir.join(ACTIVE_MARKER);
    match content {
        None => match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
        Some(text) => std::fs::write(&path, text),
    }
}

/// Returns the identity and size of `zenz.swz`, using the marker file only
/// when its recorded size and prefix hash match the current file, and falling
/// back to the file size otherwise (older installs or external replacements).
fn active_model(data_dir: &Path) -> Option<(ActiveModel, u64)> {
    let active = data_dir.join("zenz.swz");
    let fp = file_fingerprint(&active)?;
    let id = read_marker(data_dir, &fp).unwrap_or_else(|| {
        classify_swz_size(fp.0).map_or(ActiveModel::Custom, ActiveModel::Builtin)
    });
    Some((id, fp.0))
}

fn inspect_models(data_dir: &Path) -> ModelInventory {
    let mut inv = ModelInventory {
        xsmall: ModelSlot::NotInstalled,
        small: ModelSlot::NotInstalled,
        custom_active: None,
    };
    match active_model(data_dir) {
        Some((ActiveModel::Builtin(ModelKind::Xsmall), sz)) => inv.xsmall = ModelSlot::Active(sz),
        Some((ActiveModel::Builtin(ModelKind::Small), sz)) => inv.small = ModelSlot::Active(sz),
        Some((ActiveModel::Custom, sz)) => inv.custom_active = Some(sz),
        None => {}
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

/// Moves the current `zenz.swz` aside before activating another model: a
/// built-in model other than `keep` goes to its named file (kept), anything
/// else goes to a temporary backup (removed after success).
fn move_active_aside(
    data_dir: &Path,
    keep: Option<ModelKind>,
) -> Result<Option<(PathBuf, bool)>, String> {
    let active = data_dir.join("zenz.swz");
    let Some((id, _)) = active_model(data_dir) else {
        return Ok(None);
    };
    let fail = |e: std::io::Error| format!("既存モデルの退避に失敗しました: {e}");
    match id {
        ActiveModel::Builtin(old) if Some(old) != keep => {
            let archive = data_dir.join(old.stored_filename());
            safe_remove(&archive).map_err(fail)?;
            std::fs::rename(&active, &archive).map_err(fail)?;
            Ok(Some((archive, false)))
        }
        _ => {
            let temp = active.with_extension("swz.bak");
            let _ = std::fs::remove_file(&temp);
            std::fs::rename(&active, &temp).map_err(fail)?;
            Ok(Some((temp, true)))
        }
    }
}

/// Puts the previous model back after a failed activation and describes
/// where it remains if that also fails.
fn restore_backup(backup: Option<(PathBuf, bool)>, active: &Path, error: String) -> String {
    if let Some((path, _)) = backup
        && let Err(e) = std::fs::rename(&path, active)
    {
        return format!(
            "{error}（以前のモデルを元に戻せませんでした: {e}。以前のモデルは {} に残っています）",
            path.display()
        );
    }
    error
}

/// Renames `source` to `zenz.swz` with the previous model moved aside,
/// recording `id` bound to `source`'s fingerprint before the final rename so
/// `zenz.swz` and `zenz-active.txt` stay consistent on failure or crash.
fn swap_in(
    data_dir: &Path,
    source: &Path,
    keep: Option<ModelKind>,
    id: ActiveModel,
    apply_error: &str,
) -> Result<(), String> {
    let active = data_dir.join("zenz.swz");
    let new_fp =
        file_fingerprint(source).ok_or_else(|| format!("{apply_error}: ファイルを読めません"))?;
    let old_marker = std::fs::read_to_string(data_dir.join(ACTIVE_MARKER)).ok();
    let backup = move_active_aside(data_dir, keep)?;
    let new_marker = format_marker(id, &new_fp);
    if let Err(e) = write_marker_raw(data_dir, Some(&new_marker)) {
        let _ = write_marker_raw(data_dir, old_marker.as_deref());
        return Err(restore_backup(
            backup,
            &active,
            format!("モデル状態の保存に失敗しました: {e}"),
        ));
    }
    if let Err(e) = std::fs::rename(source, &active) {
        let _ = write_marker_raw(data_dir, old_marker.as_deref());
        return Err(restore_backup(
            backup,
            &active,
            format!("{apply_error}: {e}"),
        ));
    }
    if let Some((temp, true)) = &backup {
        let _ = std::fs::remove_file(temp);
    }
    Ok(())
}

/// Switches `zenz.swz` to `target`, preserving the previously active built-in
/// model in its named file so switching back does not require re-downloading.
/// Keeps a reversible backup of the previous active file until the final
/// rename succeeds.
fn activate_model(data_dir: &Path, target: ModelKind) -> Result<(), String> {
    let source = data_dir.join(target.stored_filename());
    if !source.exists() {
        if matches!(active_model(data_dir), Some((ActiveModel::Builtin(k), _)) if k == target) {
            return Ok(());
        }
        return Err(format!("{} のファイルが見つかりません。", target.name()));
    }
    swap_in(
        data_dir,
        &source,
        Some(target),
        ActiveModel::Builtin(target),
        "モデルの切り替えに失敗しました",
    )
}

/// Deletes `target` whether it is currently active or stored as inactive.
fn delete_model(data_dir: &Path, target: ModelKind) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("削除できませんでした: {e}");
    if matches!(active_model(data_dir), Some((ActiveModel::Builtin(k), _)) if k == target) {
        safe_remove(&data_dir.join("zenz.swz")).map_err(fail)?;
        write_marker_raw(data_dir, None).map_err(fail)?;
    }
    let stored = data_dir.join(target.stored_filename());
    if stored.exists() {
        safe_remove(&stored).map_err(fail)?;
    }
    Ok(())
}

/// Deletes the active custom model; built-in models are left untouched.
fn delete_custom_model(data_dir: &Path) -> Result<(), String> {
    let fail = |e: std::io::Error| format!("削除できませんでした: {e}");
    if matches!(active_model(data_dir), Some((ActiveModel::Custom, _))) {
        safe_remove(&data_dir.join("zenz.swz")).map_err(fail)?;
        write_marker_raw(data_dir, None).map_err(fail)?;
    }
    Ok(())
}

/// Activates a staged `.swz` file (from custom `.swz` or `.gguf` import) as
/// `zenz.swz`, preserving any previously active built-in model (`xsmall`/`small`)
/// and restoring the previous `zenz.swz` if the final rename fails.
fn activate_staged_custom(data_dir: &Path, staged: &Path) -> Result<(), String> {
    if !staged.exists() {
        return Err("変換後のモデルファイルが見つかりません。".into());
    }
    swap_in(
        data_dir,
        staged,
        None,
        ActiveModel::Custom,
        "モデルの適用に失敗しました",
    )
}

/// Layer tensor names in a packed model with their element counts as
/// multiples of `n_embd` squared (`.0`) or `n_embd` (`.1`).
const SWZ_LAYER_TENSORS: [(&str, usize, usize); 12] = [
    ("ln_1.weight", 0, 1),
    ("ln_1.bias", 0, 1),
    ("attn.c_attn.weight", 3, 0),
    ("attn.c_attn.bias", 0, 3),
    ("attn.c_proj.weight", 1, 0),
    ("attn.c_proj.bias", 0, 1),
    ("ln_2.weight", 0, 1),
    ("ln_2.bias", 0, 1),
    ("mlp.c_fc.weight", 4, 0),
    ("mlp.c_fc.bias", 0, 4),
    ("mlp.c_proj.weight", 4, 0),
    ("mlp.c_proj.bias", 0, 1),
];

/// Checks that a packed `.swz` file is complete and internally consistent
/// (header, tokenizer, and every tensor range and size) before it is used.
fn validate_swz(path: &Path) -> Result<(), String> {
    use std::io::{Read, Seek, SeekFrom};
    let bad = |why: &str| format!("有効な .swz モデルファイルではありません（{why}）。");
    let mut f =
        std::fs::File::open(path).map_err(|e| format!("ファイルを開けませんでした: {e}"))?;
    let len = f
        .metadata()
        .map_err(|e| format!("ファイルを読み込めませんでした: {e}"))?
        .len();
    let mut head = [0u8; 16];
    f.read_exact(&mut head)
        .map_err(|_| bad("ファイルが短すぎます"))?;
    if &head[..8] != b"SWZENZ01" {
        return Err(bad("形式が違います"));
    }
    let hl = u64::from_le_bytes(head[8..16].try_into().unwrap());
    if hl > len.saturating_sub(16) {
        return Err(bad("ヘッダーが途中で切れています"));
    }
    let mut hdr_bytes = vec![0u8; hl as usize];
    f.read_exact(&mut hdr_bytes)
        .map_err(|_| bad("ヘッダーが途中で切れています"))?;
    let hdr: serde_json::Value =
        serde_json::from_slice(&hdr_bytes).map_err(|_| bad("ヘッダーが壊れています"))?;
    let off = (16 + hl).div_ceil(64) * 64;
    f.seek(SeekFrom::Start(off))
        .map_err(|_| bad("重みが途中で切れています"))?;
    let mut nb = [0u8; 8];
    f.read_exact(&mut nb)
        .map_err(|_| bad("重みが途中で切れています"))?;
    let n = u64::from_le_bytes(nb);
    let data_off = (off + 8).div_ceil(64) * 64;
    let need = n
        .checked_mul(4)
        .and_then(|b| b.checked_add(data_off))
        .ok_or_else(|| bad("重みのサイズが不正です"))?;
    if need > len {
        return Err(bad("重みが途中で切れています"));
    }
    let g = |k: &str| hdr[k].as_u64().filter(|&v| v > 0);
    let (Some(e), Some(h), Some(l), Some(p), Some(v)) = (
        g("n_embd"),
        g("n_head"),
        g("n_layer"),
        g("n_positions"),
        g("vocab_size"),
    ) else {
        return Err(bad("モデル設定が不足しています"));
    };
    if e % h != 0 {
        return Err(bad("モデル設定が不正です"));
    }
    let check = |name: &str, elems: Option<u64>| -> Result<(), String> {
        let a = hdr["tensors"][name]
            .as_array()
            .ok_or_else(|| bad(&format!("{name} がありません")))?;
        let (Some(s), Some(t)) = (
            a.first().and_then(|x| x.as_u64()),
            a.get(1).and_then(|x| x.as_u64()),
        ) else {
            return Err(bad(&format!("{name} の範囲が不正です")));
        };
        if s > t || t > n || Some(t - s) != elems {
            return Err(bad(&format!("{name} の範囲が不正です")));
        }
        Ok(())
    };
    check("wte", v.checked_mul(e))?;
    check("wte_t", v.checked_mul(e))?;
    check("wpe", p.checked_mul(e))?;
    check("lnf_w", Some(e))?;
    check("lnf_b", Some(e))?;
    for i in 0..l {
        for (t, sq, lin) in SWZ_LAYER_TENSORS {
            let elems = e
                .checked_mul(e)
                .and_then(|ee| ee.checked_mul(sq as u64))
                .and_then(|x| x.checked_add(e * lin as u64));
            check(&format!("h.{i}.{t}"), elems)?;
        }
    }
    let vocab_ok = hdr["vocab"]
        .as_str()
        .and_then(|s| serde_json::from_str::<std::collections::HashMap<String, u32>>(s).ok())
        .is_some_and(|m| !m.is_empty() && m.values().all(|&id| u64::from(id) < v));
    if !vocab_ok || hdr["merges"].as_str().is_none() {
        return Err(bad("トークナイザーが壊れています"));
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
    static IMPORT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = IMPORT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staged = data_dir.join(format!(
        "zenz-import-{}-{nanos}-{seq}.swz",
        std::process::id()
    ));

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
                    Ok(_) => match validate_swz(&dst) {
                        Ok(()) => {
                            let _ = tx.send(DownloadEvent::Finished(true));
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&dst);
                            let _ = tx.send(DownloadEvent::Output(e));
                            let _ = tx.send(DownloadEvent::Finished(false));
                        }
                    },
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
                let mut ok = child.wait().map(|s| s.success()).unwrap_or(false);
                if ok && let Err(e) = validate_swz(&staged_cleanup) {
                    let _ = tx.send(DownloadEvent::Output(e));
                    ok = false;
                }
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
                                self.import_model_source = None;
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
        // Ignore drops while an import runs so its completion cannot clear a
        // newer selection. Keep the native path for non-UTF-8 file names.
        if !busy && let Some(path) = dropped {
            self.import_model_path = path.to_string_lossy().into_owned();
            self.import_model_source = Some(path);
        }

        section_header(ui, "カスタムモデルのインポート");
        card_frame(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(
                    "ローカルの zenz モデルファイル（.swz、または GPT-2 形式でリトルエンディアンの .gguf）を読み込んで AI 変換に使います。ファイルをこのウィンドウへドラッグ＆ドロップしても指定できます。",
                )
                .small()
                .weak(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field_w = (ui.available_width() - 180.0).max(160.0);
                let edit = ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.import_model_path)
                        .hint_text("モデルファイルのパス (.swz / .gguf)")
                        .desired_width(field_w),
                );
                if edit.changed() {
                    self.import_model_source = None;
                }
                let trimmed = self.import_model_path.trim().trim_matches('"').to_string();
                if ui
                    .add_enabled(
                        !busy && !trimmed.is_empty(),
                        sweet_ui::accent_button(ui, "インポートして使う"),
                    )
                    .clicked()
                {
                    match data_dir.as_deref() {
                        Some(dir) => match start_import(
                            &self
                                .import_model_source
                                .clone()
                                .unwrap_or_else(|| PathBuf::from(&trimmed)),
                            dir,
                            ui.ctx().clone(),
                        ) {
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

    #[test]
    fn custom_import_in_builtin_size_range_keeps_builtin_separate() {
        let dir = temp_dir("custom-identity");
        write_sized(&dir.join("zenz.swz"), 98_000_000);
        let staged = dir.join("zenz-custom-import.swz");
        write_sized(&staged, 99_000_000);

        activate_staged_custom(&dir, &staged).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.custom_active, Some(99_000_000));
        assert_eq!(inv.xsmall, ModelSlot::Downloaded(98_000_000));

        // Deleting xsmall leaves the custom model alone.
        write_sized(&staged, 99_500_000);
        delete_model(&dir, ModelKind::Xsmall).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.custom_active, Some(99_000_000));
        assert_eq!(inv.xsmall, ModelSlot::NotInstalled);

        // A second custom import replaces the first without touching built-ins.
        write_sized(&dir.join(ModelKind::Xsmall.stored_filename()), 98_000_000);
        activate_staged_custom(&dir, &staged).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.custom_active, Some(99_500_000));
        assert_eq!(inv.xsmall, ModelSlot::Downloaded(98_000_000));

        // Switching back to the built-in works and clears the custom model.
        activate_model(&dir, ModelKind::Xsmall).unwrap();
        let inv = inspect_models(&dir);
        assert_eq!(inv.xsmall, ModelSlot::Active(98_000_000));
        assert_eq!(inv.custom_active, None);
        delete_custom_model(&dir).unwrap();
        assert_eq!(inspect_models(&dir).xsmall, ModelSlot::Active(98_000_000));

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn tiny_swz_with_vocab(path: &Path, vocab: &str, truncate: bool) {
        let (e, l, p, v) = (4u64, 1u64, 2u64, 2u64);
        let mut tensors = serde_json::Map::new();
        let mut at = 0u64;
        let mut add = |name: String, n: u64| {
            tensors.insert(name, serde_json::json!([at, at + n]));
            at += n;
        };
        add("wte".into(), v * e);
        add("wte_t".into(), v * e);
        add("wpe".into(), p * e);
        add("lnf_w".into(), e);
        add("lnf_b".into(), e);
        for i in 0..l {
            for (t, sq, lin) in SWZ_LAYER_TENSORS {
                add(format!("h.{i}.{t}"), e * e * sq as u64 + e * lin as u64);
            }
        }
        let n = at;
        let hdr = serde_json::json!({
            "n_embd": e, "n_head": 2, "n_layer": l, "n_positions": p, "vocab_size": v,
            "eps": 1e-5, "tensors": tensors, "vocab": vocab, "merges": "",
        });
        let hb = serde_json::to_vec(&hdr).unwrap();
        let mut buf = b"SWZENZ01".to_vec();
        buf.extend_from_slice(&(hb.len() as u64).to_le_bytes());
        buf.extend_from_slice(&hb);
        buf.resize(buf.len().div_ceil(64) * 64, 0);
        buf.extend_from_slice(&n.to_le_bytes());
        buf.resize(buf.len().div_ceil(64) * 64, 0);
        buf.resize(buf.len() + n as usize * 4, 0);
        if truncate {
            buf.truncate(buf.len() - 4);
        }
        std::fs::write(path, buf).unwrap();
    }

    #[test]
    fn validate_swz_accepts_complete_and_rejects_bad_files() {
        let dir = temp_dir("validate");
        let ok = dir.join("ok.swz");
        let short = dir.join("short.swz");
        let oob_vocab = dir.join("oob.swz");
        tiny_swz_with_vocab(&ok, "{\"a\":0,\"b\":1}", false);
        tiny_swz_with_vocab(&short, "{\"a\":0,\"b\":1}", true);
        tiny_swz_with_vocab(&oob_vocab, "{\"a\":2,\"b\":1}", false);
        assert!(validate_swz(&ok).is_ok());
        assert!(validate_swz(&short).is_err());
        assert!(validate_swz(&oob_vocab).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_marker_ignored_after_external_replacement_and_marker_failure_rolls_back() {
        let dir = temp_dir("marker-safety");
        write_sized(&dir.join("zenz.swz"), 98_000_000);
        let staged = dir.join("staged.swz");
        write_sized(&staged, 99_000_000);
        activate_staged_custom(&dir, &staged).unwrap();
        assert_eq!(inspect_models(&dir).custom_active, Some(99_000_000));

        // Simulate external replacement (e.g. get-zenz.sh writing a built-in small).
        write_sized(&dir.join("zenz.swz"), 380_000_000);
        let inv = inspect_models(&dir);
        assert_eq!(inv.small, ModelSlot::Active(380_000_000));
        assert_eq!(inv.custom_active, None);

        // Simulate unwritable marker by replacing it with a directory; swap must roll back.
        let marker = dir.join(ACTIVE_MARKER);
        let _ = std::fs::remove_file(&marker);
        std::fs::create_dir(&marker).unwrap();
        assert!(activate_model(&dir, ModelKind::Xsmall).is_err());
        assert_eq!(inspect_models(&dir).small, ModelSlot::Active(380_000_000));
        assert_eq!(
            inspect_models(&dir).xsmall,
            ModelSlot::Downloaded(98_000_000)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
