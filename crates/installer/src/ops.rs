//! Install and uninstall steps (run elevated, on a worker thread).

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegDeleteTreeW, RegOpenKeyExW, RegSetValueExW,
};
use windows::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface, PCWSTR};

use crate::payload::Entry;

pub const DISPLAY_NAME: &str = "Sweet IME";
const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\SweetIME";
const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const UPDATE_CHECK_VALUE: &str = "SweetIMEUpdateCheck";
const TIP: &str = "0411:{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A01}{7E1D3C2A-5B8F-4C1E-9A3D-2F6B8C4E1A02}";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub type Result<T> = std::result::Result<T, String>;

pub fn target_dir() -> PathBuf {
    let pf = std::env::var_os("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
    PathBuf::from(pf).join("SweetIME")
}

pub fn has_shortcut() -> bool {
    shortcut_path().exists()
}

fn shortcut_path() -> PathBuf {
    let pd = std::env::var_os("ProgramData").unwrap_or_else(|| r"C:\ProgramData".into());
    PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs").join("Sweet IME の設定.lnk")
}

pub fn is_installed() -> bool {
    let mut key = HKEY::default();
    let ok = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(UNINSTALL_KEY), None, KEY_READ, &mut key) }
        .is_ok();
    if ok {
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
    ok
}

fn timestamp() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// Returns true when `path` already exists with the exact bytes of `data`.
fn file_matches(path: &Path, data: &[u8]) -> bool {
    use std::io::Read;
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if meta.len() != data.len() as u64 {
        return false;
    }
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 64 * 1024];
    let mut offset = 0;
    while offset < data.len() {
        let Ok(n) = f.read(&mut buf) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        if offset + n > data.len() || buf[..n] != data[offset..offset + n] {
            return false;
        }
        offset += n;
    }
    true
}

/// Writes `data` to `dst`. Skips rewriting when `dst` already has identical
/// bytes (saving disk space for unchanged large files like `system.dict`).
/// Always moves any existing file aside before writing so a failed write (such
/// as disk full) restores the previous file instead of leaving a 0-byte file.
fn replace_file(dst: &Path, data: &[u8]) -> Result<()> {
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    if file_matches(dst, data) {
        return Ok(());
    }
    let mut backup = None;
    if dst.exists() {
        let old = dst.with_extension(format!("{}.old", timestamp()));
        std::fs::rename(dst, &old).map_err(|e| format!("{}: {e}", dst.display()))?;
        backup = Some(old);
    }
    if let Err(e) = std::fs::write(dst, data) {
        let _ = std::fs::remove_file(dst);
        if let Some(old) = backup {
            let _ = std::fs::rename(&old, dst);
        }
        return Err(format!("{}: {e}", dst.display()));
    }
    if let Some(old) = backup {
        delete_later(&old);
    }
    Ok(())
}

/// Deletes now, or at the next restart if the file is in use.
fn delete_later(p: &Path) {
    let r = if p.is_dir() { std::fs::remove_dir(p) } else { std::fs::remove_file(p) };
    if r.is_err() {
        unsafe {
            let _ = MoveFileExW(&HSTRING::from(p.as_os_str()), PCWSTR::null(), MOVEFILE_DELAY_UNTIL_REBOOT);
        }
    }
}

pub fn copy_files(files: &[Entry], target: &Path) -> Result<()> {
    // Reclaim leftovers from earlier updates first to maximize free disk space.
    remove_old_files(target);
    for arch in ["x64", "x86"] {
        remove_old_cores(&target.join(arch));
    }
    // core.txt last: running applications switch to the core it names, so
    // that core must already be in place.
    let is_core_txt = |f: &&Entry| Path::new(&f.name).file_name().is_some_and(|n| n == "core.txt");
    for f in files.iter().filter(|f| !is_core_txt(f)).chain(files.iter().filter(is_core_txt)) {
        replace_file(&target.join(&f.name), &f.data)?;
    }
    // Leftovers from earlier updates that are no longer in use.
    remove_old_files(target);
    for arch in ["x64", "x86"] {
        remove_old_cores(&target.join(arch));
    }
    Ok(())
}

/// Deletes the TSF cores that `core.txt` no longer names. Applications that
/// have not switched to the new core yet keep theirs loaded, so those go at
/// the next restart.
fn remove_old_cores(dir: &Path) {
    let Ok(current) = std::fs::read_to_string(dir.join("core.txt")) else { return };
    let current = current.trim();
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("sweet_tsf_core-") && lower.ends_with(".dll") && !name.eq_ignore_ascii_case(current) {
            delete_later(&e.path());
        }
    }
}

fn remove_old_files(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            remove_old_files(&p);
        } else if p.extension().is_some_and(|x| x == "old") {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Copies the plain installer (no payload) as the uninstaller.
pub fn write_uninstaller(target: &Path) -> Result<()> {
    let exe = crate::payload::plain_exe().map_err(|e| e.to_string())?;
    replace_file(&target.join("uninstall.exe"), &exe)
}

fn system_dir(name: &str) -> PathBuf {
    let win = std::env::var_os("windir").unwrap_or_else(|| r"C:\Windows".into());
    PathBuf::from(win).join(name)
}

/// Registers (or with `unregister` removes) the DLL for one architecture.
pub fn regsvr32(x86: bool, dll: &Path, unregister: bool) -> Result<()> {
    let exe = system_dir(if x86 { r"SysWOW64\regsvr32.exe" } else { r"System32\regsvr32.exe" });
    let mut cmd = Command::new(exe);
    cmd.arg("/s");
    if unregister {
        cmd.arg("/u");
    }
    let status = cmd.arg(dll).status().map_err(|e| format!("regsvr32: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("regsvr32 failed ({status})")) }
}

fn powershell(script: &str) -> Result<String> {
    let out = Command::new(system_dir(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn add_to_language_list() -> Result<()> {
    powershell(&format!(
        "$tip = '{TIP}'; $list = Get-WinUserLanguageList; \
         $ja = $list | Where-Object LanguageTag -eq 'ja'; \
         if (-not $ja) {{ $list.Add('ja'); $ja = $list | Where-Object LanguageTag -eq 'ja' }}; \
         if (-not ($ja.InputMethodTips -contains $tip)) {{ $ja.InputMethodTips.Add($tip); Set-WinUserLanguageList $list -Force }}"
    ))
    .map(|_| ())
}

pub fn remove_from_language_list() -> Result<()> {
    powershell(&format!(
        "$tip = '{TIP}'; $list = Get-WinUserLanguageList; $changed = $false; \
         foreach ($l in $list) {{ if ($l.InputMethodTips -contains $tip) {{ [void]$l.InputMethodTips.Remove($tip); $changed = $true }} }}; \
         if ($changed) {{ Set-WinUserLanguageList $list -Force }}"
    ))
    .map(|_| ())
}

pub fn create_shortcut(target: &Path) -> Result<()> {
    save_shortcut(&shortcut_path(), &target.join("sweet-settings.exe"), target)
}

/// Saves a .lnk through IShellLinkW (Unicode paths, unlike WScript.Shell).
fn save_shortcut(lnk: &Path, exe: &Path, target: &Path) -> Result<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW =
            CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        link.SetPath(&HSTRING::from(exe.as_os_str())).map_err(|e| e.to_string())?;
        link.SetWorkingDirectory(&HSTRING::from(target.as_os_str())).map_err(|e| e.to_string())?;
        link.SetDescription(&HSTRING::from("Sweet IME の設定")).map_err(|e| e.to_string())?;
        let file: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        file.Save(&HSTRING::from(lnk.as_os_str()), true).map_err(|e| format!("{}: {e}", lnk.display()))
    }
}

pub fn remove_shortcut() {
    let _ = std::fs::remove_file(shortcut_path());
}

fn set_value(key: HKEY, name: &str, value: &Value) -> Result<()> {
    let r = unsafe {
        match value {
            Value::Str(s) => {
                let wide: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
                let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(bytes))
            }
            Value::Dword(d) => RegSetValueExW(key, &HSTRING::from(name), None, REG_DWORD, Some(&d.to_le_bytes())),
        }
    };
    r.ok().map_err(|e| format!("registry {name}: {e}"))
}

enum Value {
    Str(String),
    Dword(u32),
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).map_or(0, |rd| {
        rd.flatten()
            .map(|e| {
                let p = e.path();
                if p.is_dir() { dir_size(&p) } else { e.metadata().map_or(0, |m| m.len()) }
            })
            .sum()
    })
}

/// "Installed apps" entry with an uninstall command.
pub fn register_uninstall(target: &Path) -> Result<()> {
    let t = target.display().to_string();
    let values = [
        ("DisplayName", Value::Str(DISPLAY_NAME.into())),
        ("DisplayVersion", Value::Str(env!("CARGO_PKG_VERSION").into())),
        ("Publisher", Value::Str("Sweet IME contributors".into())),
        ("InstallLocation", Value::Str(t.clone())),
        ("DisplayIcon", Value::Str(format!("{t}\\sweet-settings.exe"))),
        ("UninstallString", Value::Str(format!("\"{t}\\uninstall.exe\" --uninstall"))),
        ("NoModify", Value::Dword(1)),
        ("NoRepair", Value::Dword(1)),
        ("EstimatedSize", Value::Dword((dir_size(target) / 1024) as u32)),
    ];
    write_values(UNINSTALL_KEY, &values)?;
    // Look for updates at sign-in. The settings app stays quiet unless there
    // is one, and honors the setting that turns this off.
    let check = [(UPDATE_CHECK_VALUE, Value::Str(format!("\"{t}\\sweet-settings.exe\" --check-update")))];
    write_values(RUN_KEY, &check)
}

/// Creates `HKLM\<subkey>` if needed and sets `values` in it.
fn write_values(subkey: &str, values: &[(&str, Value)]) -> Result<()> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(subkey),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|e| format!("registry: {e}"))?;
    }
    let r = values.iter().try_for_each(|(n, v)| set_value(key, n, v));
    unsafe {
        let _ = RegCloseKey(key);
    }
    r
}

pub fn unregister_uninstall() {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(UNINSTALL_KEY));
        let _ = RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(RUN_KEY), &HSTRING::from(UPDATE_CHECK_VALUE));
    }
}

/// Runs get-zenz.ps1 from the install directory, reporting output lines.
pub fn download_model(target: &Path, model: &str, mut line: impl FnMut(String)) -> Result<()> {
    use std::io::BufRead;
    let script = target.join("get-zenz.ps1");
    let mut child = Command::new(system_dir(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .args(["-Model", model])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("powershell: {e}"))?;
    let err = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        std::io::BufReader::new(err).lines().map_while(std::result::Result::ok).collect::<Vec<_>>().join("\n")
    });
    for l in std::io::BufReader::new(child.stdout.take().unwrap()).lines().map_while(std::result::Result::ok) {
        line(l);
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let stderr = err_thread.join().unwrap_or_default();
    if status.success() { Ok(()) } else { Err(if stderr.is_empty() { status.to_string() } else { stderr }) }
}

/// Removes the install directory; files in use go at the next restart.
pub fn remove_files(target: &Path) -> bool {
    let mut all_gone = true;
    fn walk(dir: &Path, all_gone: &mut bool) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, all_gone);
            } else if std::fs::remove_file(&p).is_err() {
                *all_gone = false;
                delete_later(&p);
            }
        }
        if std::fs::remove_dir(dir).is_err() {
            *all_gone = false;
            delete_later(dir);
        }
    }
    walk(target, &mut all_gone);
    all_gone
}

/// Per-user settings, learning history and the downloaded model.
pub fn remove_user_data() {
    for var in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(d) = std::env::var_os(var) {
            let p = PathBuf::from(d).join("SweetIME");
            if p.exists() && std::fs::remove_dir_all(&p).is_err() {
                remove_files(&p);
            }
        }
    }
}

/// Opens the settings app without elevation (through the shell).
pub fn open_settings(target: &Path) {
    let _ = Command::new(system_dir("explorer.exe")).arg(target.join("sweet-settings.exe")).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sweet-setup-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn copy_replace_and_remove() {
        let target = temp("copy");
        let files = vec![
            Entry { name: "system.dict".into(), data: vec![1; 1000] },
            Entry { name: r"x64\sweet_tsf.dll".into(), data: vec![2; 10] },
        ];
        copy_files(&files, &target).unwrap();
        assert_eq!(std::fs::read(target.join(r"x64\sweet_tsf.dll")).unwrap(), vec![2; 10]);
        // Overwrite while the file is open (as a loaded DLL would be).
        let held = std::fs::File::open(target.join("system.dict")).unwrap();
        let files2 = vec![Entry { name: "system.dict".into(), data: vec![3; 5] }];
        copy_files(&files2, &target).unwrap();
        assert_eq!(std::fs::read(target.join("system.dict")).unwrap(), vec![3; 5]);
        drop(held);
        write_uninstaller(&target).unwrap();
        // An update names a new core; the previous one is removed.
        let core = |v: &str| Entry { name: format!(r"x64\sweet_tsf_core-{v}.dll"), data: vec![4; 8] };
        let txt = |v: &str| Entry { name: r"x64\core.txt".into(), data: format!("sweet_tsf_core-{v}.dll").into_bytes() };
        copy_files(&[core("0.1.0"), txt("0.1.0")], &target).unwrap();
        copy_files(&[core("0.2.0"), txt("0.2.0")], &target).unwrap();
        assert!(!target.join(r"x64\sweet_tsf_core-0.1.0.dll").exists());
        assert!(target.join(r"x64\sweet_tsf_core-0.2.0.dll").exists());
        assert!(target.join("uninstall.exe").exists());
        assert!(remove_files(&target) || !target.join("system.dict").exists());
        assert!(!target.join("system.dict").exists());
    }

    #[test]
    fn shortcut_with_japanese_name() {
        let dir = temp("lnk");
        std::fs::create_dir_all(&dir).unwrap();
        let lnk = dir.join("Sweet IME の設定.lnk");
        save_shortcut(&lnk, &dir.join("sweet-settings.exe"), &dir).unwrap();
        assert!(lnk.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
