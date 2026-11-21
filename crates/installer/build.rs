fn main() {
    embed_icon("sweet-ime.ico");
}

/// Embeds `assets/icon/<name>` as icon resource 1 in Windows builds.
fn embed_icon(name: &str) {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icon");
    let ico = dir.join(name).canonicalize().expect("icon file");
    let path = ico.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    let rc = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("icon.rc");
    std::fs::write(&rc, format!("1 ICON \"{path}\"\n")).unwrap();
    println!("cargo:rerun-if-changed={}", ico.display());
    embed_resource::compile(&rc, embed_resource::NONE).manifest_optional().unwrap();
}
