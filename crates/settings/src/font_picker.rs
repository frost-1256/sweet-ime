use sweet_ui::card;
use sweet_ui::eframe::egui;

pub struct FontPicker {
    family: String,
    families: Vec<String>,
    filter: String,
}

impl FontPicker {
    pub fn new() -> Self {
        #[cfg(windows)]
        let family = sweet_engine::paths::load_candidate_font();
        #[cfg(not(windows))]
        let family = crate::fcitx_theme::candidate_font();
        let mut families = installed_fonts();
        families.sort_by_cached_key(|name| name.to_lowercase());
        families.dedup();
        Self {
            family,
            families,
            filter: String::new(),
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, status: &mut Option<(String, bool)>) {
        #[cfg(windows)]
        let note = "インストール済みのフォントを選びます。アプリを切り替えると候補ウィンドウに反映されます。";
        #[cfg(not(windows))]
        let note = "fcitx5 の Classic UI の候補に適用されます。Classic UI を使う他の入力メソッドにも反映されます。";
        card(ui, "候補のフォント", note, |ui| {
            let current = if self.family.is_empty() {
                "標準"
            } else {
                &self.family
            };
            let mut selected = None;
            egui::ComboBox::from_id_salt("candidate-font")
                .selected_text(current)
                .height(300.0)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("フォント名で絞り込み"),
                    );
                    if ui
                        .selectable_label(self.family.is_empty(), "標準")
                        .clicked()
                    {
                        selected = Some(String::new());
                        ui.close();
                    }
                    let filter = self.filter.trim().to_lowercase();
                    for family in &self.families {
                        if family.to_lowercase().contains(&filter)
                            && ui
                                .selectable_label(&self.family == family, family)
                                .clicked()
                        {
                            selected = Some(family.clone());
                            ui.close();
                        }
                    }
                });
            if self.families.is_empty() {
                ui.label("インストール済みフォントを取得できませんでした。");
            }
            if let Some(family) = selected.filter(|family| family != &self.family) {
                *status = Some(match save_font(&family) {
                    Ok(()) => {
                        self.family = family;
                        self.filter.clear();
                        ("候補のフォントを保存しました。".into(), true)
                    }
                    Err(error) => (error, false),
                });
            }
        });
    }
}

#[cfg(windows)]
fn save_font(name: &str) -> Result<(), String> {
    sweet_engine::paths::save_candidate_font(name)
        .map_err(|e| format!("候補のフォントを保存できませんでした: {e}"))
}

#[cfg(not(windows))]
fn save_font(name: &str) -> Result<(), String> {
    crate::fcitx_theme::apply_candidate_font(name)
}

#[cfg(windows)]
fn installed_fonts() -> Vec<String> {
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
    };
    use windows::core::w;

    unsafe {
        let Ok(dwrite): windows::core::Result<IDWriteFactory> =
            DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
        else {
            return Vec::new();
        };
        let mut collection = None;
        if dwrite
            .GetSystemFontCollection(&mut collection, true)
            .is_err()
        {
            return Vec::new();
        }
        let Some(collection) = collection else {
            return Vec::new();
        };
        (0..collection.GetFontFamilyCount())
            .filter_map(|i| {
                let names = collection.GetFontFamily(i).ok()?.GetFamilyNames().ok()?;
                let mut index = 0;
                for locale in [w!("ja-jp"), w!("en-us")] {
                    let mut found = false.into();
                    if names.FindLocaleName(locale, &mut index, &mut found).is_ok()
                        && found.as_bool()
                    {
                        break;
                    }
                    index = 0;
                }
                let length = names.GetStringLength(index).ok()? as usize;
                let mut value = vec![0; length + 1];
                names.GetString(index, &mut value).ok()?;
                String::from_utf16(&value[..length]).ok()
            })
            .collect()
    }
}

#[cfg(not(windows))]
fn installed_fonts() -> Vec<String> {
    let Ok(output) = std::process::Command::new("fc-list")
        .args(["-f", "%{family[0]}\n"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}
